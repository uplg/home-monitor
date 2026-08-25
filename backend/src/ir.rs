//! IR keymap: maps remote keycodes to device actions.
//!
//! The AirTies STB decodes the Ruwido remote in hardware and its `kird`
//! daemon POSTs every key event to `/api/ir/key` (authenticated with the
//! `IR_API_TOKEN` machine token). This module owns the keycode -> action
//! table: loaded from `ir-keymap.json` at startup, edited at runtime through
//! the `/api/ir` routes (the frontend configurator), persisted on every
//! change. It also keeps a small ring buffer of the last received events so
//! the configurator can capture a key by asking the user to press it.
//!
//! Keymap file format (JSON object, keycodes as string keys, one binding =
//! a label + a LIST of actions fired in order):
//! ```json
//! {
//!   "207": {
//!     "label": "OK — taichi + lumière",
//!     "actions": [
//!       { "action": "nabaztag", "command": "chor taichi" },
//!       { "action": "zigbee_power", "lamp": "17ff040901881700", "on": false }
//!     ],
//!     "repeat": false
//!   }
//! }
//! ```

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::error::AppError;

const RECENT_EVENTS_CAP: usize = 50;

/// Presses of the same key closer than this are phantoms: when IR reception
/// drops a repeat frame mid-hold, the STB driver's release timer expires and
/// the next frame arrives as a fresh press — observed as double toggles
/// ~200-600 ms apart (and later when the button is held long under marginal
/// reception). A deliberate human re-press comes later than this.
const PRESS_DEBOUNCE: Duration = Duration::from_millis(1200);

/// What a switch-like action does to the device: force a state, or flip
/// whatever the current state is — the remote-control default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchState {
    On,
    Off,
    #[default]
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IrAction {
    Nabaztag {
        command: String,
    },
    ZigbeePower {
        lamp: String,
        #[serde(default)]
        state: SwitchState,
    },
    ZigbeeBrightness {
        lamp: String,
        brightness: u8,
    },
    BroadlinkCode {
        host: String,
        code_id: String,
    },
    MerossPower {
        device: String,
        #[serde(default)]
        state: SwitchState,
    },
    /// Powers the Philips TV through JointSPACE. `switch_to_box` also routes
    /// the set to the Android box's HDMI input on power-on, since the TV
    /// otherwise comes back on whatever source it was last left on.
    TvPower {
        #[serde(default)]
        state: SwitchState,
        /// Aliased: `rename_all` on the enum renames variants, not fields, so
        /// a camelCase key would otherwise be dropped without a word.
        #[serde(default = "default_true", alias = "switchToBox")]
        switch_to_box: bool,
    },
    /// Sends one remote-control key to the TV.
    TvKey {
        key: crate::tv::TvKey,
    },
    /// Absolute volume on the TV, preferred over repeated volume keys.
    TvVolume {
        level: u8,
    },
    TvAmbilight {
        #[serde(default)]
        state: SwitchState,
    },
    /// Launches an app on the Android TV box, powering the television on and
    /// routing it to the box first — a remote button that lights up a dark
    /// room's screen is the whole point.
    #[serde(rename = "androidtv_app")]
    AndroidTvApp {
        package: String,
        #[serde(default = "default_true", alias = "ensureTvOn")]
        ensure_tv_on: bool,
    },
    /// Sends one key to the Android TV box (D-pad, media, volume).
    #[serde(rename = "androidtv_key")]
    AndroidTvKey {
        key: crate::androidtv::AndroidKey,
    },
    /// Toggles the Mitsubishi AC through the Broadlink blaster: if the last
    /// commanded state left it on, sends `state-off`; otherwise sends
    /// `on_command` (a structured `state-…` command, e.g.
    /// `state-cool-16-fan-4-vane-swing`).
    /// Same reversibility for the AC, driven by the last commanded state
    /// (IR is one-way, so that is the best approximation available).
    ClimateToggle {
        host: String,
        on_command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
}

fn default_true() -> bool {
    true
}

/// Config-time validation so a typo'd binding fails at save, not at keypress.
pub fn validate_actions(actions: &[IrAction]) -> Result<(), String> {
    for action in actions {
        if let IrAction::AndroidTvApp { package, .. } = action {
            let valid = !package.is_empty()
                && package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            if !valid {
                return Err(format!(
                    "invalid Android package {package:?} (expected e.g. org.smarttube.beta)"
                ));
            }
        }
        if let IrAction::ClimateToggle { on_command, .. } = action {
            if crate::mitsubishi_ir::parse_climate_settings(on_command).is_none() {
                return Err(format!(
                    "invalid climate on_command {on_command:?} (expected e.g. \
                     state-cool-16-fan-4-vane-swing, and not state-off)"
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrBinding {
    /// Fired in order; one failing action does not stop the others.
    pub actions: Vec<IrAction>,
    /// Optional label shown in the configurator (e.g. "OK button").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Also fire on kernel autorepeat events (value == 2) — for held keys
    /// like dim up/down. Presses (value == 1) always fire; releases never do.
    #[serde(default)]
    pub repeat: bool,
}

/// One received key event, kept for the configurator's capture flow.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrEventLog {
    pub code: u16,
    pub value: i32,
    pub mapped: bool,
    pub received_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct IrManager {
    path: Arc<PathBuf>,
    keymap: Arc<RwLock<HashMap<u16, IrBinding>>>,
    recent: Arc<Mutex<VecDeque<IrEventLog>>>,
    last_press: Arc<Mutex<HashMap<u16, Instant>>>,
}

impl IrManager {
    /// Missing file = empty keymap (bindings are created from the UI).
    /// A present but invalid file is a startup error: a corrupt keymap
    /// should not silently disable the remote.
    pub fn new(path: &Path) -> Result<Self, AppError> {
        let keymap = match std::fs::read_to_string(path) {
            Ok(content) => parse_keymap(&content).map_err(|error| {
                AppError::http(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Invalid IR keymap {}: {error}", path.display()),
                )
            })?,
            Err(_) => {
                tracing::debug!(path = %path.display(), "no IR keymap yet, starting empty");
                HashMap::new()
            }
        };
        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            keymap: Arc::new(RwLock::new(keymap)),
            recent: Arc::new(Mutex::new(VecDeque::with_capacity(RECENT_EVENTS_CAP))),
            last_press: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Returns `false` when this press is a phantom double (see
    /// [`PRESS_DEBOUNCE`]); an accepted press starts the next window.
    /// Autorepeat events are not debounced — they are the point of `repeat`.
    pub async fn accept_press(&self, code: u16) -> bool {
        let mut map = self.last_press.lock().await;
        let now = Instant::now();
        match map.get(&code) {
            Some(previous) if now.duration_since(*previous) < PRESS_DEBOUNCE => false,
            _ => {
                map.insert(code, now);
                true
            }
        }
    }

    pub async fn binding(&self, code: u16) -> Option<IrBinding> {
        self.keymap.read().await.get(&code).cloned()
    }

    pub async fn keymap(&self) -> HashMap<u16, IrBinding> {
        self.keymap.read().await.clone()
    }

    pub async fn set_binding(&self, code: u16, binding: IrBinding) -> Result<(), AppError> {
        let mut map = self.keymap.write().await;
        map.insert(code, binding);
        self.persist(&map)
    }

    /// Returns `true` when a binding existed and was removed.
    pub async fn remove_binding(&self, code: u16) -> Result<bool, AppError> {
        let mut map = self.keymap.write().await;
        let removed = map.remove(&code).is_some();
        if removed {
            self.persist(&map)?;
        }
        Ok(removed)
    }

    pub async fn record_event(&self, code: u16, value: i32, mapped: bool) {
        let mut recent = self.recent.lock().await;
        if recent.len() == RECENT_EVENTS_CAP {
            recent.pop_front();
        }
        recent.push_back(IrEventLog {
            code,
            value,
            mapped,
            received_at: Utc::now(),
        });
    }

    /// Most recent first.
    pub async fn recent_events(&self) -> Vec<IrEventLog> {
        self.recent.lock().await.iter().rev().cloned().collect()
    }

    fn persist(&self, map: &HashMap<u16, IrBinding>) -> Result<(), AppError> {
        // String keys so the file round-trips through parse_keymap.
        let as_strings: HashMap<String, &IrBinding> = map
            .iter()
            .map(|(code, binding)| (code.to_string(), binding))
            .collect();
        let payload = serde_json::to_string_pretty(&as_strings).map_err(|error| {
            AppError::http(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize IR keymap: {error}"),
            )
        })?;
        std::fs::write(self.path.as_ref(), format!("{payload}\n")).map_err(|error| {
            AppError::http(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to write {}: {error}", self.path.display()),
            )
        })
    }
}

pub fn parse_keymap(content: &str) -> Result<HashMap<u16, IrBinding>, String> {
    let raw: HashMap<String, IrBinding> =
        serde_json::from_str(content.trim()).map_err(|error| error.to_string())?;
    raw.into_iter()
        .map(|(key, binding)| {
            key.parse::<u16>()
                .map(|code| (code, binding))
                .map_err(|_| format!("keycode {key:?} is not a u16"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_action_kinds_and_multi_action_bindings() {
        let map = parse_keymap(
            r#"{
                "207": { "label": "OK", "actions": [
                    { "action": "nabaztag", "command": "chor taichi" },
                    { "action": "zigbee_power", "lamp": "abc" }
                ]},
                "115": { "actions": [
                    { "action": "zigbee_brightness", "lamp": "abc", "brightness": 128 }
                ], "repeat": true },
                "2": { "actions": [
                    { "action": "broadlink_code", "host": "192.168.1.2", "code_id": "tv-power" },
                    { "action": "meross_power", "device": "192.168.1.3", "state": "off" }
                ]}
            }"#,
        )
        .expect("valid keymap");
        assert_eq!(map.len(), 3);
        assert_eq!(map[&207].actions.len(), 2);
        assert_eq!(
            map[&207].actions[0],
            IrAction::Nabaztag {
                command: "chor taichi".to_string()
            }
        );
        assert_eq!(
            map[&207].actions[1],
            IrAction::ZigbeePower {
                lamp: "abc".to_string(),
                state: SwitchState::Toggle
            },
            "omitted state defaults to toggle"
        );
        assert_eq!(
            map[&2].actions[1],
            IrAction::MerossPower {
                device: "192.168.1.3".to_string(),
                state: SwitchState::Off
            }
        );
        assert_eq!(map[&207].label.as_deref(), Some("OK"));
        assert!(!map[&207].repeat, "repeat defaults to false");
        assert!(map[&115].repeat);
    }

    #[test]
    fn rejects_non_numeric_keycode() {
        let error = parse_keymap(
            r#"{ "power": { "actions": [{ "action": "nabaztag", "command": "ping" }] } }"#,
        )
        .expect_err("keycode must be numeric");
        assert!(error.contains("power"));
    }

    #[test]
    fn parses_and_validates_climate_toggle() {
        let map = parse_keymap(
            r#"{ "353": { "actions": [{ "action": "climate_toggle",
                "host": "192.168.1.2", "on_command": "state-cool-16-fan-4-vane-swing" }] } }"#,
        )
        .expect("valid climate binding");
        assert!(validate_actions(&map[&353].actions).is_ok());

        // state-off as on_command makes the toggle a no-op: refused at save.
        let off = vec![IrAction::ClimateToggle {
            host: "h".to_string(),
            on_command: "state-off".to_string(),
            model: None,
        }];
        assert!(validate_actions(&off).is_err());

        let garbage = vec![IrAction::ClimateToggle {
            host: "h".to_string(),
            on_command: "state-cool-99-fan-4-vane-swing".to_string(),
            model: None,
        }];
        assert!(validate_actions(&garbage).is_err(), "temp out of range");
    }

    /// The TV bindings are the ones a user hand-writes most often, so pin
    /// their JSON shape — including the camelCase `switchToBox` and the
    /// snake_case key names.
    #[test]
    fn parses_tv_actions() {
        let keymap = parse_keymap(
            r#"{
                "9": { "actions": [
                    { "action": "tv_power", "state": "toggle", "switchToBox": true },
                    { "action": "tv_key", "key": "play_pause" },
                    { "action": "tv_volume", "level": 22 },
                    { "action": "tv_ambilight", "state": "off" }
                ] }
            }"#,
        )
        .expect("TV actions should parse");

        let actions = &keymap.get(&9).expect("binding 9").actions;
        assert_eq!(
            actions[0],
            IrAction::TvPower {
                state: SwitchState::Toggle,
                switch_to_box: true,
            }
        );
        assert_eq!(
            actions[1],
            IrAction::TvKey {
                key: crate::tv::TvKey::PlayPause,
            }
        );
        assert_eq!(actions[2], IrAction::TvVolume { level: 22 });
        assert_eq!(
            actions[3],
            IrAction::TvAmbilight {
                state: SwitchState::Off,
            }
        );
    }

    /// Switching to the box input is the sane default: the set otherwise
    /// wakes on whatever source it was last left on.
    #[test]
    fn tv_power_defaults_to_switching_to_the_box() {
        let keymap =
            parse_keymap(r#"{ "9": { "actions": [{ "action": "tv_power" }] } }"#).expect("parses");
        assert_eq!(
            keymap.get(&9).expect("binding").actions[0],
            IrAction::TvPower {
                state: SwitchState::Toggle,
                switch_to_box: true,
            }
        );
    }

    /// `rename_all = "snake_case"` on an enum renames *variants*, not the
    /// fields inside them, so a camelCase key is dropped silently unless it is
    /// aliased. Both spellings must reach the field — asserted with the
    /// NON-default value, or the test would pass on the default alone.
    #[test]
    fn both_field_spellings_are_honoured() {
        for body in [
            r#"{ "9": { "actions": [{ "action": "tv_power", "switchToBox": false }] } }"#,
            r#"{ "9": { "actions": [{ "action": "tv_power", "switch_to_box": false }] } }"#,
        ] {
            let keymap = parse_keymap(body).expect("parses");
            assert_eq!(
                keymap.get(&9).expect("binding").actions[0],
                IrAction::TvPower {
                    state: SwitchState::Toggle,
                    switch_to_box: false,
                },
                "spelling was ignored in {body}"
            );
        }

        for body in [
            r#"{ "9": { "actions": [{ "action": "androidtv_app", "package": "a.b", "ensureTvOn": false }] } }"#,
            r#"{ "9": { "actions": [{ "action": "androidtv_app", "package": "a.b", "ensure_tv_on": false }] } }"#,
        ] {
            let keymap = parse_keymap(body).expect("parses");
            assert_eq!(
                keymap.get(&9).expect("binding").actions[0],
                IrAction::AndroidTvApp {
                    package: "a.b".to_string(),
                    ensure_tv_on: false,
                },
                "spelling was ignored in {body}"
            );
        }
    }

    #[test]
    fn rejects_unknown_action() {
        assert!(
            parse_keymap(r#"{ "1": { "actions": [{ "action": "teleport", "target": "moon" }] } }"#)
                .is_err()
        );
    }

    #[tokio::test]
    async fn debounces_phantom_double_press_per_key() {
        let path = std::env::temp_dir()
            .join("maison-ir-unit")
            .join(format!("{}-debounce.json", uuid::Uuid::new_v4()));
        let manager = IrManager::new(&path).expect("empty manager");
        assert!(manager.accept_press(116).await, "first press fires");
        assert!(
            !manager.accept_press(116).await,
            "immediate second press is a phantom"
        );
        assert!(manager.accept_press(117).await, "other keys are independent");
    }

    #[tokio::test]
    async fn set_remove_persist_roundtrip() {
        let dir = std::env::temp_dir()
            .join("maison-ir-unit")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("ir-keymap.json");

        let manager = IrManager::new(&path).expect("empty manager");
        manager
            .set_binding(
                207,
                IrBinding {
                    actions: vec![IrAction::Nabaztag {
                        command: "chor taichi".to_string(),
                    }],
                    label: Some("OK".to_string()),
                    repeat: false,
                },
            )
            .await
            .expect("binding saved");

        // A fresh manager sees the persisted binding.
        let reloaded = IrManager::new(&path).expect("reload");
        assert!(reloaded.binding(207).await.is_some());

        assert!(reloaded.remove_binding(207).await.expect("removed"));
        assert!(!reloaded.remove_binding(207).await.expect("idempotent"));
        let reloaded_again = IrManager::new(&path).expect("reload again");
        assert!(reloaded_again.binding(207).await.is_none());
    }
}

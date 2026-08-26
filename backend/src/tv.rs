//! Philips TV control over the JointSPACE API.
//!
//! The living-room set (55PUS6753/12) runs Saphi, not Android TV, which is
//! what makes this tractable: the API answers plain HTTP on port 1925 with
//! `pairing_type: "none"` — no pairing, no digest auth, no certificate.
//! Android-branch Philips sets would need HTTPS on 1926 plus a paired
//! credential; nothing here would apply to them.
//!
//! Two hard-won constraints shape this module, both discovered by breaking the
//! set and having to power-cycle it from the mains:
//!
//!  1. **Only whitelisted endpoints may be touched.** Saphi answers `Forbidden`
//!     or `Not Found` on the endpoints it does not implement (`/6/sources`,
//!     `/6/applications`, `/6/activities/*`, anything under `/5/`), and hitting
//!     them repeatedly kills the embedded server *persistently*: neither a
//!     standby cycle nor the API's own `Standby` brings it back, only pulling
//!     the mains for ~30 s. So the reachable surface is an enum, not a string —
//!     an unsupported path is unrepresentable rather than merely discouraged.
//!  2. **Requests are serialized and spaced.** The server is single-threaded
//!     and fragile; a burst is what killed it in the first place. Every call
//!     goes through one gate holding at least `MIN_REQUEST_GAP` between hits.
//!
//! Power is the one thing this module does *not* do over the API. The set has
//! two sleep depths — *light* standby still answers on 1925
//! (`powerstate: "Standby"`), *deep* standby drops the network stack entirely
//! — and from the deep one nothing on the network can reach it. Wake-on-LAN
//! does not work despite the SSDP `WAKEUP` header: the interface is powered
//! down, and even raw layer-2 magic packets, which bypass all IP routing, go
//! unanswered. CEC is out too, the set having left the bus. So power on and
//! off both go through infrared, whose receiver stays live at every depth and
//! keeps working when the JointSPACE server has crashed. See `philips_ir`.

use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock},
    time::{timeout, Instant},
};

use crate::{broadlink::BroadlinkManager, error::AppError, philips_ir};

const API_PORT: u16 = 1925;
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);
/// A TCP connect is the cheapest way to tell the sleep depths apart, and it
/// never touches the HTTP server.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1_500);
/// Minimum spacing between two calls to the TV. See the module note on the
/// single-threaded server.
const MIN_REQUEST_GAP: Duration = Duration::from_millis(900);
/// Deep standby took ~20 s to answer again in practice; leave generous margin.
const WAKE_TIMEOUT: Duration = Duration::from_secs(45);
const WAKE_POLL_GAP: Duration = Duration::from_secs(3);
/// How long to let the set catch up with a power-on before reporting back.
/// It acknowledges the write well before `powerstate` reflects it.
const POWER_SETTLE: Duration = Duration::from_secs(6);
const POWER_POLL_GAP: Duration = Duration::from_millis(900);
/// DIAL on the Android box. Waking the box makes it assert CEC One Touch Play,
/// which powers the TV on *and* switches it to the box's HDMI input.
const BOX_DIAL_PORT: u16 = 8008;

/// Every endpoint this module is allowed to touch, verified against the set.
/// Adding a variant means having verified it answers on Saphi — see the module
/// note before extending this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    System,
    PowerState,
    AudioVolume,
    InputKey,
    AmbilightPower,
    AmbilightMode,
    AmbilightTopology,
    AmbilightSupportedStyles,
    AmbilightCurrentConfiguration,
}

impl Endpoint {
    fn path(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::PowerState => "powerstate",
            Self::AudioVolume => "audio/volume",
            Self::InputKey => "input/key",
            Self::AmbilightPower => "ambilight/power",
            Self::AmbilightMode => "ambilight/mode",
            Self::AmbilightTopology => "ambilight/topology",
            Self::AmbilightSupportedStyles => "ambilight/supportedstyles",
            Self::AmbilightCurrentConfiguration => "ambilight/currentconfiguration",
        }
    }
}

/// Remote-control keys accepted by `/6/input/key`. Kept as an enum for the same
/// reason as `Endpoint`: an unknown key name is a rejected request, and a
/// rejected request is a step towards a dead server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TvKey {
    Standby,
    Back,
    Home,
    Source,
    WatchTv,
    Confirm,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    VolumeUp,
    VolumeDown,
    Mute,
    ChannelStepUp,
    ChannelStepDown,
    PlayPause,
    Pause,
    Stop,
    FastForward,
    Rewind,
    Next,
    Previous,
    Info,
    Options,
    Subtitle,
    Teletext,
    AmbilightOnOff,
}

impl TvKey {
    /// The wire name, which is CamelCase and not always the obvious casing
    /// (`WatchTV`, not `WatchTv`).
    fn wire_name(self) -> &'static str {
        match self {
            Self::Standby => "Standby",
            Self::Back => "Back",
            Self::Home => "Home",
            Self::Source => "Source",
            Self::WatchTv => "WatchTV",
            Self::Confirm => "Confirm",
            Self::CursorUp => "CursorUp",
            Self::CursorDown => "CursorDown",
            Self::CursorLeft => "CursorLeft",
            Self::CursorRight => "CursorRight",
            Self::VolumeUp => "VolumeUp",
            Self::VolumeDown => "VolumeDown",
            Self::Mute => "Mute",
            Self::ChannelStepUp => "ChannelStepUp",
            Self::ChannelStepDown => "ChannelStepDown",
            Self::PlayPause => "PlayPause",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::FastForward => "FastForward",
            Self::Rewind => "Rewind",
            Self::Next => "Next",
            Self::Previous => "Previous",
            Self::Info => "Info",
            Self::Options => "Options",
            Self::Subtitle => "Subtitle",
            Self::Teletext => "Teletext",
            Self::AmbilightOnOff => "AmbilightOnOff",
        }
    }
}

/// How awake the set is. The distinction is diagnostic only now that power
/// goes over infrared, which reaches the set at either depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TvPower {
    On,
    /// Panel off, JointSPACE still answering.
    Standby,
    /// Network stack down — the API port refuses or times out.
    DeepStandby,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TvConfig {
    /// TV address, e.g. `192.168.1.52`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Broadlink IR blaster that fronts the set, e.g. `192.168.1.73`. Without
    /// it the set cannot be powered on or off at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ir_blaster_host: Option<String>,
    /// Android TV box address. Used to force the HDMI input via CEC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_host: Option<String>,
    /// DIAL app woken on the box to trigger CEC One Touch Play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_wake_app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvVolume {
    pub current: u8,
    pub min: u8,
    pub max: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvAmbilight {
    pub power: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvStatus {
    pub configured: bool,
    pub power: TvPower,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<TvVolume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambilight: Option<TvAmbilight>,
}

#[derive(Clone)]
pub struct TvManager {
    config_path: PathBuf,
    config: Arc<RwLock<TvConfig>>,
    client: reqwest::Client,
    /// The request gate: held across every call so requests are both
    /// serialized and spaced by at least `MIN_REQUEST_GAP`.
    gate: Arc<Mutex<Option<Instant>>>,
    /// Blaster fronting the set. Power goes through it rather than the API,
    /// infrared being the only channel that survives deep standby.
    broadlink: BroadlinkManager,
    /// RC5 toggle bit, flipped on every code sent.
    ir_toggle: Arc<AtomicBool>,
}

impl TvManager {
    pub fn new(config_path: &Path, broadlink: BroadlinkManager) -> Result<Self, AppError> {
        let config = match std::fs::read_to_string(config_path) {
            Ok(content) if !content.trim().is_empty() => serde_json::from_str(&content)
                .map_err(|error| {
                    AppError::http(
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("invalid TV config at {}: {error}", config_path.display()),
                    )
                })?,
            // A missing or empty file is the normal unconfigured state, not an
            // error: the shelf renders and offers to fill it in.
            _ => TvConfig::default(),
        };

        Ok(Self {
            config_path: config_path.to_path_buf(),
            config: Arc::new(RwLock::new(config)),
            client: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .map_err(AppError::Reqwest)?,
            gate: Arc::new(Mutex::new(None)),
            broadlink,
            ir_toggle: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn config(&self) -> TvConfig {
        self.config.read().await.clone()
    }

    pub async fn set_config(&self, config: TvConfig) -> Result<TvConfig, AppError> {
        if let Some(host) = config.ir_blaster_host.as_deref() {
            // Broadlink discovery speaks IPv4 only, so a hostname here would
            // fail later, at the point of trying to switch the set on.
            host.parse::<std::net::Ipv4Addr>().map_err(|_| {
                AppError::http(
                    axum::http::StatusCode::BAD_REQUEST,
                    format!("invalid IR blaster address {host:?} (expected an IPv4 address)"),
                )
            })?;
        }
        let serialized = serde_json::to_string_pretty(&config)?;
        std::fs::write(&self.config_path, serialized)?;
        *self.config.write().await = config.clone();
        Ok(config)
    }

    async fn host(&self) -> Result<String, AppError> {
        self.config
            .read()
            .await
            .host
            .clone()
            .ok_or_else(|| AppError::service_unavailable("No TV configured"))
    }

    /// Waits out the inter-request gap, then reports the moment the caller may
    /// hit the wire. The guard is held for the duration of the request.
    async fn acquire(&self) -> tokio::sync::MutexGuard<'_, Option<Instant>> {
        let mut gate = self.gate.lock().await;
        if let Some(last) = *gate {
            let elapsed = last.elapsed();
            if elapsed < MIN_REQUEST_GAP {
                tokio::time::sleep(MIN_REQUEST_GAP - elapsed).await;
            }
        }
        *gate = Some(Instant::now());
        gate
    }

    async fn get(&self, endpoint: Endpoint) -> Result<serde_json::Value, AppError> {
        let host = self.host().await?;
        let _guard = self.acquire().await;
        let url = format!("http://{host}:{API_PORT}/6/{}", endpoint.path());
        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(AppError::service_unavailable(format!(
                "TV refused GET {} ({})",
                endpoint.path(),
                response.status()
            )));
        }
        Ok(response.json().await?)
    }

    async fn post(&self, endpoint: Endpoint, body: serde_json::Value) -> Result<(), AppError> {
        let host = self.host().await?;
        let _guard = self.acquire().await;
        let url = format!("http://{host}:{API_PORT}/6/{}", endpoint.path());
        let response = self.client.post(&url).json(&body).send().await?;
        if !response.status().is_success() {
            return Err(AppError::service_unavailable(format!(
                "TV refused POST {} ({})",
                endpoint.path(),
                response.status()
            )));
        }
        Ok(())
    }

    /// Is the API port answering? A plain TCP connect, so it costs the HTTP
    /// server nothing and tells light standby from deep standby.
    async fn api_reachable(&self) -> bool {
        let Ok(host) = self.host().await else {
            return false;
        };
        let Ok(addr) = format!("{host}:{API_PORT}").parse::<SocketAddr>().or_else(|_| {
            host.parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, API_PORT))
        }) else {
            return false;
        };
        matches!(
            timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await,
            Ok(Ok(_))
        )
    }

    pub async fn power(&self) -> TvPower {
        if !self.api_reachable().await {
            return TvPower::DeepStandby;
        }
        match self.get(Endpoint::PowerState).await {
            Ok(value) => match value.get("powerstate").and_then(|v| v.as_str()) {
                Some("On") => TvPower::On,
                // Saphi reports "Standby"; treat anything else answered by a
                // live API as standby rather than guessing.
                _ => TvPower::Standby,
            },
            Err(_) => TvPower::DeepStandby,
        }
    }

    /// Full snapshot for the dashboard shelf. Volume and Ambilight are only
    /// read when the panel is actually on — they are meaningless otherwise and
    /// would spend gate time for nothing.
    pub async fn status(&self) -> TvStatus {
        let configured = self.config.read().await.host.is_some();
        if !configured {
            return TvStatus {
                configured: false,
                power: TvPower::DeepStandby,
                name: None,
                volume: None,
                ambilight: None,
            };
        }

        let power = self.power().await;
        if power != TvPower::On {
            return TvStatus {
                configured: true,
                power,
                name: None,
                volume: None,
                ambilight: None,
            };
        }

        let name = self
            .get(Endpoint::System)
            .await
            .ok()
            .and_then(|value| value.get("name")?.as_str().map(str::to_string));

        TvStatus {
            configured: true,
            power,
            name,
            volume: self.volume().await.ok(),
            ambilight: self.ambilight().await.ok(),
        }
    }

    pub async fn volume(&self) -> Result<TvVolume, AppError> {
        let value = self.get(Endpoint::AudioVolume).await?;
        Ok(TvVolume {
            current: value.get("current").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            min: value.get("min").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            max: value.get("max").and_then(|v| v.as_u64()).unwrap_or(60) as u8,
            muted: value
                .get("muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    /// Absolute volume. Preferred over repeated `VolumeUp` keys: the key path
    /// is fire-and-forget and its effect lags the readback by about a step.
    pub async fn set_volume(&self, level: u8, muted: Option<bool>) -> Result<TvVolume, AppError> {
        let current = self.volume().await?;
        let level = level.clamp(current.min, current.max);
        self.post(
            Endpoint::AudioVolume,
            json!({ "muted": muted.unwrap_or(current.muted), "current": level }),
        )
        .await?;
        self.volume().await
    }

    pub async fn send_key(&self, key: TvKey) -> Result<(), AppError> {
        self.post(Endpoint::InputKey, json!({ "key": key.wire_name() }))
            .await
    }

    pub async fn ambilight(&self) -> Result<TvAmbilight, AppError> {
        let power = self
            .get(Endpoint::AmbilightPower)
            .await?
            .get("power")
            .and_then(|v| v.as_str())
            .map(|v| v.eq_ignore_ascii_case("on"))
            .unwrap_or(false);
        let mode = self
            .get(Endpoint::AmbilightMode)
            .await
            .ok()
            .and_then(|v| v.get("current")?.as_str().map(str::to_string));
        let configuration = self.get(Endpoint::AmbilightCurrentConfiguration).await.ok();

        Ok(TvAmbilight {
            power,
            mode,
            style: configuration
                .as_ref()
                .and_then(|v| v.get("styleName")?.as_str().map(str::to_string)),
            setting: configuration
                .as_ref()
                .and_then(|v| v.get("stringValue")?.as_str().map(str::to_string)),
        })
    }

    pub async fn set_ambilight_power(&self, on: bool) -> Result<(), AppError> {
        self.post(
            Endpoint::AmbilightPower,
            json!({ "power": if on { "On" } else { "Off" } }),
        )
        .await
    }

    pub async fn ambilight_styles(&self) -> Result<serde_json::Value, AppError> {
        self.get(Endpoint::AmbilightSupportedStyles).await
    }

    pub async fn ambilight_topology(&self) -> Result<serde_json::Value, AppError> {
        self.get(Endpoint::AmbilightTopology).await
    }

    /// Powers the set on, whatever depth it is sleeping at, and optionally
    /// leaves it on the box's HDMI input.
    ///
    /// One channel only: infrared reaches the set at either sleep depth and
    /// keeps working when JointSPACE has crashed, which is more than the API,
    /// Wake-on-LAN or CEC can each claim. The code is discrete rather than a
    /// toggle, so firing it at a set that is already on is a no-op.
    pub async fn power_on(&self, switch_to_box: bool) -> Result<TvPower, AppError> {
        self.send_power_code(philips_ir::TV_POWER_ON).await?;

        // The set rejoins the network several seconds before JointSPACE starts
        // listening, and once that server has crashed it never listens again
        // until the mains are pulled. So this wait is best-effort rather than a
        // precondition: a discrete power-on code cannot have left the set
        // anywhere but on, and reporting failure because the API stayed quiet
        // would be reporting on the wrong thing.
        let reachable = self.await_api(WAKE_TIMEOUT).await;
        if !reachable {
            tracing::warn!(
                "TV switched on over infrared, but JointSPACE never answered — \
                 the embedded server needs a mains power cycle"
            );
        }

        if switch_to_box {
            // Best-effort: the set is on either way, and the box may simply
            // not be configured. This runs over CEC, so it works even with the
            // API down.
            if let Err(error) = self.switch_to_box().await {
                tracing::debug!(%error, "could not switch the TV to the box input");
            }
        }

        if !reachable {
            return Ok(TvPower::On);
        }

        // The set lags its own writes: reading `powerstate` straight back
        // reports the previous value, so a successful power-on looked like it
        // had left the TV in standby. Give it a moment to catch up rather than
        // reporting a state we know to be stale.
        let deadline = Instant::now() + POWER_SETTLE;
        while Instant::now() < deadline {
            if self.power().await == TvPower::On {
                break;
            }
            tokio::time::sleep(POWER_POLL_GAP).await;
        }

        Ok(self.power().await)
    }

    /// Polls the API until it answers or the budget runs out.
    async fn await_api(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            if self.api_reachable().await {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(WAKE_POLL_GAP).await;
        }
    }

    /// Fires one Philips RC5 power code through the configured blaster.
    async fn send_power_code(&self, command: u8) -> Result<(), AppError> {
        let host = self
            .config
            .read()
            .await
            .ir_blaster_host
            .clone()
            .ok_or_else(|| {
                AppError::service_unavailable("No IR blaster configured for the TV")
            })?;

        // RC5 wants the toggle bit to flip between presses; a receiver seeing
        // it unchanged reads the second frame as a held key and drops it.
        let toggle = self.ir_toggle.fetch_xor(true, Ordering::Relaxed);
        let packet = philips_ir::encode_rc5(philips_ir::TV_ADDRESS, command, toggle);

        tracing::info!(
            command = format!("0x{command:02X}"),
            %host,
            toggle,
            "sending a TV power code over infrared"
        );

        self.broadlink
            .send_packet(host, None, STANDARD.encode(&packet), None, None)
            .await
            .map(|_| ())
    }

    /// Switches the set off.
    ///
    /// Infrared again rather than the API's `Standby` key, for the symmetry as
    /// much as the reach: one channel to reason about, and it still works on
    /// the day JointSPACE is down.
    pub async fn power_off(&self) -> Result<TvPower, AppError> {
        self.send_power_code(philips_ir::TV_POWER_OFF).await?;
        Ok(TvPower::Standby)
    }

    /// Nudges the Android box awake over DIAL so it asserts CEC One Touch
    /// Play, which both powers the set and routes it to the box's HDMI input.
    ///
    /// This is what fixes "the TV came up on the wrong input": JointSPACE
    /// cannot switch sources at all on Saphi (`/6/sources` is `Forbidden`), so
    /// the input has to be driven from the HDMI side.
    pub async fn switch_to_box(&self) -> Result<(), AppError> {
        let config = self.config.read().await.clone();
        let host = config
            .box_host
            .ok_or_else(|| AppError::service_unavailable("No Android TV box configured"))?;
        let app = config.box_wake_app.unwrap_or_else(|| "YouTube".to_string());
        let url = format!("http://{host}:{BOX_DIAL_PORT}/apps/{app}");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body("")
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AppError::service_unavailable(format!(
                "box refused the DIAL wake ({})",
                response.status()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manager needs a blaster to hold, but no test here sends a code, so
    /// it only has to exist and point at scratch paths.
    fn test_broadlink(dir: &tempfile::TempDir) -> BroadlinkManager {
        BroadlinkManager::new(&dir.path().join("codes.json"), &dir.path().join("climate.json"))
            .expect("broadlink")
    }

    /// The wire spelling is not derivable from the variant name, so it is
    /// worth pinning: `WatchTV` carries a capital V.
    #[test]
    fn key_wire_names_match_the_api_spelling() {
        assert_eq!(TvKey::WatchTv.wire_name(), "WatchTV");
        assert_eq!(TvKey::Standby.wire_name(), "Standby");
        assert_eq!(TvKey::AmbilightOnOff.wire_name(), "AmbilightOnOff");
    }

    /// Guards the module's core safety property: every reachable path is one
    /// the set actually implements. A typo here is a bricked API until someone
    /// pulls the mains, so the list is asserted rather than assumed.
    #[test]
    fn endpoints_stay_within_the_verified_whitelist() {
        let allowed = [
            "system",
            "powerstate",
            "audio/volume",
            "input/key",
            "ambilight/power",
            "ambilight/mode",
            "ambilight/topology",
            "ambilight/supportedstyles",
            "ambilight/currentconfiguration",
        ];
        for endpoint in [
            Endpoint::System,
            Endpoint::PowerState,
            Endpoint::AudioVolume,
            Endpoint::InputKey,
            Endpoint::AmbilightPower,
            Endpoint::AmbilightMode,
            Endpoint::AmbilightTopology,
            Endpoint::AmbilightSupportedStyles,
            Endpoint::AmbilightCurrentConfiguration,
        ] {
            assert!(
                allowed.contains(&endpoint.path()),
                "{} is not a verified endpoint",
                endpoint.path()
            );
        }
    }

    #[tokio::test]
    async fn missing_config_file_is_an_empty_config_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager =
            TvManager::new(&dir.path().join("tv.json"), test_broadlink(&dir)).expect("manager");
        assert!(manager.config().await.host.is_none());
        assert!(!manager.status().await.configured);
    }

    #[tokio::test]
    async fn set_config_persists_and_rejects_bad_mac() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tv.json");
        let manager = TvManager::new(&path, test_broadlink(&dir)).expect("manager");

        manager
            .set_config(TvConfig {
                host: Some("192.168.1.52".to_string()),
                ir_blaster_host: Some("192.168.1.73".to_string()),
                box_host: Some("192.168.1.153".to_string()),
                box_wake_app: None,
            })
            .await
            .expect("saved");

        let reloaded = TvManager::new(&path, test_broadlink(&dir))
            .expect("reload")
            .config()
            .await;
        assert_eq!(reloaded.host.as_deref(), Some("192.168.1.52"));
        assert_eq!(reloaded.box_host.as_deref(), Some("192.168.1.153"));

        let rejected = manager
            .set_config(TvConfig {
                host: Some("192.168.1.52".to_string()),
                ir_blaster_host: Some("nope".to_string()),
                box_host: None,
                box_wake_app: None,
            })
            .await;
        assert!(rejected.is_err());
    }

    /// The gate is the other half of the safety story: bursts are what killed
    /// the server, so two consecutive acquisitions must be spaced.
    #[tokio::test]
    async fn request_gate_spaces_consecutive_calls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager =
            TvManager::new(&dir.path().join("tv.json"), test_broadlink(&dir)).expect("manager");

        let start = Instant::now();
        drop(manager.acquire().await);
        drop(manager.acquire().await);
        assert!(
            start.elapsed() >= MIN_REQUEST_GAP,
            "second call was not spaced (elapsed {:?})",
            start.elapsed()
        );
    }
}

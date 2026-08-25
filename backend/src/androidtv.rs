//! Android TV box control over ADB.
//!
//! The living-room box (MECOOL LEAP-S1, Android 14) has network debugging
//! enabled, and `adb.rs` speaks the protocol natively — so this module is
//! mostly a vocabulary: remote keys, app launching, power, and the CEC lever
//! that routes the television to the box's own HDMI input.
//!
//! Two design notes:
//!
//!  - **Commands are never assembled from free-form input.** Keys are an enum
//!    and package names are validated, so nothing a client sends can turn into
//!    arbitrary shell.
//!  - **The connection is kept and reused.** The handshake costs an RSA
//!    signature, which is cheap on a laptop and distinctly not on an ARMv6 Pi;
//!    a dropped connection is re-established once, transparently.
//!
//! The signing key is generated on first use and persisted. Generating 2048
//! bits takes tens of seconds on a Pi 1, so it happens off the async runtime,
//! and the box will show one "Allow USB debugging?" prompt the first time the
//! backend talks to it.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rsa::RsaPrivateKey;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::{
    adb::{self, AdbDevice},
    atvremote::{Identity, Pairing, Session},
    error::AppError,
};

const DEFAULT_ADB_PORT: u16 = 5555;

/// Remote-control keys, mapped to Android keycodes. An enum rather than a
/// string so no caller can smuggle shell syntax into `input keyevent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidKey {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Home,
    Menu,
    Search,
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Rewind,
    FastForward,
    ChannelUp,
    ChannelDown,
    Power,
    Sleep,
    Wakeup,
}

impl AndroidKey {
    /// Numeric Android keycode, which is what the Remote v2 protocol carries
    /// (ADB takes the symbolic name instead — same key, two spellings).
    fn android_keycode(self) -> i64 {
        match self {
            Self::Home => 3,
            Self::Back => 4,
            Self::Up => 19,
            Self::Down => 20,
            Self::Left => 21,
            Self::Right => 22,
            Self::Ok => 23,
            Self::VolumeUp => 24,
            Self::VolumeDown => 25,
            Self::Power => 26,
            Self::Menu => 82,
            Self::Search => 84,
            Self::PlayPause => 85,
            Self::Stop => 86,
            Self::Next => 87,
            Self::Previous => 88,
            Self::Rewind => 89,
            Self::FastForward => 90,
            Self::Play => 126,
            Self::Pause => 127,
            Self::Mute => 164,
            Self::ChannelUp => 166,
            Self::ChannelDown => 167,
            Self::Sleep => 223,
            Self::Wakeup => 224,
        }
    }

    fn keycode(self) -> &'static str {
        match self {
            Self::Up => "KEYCODE_DPAD_UP",
            Self::Down => "KEYCODE_DPAD_DOWN",
            Self::Left => "KEYCODE_DPAD_LEFT",
            Self::Right => "KEYCODE_DPAD_RIGHT",
            Self::Ok => "KEYCODE_DPAD_CENTER",
            Self::Back => "KEYCODE_BACK",
            Self::Home => "KEYCODE_HOME",
            Self::Menu => "KEYCODE_MENU",
            Self::Search => "KEYCODE_SEARCH",
            Self::VolumeUp => "KEYCODE_VOLUME_UP",
            Self::VolumeDown => "KEYCODE_VOLUME_DOWN",
            Self::Mute => "KEYCODE_VOLUME_MUTE",
            Self::PlayPause => "KEYCODE_MEDIA_PLAY_PAUSE",
            Self::Play => "KEYCODE_MEDIA_PLAY",
            Self::Pause => "KEYCODE_MEDIA_PAUSE",
            Self::Stop => "KEYCODE_MEDIA_STOP",
            Self::Next => "KEYCODE_MEDIA_NEXT",
            Self::Previous => "KEYCODE_MEDIA_PREVIOUS",
            Self::Rewind => "KEYCODE_MEDIA_REWIND",
            Self::FastForward => "KEYCODE_MEDIA_FAST_FORWARD",
            Self::ChannelUp => "KEYCODE_CHANNEL_UP",
            Self::ChannelDown => "KEYCODE_CHANNEL_DOWN",
            Self::Power => "KEYCODE_POWER",
            Self::Sleep => "KEYCODE_SLEEP",
            Self::Wakeup => "KEYCODE_WAKEUP",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidTvConfig {
    /// Box address, e.g. `192.168.1.153`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Packages surfaced as shortcuts in the dashboard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favourite_apps: Vec<AndroidApp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidApp {
    pub package: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidTvStatus {
    pub configured: bool,
    pub reachable: bool,
    /// `false` while the box is asleep (screen off), which is its idle state.
    pub awake: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// True once a Remote v2 session is available, which is what makes keys
    /// fast. Unpaired is a normal state, not a fault: ADB still works.
    pub paired: bool,
}

#[derive(Clone)]
pub struct AndroidTvManager {
    config_path: PathBuf,
    key_path: PathBuf,
    identity_path: PathBuf,
    config: Arc<RwLock<AndroidTvConfig>>,
    key: Arc<Mutex<Option<RsaPrivateKey>>>,
    /// Reused across calls; also serializes them, which suits a remote
    /// control and keeps a single ADB stream open at a time.
    device: Arc<Mutex<Option<AdbDevice>>>,
    /// Remote v2 session, preferred for keys and app launches: ~20 ms against
    /// ADB's ~150 ms, nearly all of which is `input` booting a JVM per press.
    remote: Arc<Mutex<Option<Session>>>,
    /// A pairing in flight. It spans two HTTP requests — the TV shows a code
    /// between them — so the open TLS session has to be held here.
    pending_pairing: Arc<Mutex<Option<Pairing>>>,
}

impl AndroidTvManager {
    pub fn new(
        config_path: &Path,
        key_path: &Path,
        identity_path: &Path,
    ) -> Result<Self, AppError> {
        let config = match std::fs::read_to_string(config_path) {
            Ok(content) if !content.trim().is_empty() => {
                serde_json::from_str(&content).map_err(|error| {
                    AppError::http(
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "invalid Android TV config at {}: {error}",
                            config_path.display()
                        ),
                    )
                })?
            }
            _ => AndroidTvConfig::default(),
        };

        Ok(Self {
            config_path: config_path.to_path_buf(),
            key_path: key_path.to_path_buf(),
            identity_path: identity_path.to_path_buf(),
            config: Arc::new(RwLock::new(config)),
            key: Arc::new(Mutex::new(None)),
            device: Arc::new(Mutex::new(None)),
            remote: Arc::new(Mutex::new(None)),
            pending_pairing: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn config(&self) -> AndroidTvConfig {
        self.config.read().await.clone()
    }

    pub async fn set_config(
        &self,
        config: AndroidTvConfig,
    ) -> Result<AndroidTvConfig, AppError> {
        for app in &config.favourite_apps {
            validate_package(&app.package)?;
        }
        std::fs::write(&self.config_path, serde_json::to_string_pretty(&config)?)?;
        *self.config.write().await = config.clone();
        // The address may have changed; drop any connection to the old one.
        *self.device.lock().await = None;
        Ok(config)
    }

    async fn address(&self) -> Result<String, AppError> {
        let config = self.config.read().await;
        let host = config
            .host
            .clone()
            .ok_or_else(|| AppError::service_unavailable("No Android TV box configured"))?;
        Ok(format!("{host}:{}", config.port.unwrap_or(DEFAULT_ADB_PORT)))
    }

    /// Loads the signing key, generating and persisting one on first use.
    /// Generation is CPU-bound and slow on the Pi, hence `spawn_blocking`.
    async fn signing_key(&self) -> Result<RsaPrivateKey, AppError> {
        let mut slot = self.key.lock().await;
        if let Some(key) = slot.as_ref() {
            return Ok(key.clone());
        }

        let key = match std::fs::read(&self.key_path) {
            Ok(der) if !der.is_empty() => adb::decode_key(&der)?,
            _ => {
                tracing::info!("generating an ADB key (slow on the Pi, done once)");
                let generated =
                    tokio::task::spawn_blocking(adb::generate_key).await??;
                std::fs::write(&self.key_path, adb::encode_key(&generated)?)?;
                // The key authenticates this host to the box; keep it private.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &self.key_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                generated
            }
        };

        *slot = Some(key.clone());
        Ok(key)
    }

    /// Runs a shell command, reconnecting once if the kept connection died.
    async fn shell(&self, command: &str) -> Result<String, AppError> {
        let address = self.address().await?;
        let key = self.signing_key().await?;
        let mut slot = self.device.lock().await;

        if let Some(device) = slot.as_mut() {
            match device.shell(command).await {
                Ok(output) => return Ok(output),
                // A stale connection is the common case (the box slept, or
                // adbd restarted); fall through and dial again.
                Err(error) => {
                    tracing::debug!(%error, "ADB connection went stale, reconnecting");
                    *slot = None;
                }
            }
        }

        let mut device = AdbDevice::connect(&address, &key).await?;
        let output = device.shell(command).await?;
        *slot = Some(device);
        Ok(output)
    }

    /// Sends a key, over Remote v2 when the box is paired.
    ///
    /// The fallback matters: Remote v2 is unavailable until someone has paired
    /// the host, and ADB keeps working meanwhile — a slower remote beats no
    /// remote.
    pub async fn send_key(&self, key: AndroidKey) -> Result<(), AppError> {
        if let Some(session) = self.remote_session().await {
            match session.key(key.android_keycode()).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::debug!(%error, "remote session failed, falling back to ADB");
                    *self.remote.lock().await = None;
                }
            }
        }
        self.shell(&format!("input keyevent {}", key.keycode()))
            .await
            .map(|_| ())
    }

    /// Sends a key over ADB, bypassing the Remote v2 preference. Exists so
    /// the live benchmark can compare the two channels directly.
    pub async fn adb_key_for_benchmark(&self, key: AndroidKey) -> Result<(), AppError> {
        self.shell(&format!("input keyevent {}", key.keycode()))
            .await
            .map(|_| ())
    }

    /// The live Remote v2 session, reconnecting if it dropped. Returns `None`
    /// when the host is not paired, which is not an error — it is the state
    /// every host starts in.
    async fn remote_session(&self) -> Option<Session> {
        let mut slot = self.remote.lock().await;
        if let Some(session) = slot.as_ref() {
            if session.is_open() {
                return Some(session.clone());
            }
        }

        let host = self.config.read().await.host.clone()?;
        let identity = self.identity().await.ok()?;
        match Session::connect(&host, &identity).await {
            Ok(session) => {
                *slot = Some(session.clone());
                Some(session)
            }
            Err(error) => {
                tracing::debug!(%error, "no Remote v2 session (not paired?)");
                None
            }
        }
    }

    async fn identity(&self) -> Result<Identity, AppError> {
        let path = self.identity_path.clone();
        tokio::task::spawn_blocking(move || Identity::load_or_create(&path)).await?
    }

    /// Opens a pairing session; the TV shows a six hex-digit code afterwards.
    pub async fn start_pairing(&self) -> Result<(), AppError> {
        let host = self
            .config
            .read()
            .await
            .host
            .clone()
            .ok_or_else(|| AppError::service_unavailable("No Android TV box configured"))?;
        let identity = self.identity().await?;
        let pairing = Pairing::start(&host, &identity).await?;
        *self.pending_pairing.lock().await = Some(pairing);
        Ok(())
    }

    /// Completes pairing with the code from the screen.
    pub async fn finish_pairing(&self, code: &str) -> Result<(), AppError> {
        let pairing = self.pending_pairing.lock().await.take().ok_or_else(|| {
            AppError::http(
                axum::http::StatusCode::CONFLICT,
                "no pairing in progress — start one first",
            )
        })?;
        pairing.finish(code).await?;
        // Force the next key onto the freshly paired session.
        *self.remote.lock().await = None;
        Ok(())
    }

    pub async fn is_paired(&self) -> bool {
        self.remote_session().await.is_some()
    }

    pub async fn status(&self) -> AndroidTvStatus {
        let configured = self.config.read().await.host.is_some();
        if !configured {
            return AndroidTvStatus {
                configured: false,
                reachable: false,
                awake: false,
                current_app: None,
                model: None,
                paired: false,
            };
        }

        // One shell round-trip for everything: the box is on the other end of
        // a Pi 1's network stack, and each call costs a stream.
        let probe = self
            .shell("getprop ro.product.model; dumpsys power | grep -m1 mWakefulness=; dumpsys activity activities | grep -m1 ResumedActivity")
            .await;

        match probe {
            Ok(output) => {
                let model = output.lines().next().map(|line| line.trim().to_string());
                let awake = output.contains("mWakefulness=Awake");
                AndroidTvStatus {
                    configured: true,
                    reachable: true,
                    awake,
                    current_app: parse_resumed_package(&output),
                    model: model.filter(|value| !value.is_empty()),
                    paired: self.remote.lock().await.is_some(),
                }
            }
            Err(_) => AndroidTvStatus {
                configured: true,
                reachable: false,
                awake: false,
                current_app: None,
                model: None,
                paired: false,
            },
        }
    }

    pub async fn launch_app(&self, package: &str) -> Result<(), AppError> {
        validate_package(package)?;
        if let Some(session) = self.remote_session().await {
            match session.launch(package).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::debug!(%error, "remote launch failed, falling back to ADB");
                    *self.remote.lock().await = None;
                }
            }
        }
        self.shell(&format!(
            "monkey -p {package} -c android.intent.category.LAUNCHER 1"
        ))
        .await
        .map(|_| ())
    }

    /// Installed launchable packages, for the app picker.
    pub async fn apps(&self) -> Result<Vec<String>, AppError> {
        let output = self.shell("pm list packages -3").await?;
        let mut packages: Vec<String> = output
            .lines()
            .filter_map(|line| line.trim().strip_prefix("package:"))
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        packages.sort();
        Ok(packages)
    }

    /// Wakes the box, which asserts CEC One Touch Play — that powers the
    /// television on and routes it to this box's HDMI input.
    pub async fn wake(&self) -> Result<(), AppError> {
        self.send_key(AndroidKey::Wakeup).await?;
        self.one_touch_play().await
    }

    /// Sleeping the box broadcasts a CEC standby, which also turns the TV off
    /// (the box runs with `power_control_mode=broadcast`).
    pub async fn sleep(&self) -> Result<(), AppError> {
        self.send_key(AndroidKey::Sleep).await
    }

    /// Installs an APK: push it to the box's temp directory, hand it to the
    /// package manager, then clean up. `-r` reinstalls over an existing copy,
    /// which is what makes iterating on your own app painless.
    ///
    /// The payload is held in memory, so callers must bound it — the Pi 1 has
    /// 512 MB and no swap worth the name.
    pub async fn install_apk(&self, apk: &[u8]) -> Result<String, AppError> {
        const REMOTE_PATH: &str = "/data/local/tmp/maison-install.apk";

        if apk.is_empty() {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "empty APK",
            ));
        }
        // ZIP local file header: every APK is a zip, so this catches a wrong
        // upload before it costs a slow transfer.
        if !apk.starts_with(b"PK\x03\x04") {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "that file is not an APK (missing zip signature)",
            ));
        }

        let address = self.address().await?;
        let key = self.signing_key().await?;
        let mut slot = self.device.lock().await;

        // The sync service runs on its own connection: keep the shared one
        // clean by dialling a dedicated device for the transfer.
        let mut transfer = AdbDevice::connect(&address, &key).await?;
        transfer.push(REMOTE_PATH, apk, 0o644).await?;

        let device = match slot.as_mut() {
            Some(device) => device,
            None => {
                *slot = Some(AdbDevice::connect(&address, &key).await?);
                slot.as_mut().expect("just connected")
            }
        };

        let output = device
            .shell(&format!("pm install -r {REMOTE_PATH}"))
            .await?;
        let _ = device.shell(&format!("rm -f {REMOTE_PATH}")).await;

        if output.contains("Success") {
            Ok(output.trim().to_string())
        } else {
            Err(AppError::service_unavailable(format!(
                "the box refused the install: {}",
                output.trim()
            )))
        }
    }

    pub async fn one_touch_play(&self) -> Result<(), AppError> {
        self.shell("cmd hdmi_control onetouchplay").await.map(|_| ())
    }
}

/// Package names reach the shell, so they are checked against the Android
/// naming rules rather than trusted.
fn validate_package(package: &str) -> Result<(), AppError> {
    let valid = !package.is_empty()
        && package.len() <= 255
        && package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("invalid package name {package:?}"),
        ))
    }
}

/// Pulls the package out of a `ResumedActivity` dump line, which looks like
/// `ResumedActivity: ActivityRecord{… u0 org.smarttube.beta/….PlaybackActivity t32}`.
fn parse_resumed_package(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find(|line| line.contains("ResumedActivity"))?;
    let activity = line.split_whitespace().find(|token| token.contains('/'))?;
    let package = activity.split('/').next()?;
    (!package.is_empty()).then(|| package.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two channels spell the same key differently — a name for ADB, a
    /// number for Remote v2 — so they are asserted against each other. A
    /// mismatch would make a button behave differently depending on whether
    /// the box happens to be paired, which is the worst kind of bug to chase.
    #[test]
    fn keycode_spellings_agree() {
        let pairs = [
            (AndroidKey::Home, "KEYCODE_HOME", 3),
            (AndroidKey::Back, "KEYCODE_BACK", 4),
            (AndroidKey::Up, "KEYCODE_DPAD_UP", 19),
            (AndroidKey::Ok, "KEYCODE_DPAD_CENTER", 23),
            (AndroidKey::VolumeUp, "KEYCODE_VOLUME_UP", 24),
            (AndroidKey::PlayPause, "KEYCODE_MEDIA_PLAY_PAUSE", 85),
            (AndroidKey::Mute, "KEYCODE_VOLUME_MUTE", 164),
            (AndroidKey::Wakeup, "KEYCODE_WAKEUP", 224),
        ];
        for (key, name, code) in pairs {
            assert_eq!(key.keycode(), name, "{key:?} name");
            assert_eq!(key.android_keycode(), code, "{key:?} number");
        }
    }

    #[test]
    fn keycodes_match_android_names() {
        assert_eq!(AndroidKey::Ok.keycode(), "KEYCODE_DPAD_CENTER");
        assert_eq!(AndroidKey::PlayPause.keycode(), "KEYCODE_MEDIA_PLAY_PAUSE");
        assert_eq!(AndroidKey::Mute.keycode(), "KEYCODE_VOLUME_MUTE");
    }

    /// Package names are interpolated into a shell command, so anything that
    /// could break out of the argument must be refused.
    #[test]
    fn package_validation_rejects_shell_metacharacters() {
        assert!(validate_package("org.smarttube.beta").is_ok());
        assert!(validate_package("com.google.android.youtube.tv").is_ok());
        for bad in [
            "",
            "a; rm -rf /",
            "a b",
            "a$(id)",
            "a`id`",
            "a&&b",
            "a|b",
            "a>f",
            "a'b",
        ] {
            assert!(validate_package(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn parses_the_resumed_package() {
        let dump = "LEAP-S1\n  mWakefulness=Awake\n  ResumedActivity: ActivityRecord{6c4101d u0 org.smarttube.beta/com.liskovsoft.smartyoutubetv2.tv.ui.playback.PlaybackActivity t32}";
        assert_eq!(
            parse_resumed_package(dump).as_deref(),
            Some("org.smarttube.beta")
        );
    }

    #[test]
    fn resumed_package_is_absent_when_the_dump_has_none() {
        assert_eq!(parse_resumed_package("LEAP-S1\nmWakefulness=Asleep"), None);
    }

    #[tokio::test]
    async fn missing_config_is_an_empty_config_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = AndroidTvManager::new(
            &dir.path().join("androidtv.json"),
            &dir.path().join("adb-key"),
            &dir.path().join("atv-identity"),
        )
        .expect("manager");
        assert!(manager.config().await.host.is_none());
        let status = manager.status().await;
        assert!(!status.configured);
        assert!(!status.reachable);
    }

    #[tokio::test]
    async fn set_config_persists_and_rejects_bad_packages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("androidtv.json");
        let manager = AndroidTvManager::new(&path, &dir.path().join("adb-key"), &dir.path().join("atv-identity"))
            .expect("manager");

        manager
            .set_config(AndroidTvConfig {
                host: Some("192.168.1.153".to_string()),
                port: None,
                favourite_apps: vec![AndroidApp {
                    package: "org.smarttube.beta".to_string(),
                    label: "SmartTube".to_string(),
                }],
            })
            .await
            .expect("saved");

        let reloaded = AndroidTvManager::new(
            &path,
            &dir.path().join("adb-key"),
            &dir.path().join("atv-identity"),
        )
        .expect("reload")
            .config()
            .await;
        assert_eq!(reloaded.host.as_deref(), Some("192.168.1.153"));
        assert_eq!(reloaded.favourite_apps.len(), 1);

        let rejected = manager
            .set_config(AndroidTvConfig {
                host: Some("192.168.1.153".to_string()),
                port: None,
                favourite_apps: vec![AndroidApp {
                    package: "evil; reboot".to_string(),
                    label: "nope".to_string(),
                }],
            })
            .await;
        assert!(rejected.is_err());
    }
}

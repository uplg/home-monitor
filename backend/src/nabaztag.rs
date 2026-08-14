//! Garenne (Nabaztag) client.
//!
//! The rabbit runs the garenne firmware from the clapier project: it fetches
//! its content over HTTP from the clapier server and accepts commands over
//! UDP port 9998 with a `grn1 ` magic prefix. This module drives that control
//! port directly (no dependency on clapier) and mirrors the daily Tempo
//! colors on the rabbit: today's color as the breathing LED, tomorrow's as
//! the ear position.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{net::UdpSocket, sync::RwLock};
use tracing::{debug, info};

use crate::error::AppError;

/// Garenne control port on the rabbit.
const CTL_PORT: u16 = 9998;
/// Magic prefix of every control datagram.
const CTL_MAGIC: &str = "grn1 ";
/// Rabbit HTTP status endpoint timeout.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// Pause between the LED and ear commands of a Tempo push, so the firmware
/// processes them as distinct events.
const PUSH_STEP_DELAY: Duration = Duration::from_millis(300);
/// The belly (middle body) LED, where the Tempo color is displayed.
const BELLY_LED: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NabaztagConfig {
    /// Rabbit IP or hostname on the LAN.
    pub host: Option<String>,
    /// Whether the daily Tempo colors are mirrored on the rabbit.
    #[serde(default = "default_true")]
    pub tempo_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NabaztagConfig {
    fn default() -> Self {
        Self {
            host: None,
            tempo_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoPushResult {
    pub today_color: String,
    pub tomorrow_color: Option<String>,
    pub led_hex: String,
    pub ear_position: Option<u8>,
}

#[derive(Clone)]
pub struct NabaztagManager {
    inner: Arc<Inner>,
}

struct Inner {
    config_path: PathBuf,
    config: RwLock<NabaztagConfig>,
    http: reqwest::Client,
}

impl NabaztagManager {
    pub fn new(config_path: &Path, env_host: Option<&str>) -> Result<Self, AppError> {
        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(config_path)?;
            serde_json::from_str::<NabaztagConfig>(content.trim()).unwrap_or_default()
        } else {
            NabaztagConfig::default()
        };

        // The environment wins over the persisted file, like the rest of the
        // deployment configuration.
        if let Some(host) = env_host {
            if !host.trim().is_empty() {
                config.host = Some(host.trim().to_string());
            }
        }

        let http = reqwest::Client::builder()
            .timeout(STATUS_TIMEOUT)
            .build()
            .map_err(|error| AppError::service_unavailable(error.to_string()))?;

        Ok(Self {
            inner: Arc::new(Inner {
                config_path: config_path.to_path_buf(),
                config: RwLock::new(config),
                http,
            }),
        })
    }

    pub async fn config(&self) -> NabaztagConfig {
        self.inner.config.read().await.clone()
    }

    pub async fn set_config(&self, config: NabaztagConfig) -> Result<NabaztagConfig, AppError> {
        let mut current = self.inner.config.write().await;
        *current = config.clone();
        let payload = serde_json::to_string_pretty(&*current)?;
        if let Some(parent) = self.inner.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.inner.config_path, format!("{payload}\n"))?;
        Ok(config)
    }

    async fn host(&self) -> Result<String, AppError> {
        self.inner
            .config
            .read()
            .await
            .host
            .clone()
            .ok_or_else(|| {
                AppError::service_unavailable(
                    "No Nabaztag host configured (set NABAZTAG_HOST or the nabaztag config)",
                )
            })
    }

    /// Sends one garenne control command to the rabbit. Fire-and-forget by
    /// protocol design; delivery is not acknowledged.
    pub async fn send_command(&self, command: &str) -> Result<(), AppError> {
        let command = command.trim();
        if !command_is_allowed(command) {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                format!("Command not allowed: {command}"),
            ));
        }

        let host = self.host().await?;
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let payload = format!("{CTL_MAGIC}{command}");
        socket
            .send_to(payload.as_bytes(), (host.as_str(), CTL_PORT))
            .await?;
        debug!(command, host = %host, "garenne command sent");
        Ok(())
    }

    /// Fetches the rabbit's `/status` page (its only HTTP route).
    pub async fn status(&self) -> Result<String, AppError> {
        let host = self.host().await?;
        let response = self
            .inner
            .http
            .get(format!("http://{host}/status"))
            .send()
            .await
            .map_err(|error| AppError::service_unavailable(format!("Rabbit unreachable: {error}")))?;
        response
            .text()
            .await
            .map_err(|error| AppError::service_unavailable(error.to_string()))
    }

    /// Mirrors the Tempo colors: today as a static color on the belly LED,
    /// tomorrow as the ear position (both ears; up=blue, half=white,
    /// down=red).
    pub async fn push_tempo(
        &self,
        today: &str,
        tomorrow: Option<&str>,
    ) -> Result<TempoPushResult, AppError> {
        let led_hex = tempo_color_hex(today).ok_or_else(|| {
            AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                format!("Unknown Tempo color: {today}"),
            )
        })?;

        self.send_command(&format!("led {BELLY_LED} {led_hex}")).await?;

        let ear_position = tomorrow.and_then(tempo_ear_position);
        if let Some(position) = ear_position {
            tokio::time::sleep(PUSH_STEP_DELAY).await;
            self.send_command(&format!("ears {position} {position}"))
                .await?;
        }

        info!(
            today,
            tomorrow = tomorrow.unwrap_or("unknown"),
            led = led_hex,
            ears = ?ear_position,
            "tempo pushed to the rabbit"
        );

        Ok(TempoPushResult {
            today_color: today.to_string(),
            tomorrow_color: tomorrow.map(str::to_string),
            led_hex: led_hex.to_string(),
            ear_position,
        })
    }
}

/// Allow-list mirroring clapier's `vet_ctl`: safe garenne commands only.
/// `conf` is deliberately excluded — it can rewrite the rabbit's flash
/// configuration (including the server address).
fn command_is_allowed(command: &str) -> bool {
    if command.is_empty()
        || command.len() > 120
        || !command
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " ./_-".contains(c))
    {
        return false;
    }

    let verb = command.split_whitespace().next().unwrap_or_default();
    matches!(
        verb,
        "color" | "led" | "ears" | "vol" | "ping" | "dance" | "stop" | "chor" | "play" | "reboot"
    )
}

pub fn tempo_color_hex(color: &str) -> Option<&'static str> {
    match color.to_ascii_uppercase().as_str() {
        "BLUE" | "BLEU" => Some("0000ff"),
        "WHITE" | "BLANC" => Some("ffffff"),
        "RED" | "ROUGE" => Some("ff0000"),
        _ => None,
    }
}

fn tempo_ear_position(color: &str) -> Option<u8> {
    match color.to_ascii_uppercase().as_str() {
        "BLUE" | "BLEU" => Some(0),
        "WHITE" | "BLANC" => Some(8),
        "RED" | "ROUGE" => Some(16),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_allow_list_accepts_garenne_verbs_only() {
        assert!(command_is_allowed("color 0000ff"));
        assert!(command_is_allowed("ears 8 8"));
        assert!(command_is_allowed("chor /vl/chor/pilot.chor"));
        assert!(!command_is_allowed("conf 10.0.0.1/vl"));
        assert!(!command_is_allowed("color; rm -rf /"));
        assert!(!command_is_allowed(""));
    }

    #[test]
    fn tempo_colors_map_in_both_languages() {
        assert_eq!(tempo_color_hex("BLUE"), Some("0000ff"));
        assert_eq!(tempo_color_hex("bleu"), Some("0000ff"));
        assert_eq!(tempo_color_hex("ROUGE"), Some("ff0000"));
        assert_eq!(tempo_color_hex("mauve"), None);
        assert_eq!(tempo_ear_position("WHITE"), Some(8));
    }
}

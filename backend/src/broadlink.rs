use std::{
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use rbroadlink::{Device, network::WirelessConnection, traits::DeviceTrait};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{error::AppError, mitsubishi_ir};

const DEFAULT_LEARN_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct BroadlinkManager {
    codes_path: Arc<PathBuf>,
    codes: Arc<RwLock<StoredCodes>>,
    climate_state_path: Arc<PathBuf>,
    climate_state: Arc<RwLock<Option<StoredClimateState>>>,
    discovered_devices: Arc<RwLock<Option<Vec<BroadlinkDiscoveredDevice>>>>,
    operation_lock: Arc<Mutex<()>>,
}

/// Last Mitsubishi climate state commanded through this backend. IR is
/// one-way, so this is the best available approximation of the unit's state;
/// it is persisted so the UI can restore the last settings across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredClimateState {
    /// Whether the last sent command left the unit on.
    pub power: bool,
    /// The most recent command sent (possibly `state-off`).
    pub last_command: String,
    /// The most recent non-off structured command, used to restore the form.
    pub last_on_command: Option<String>,
    /// The parsed settings of `last_on_command`, so clients never have to
    /// re-parse the command grammar.
    #[serde(default)]
    pub settings: Option<mitsubishi_ir::ClimateSettings>,
    pub host: String,
    pub model: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadlinkDiscoveredDevice {
    pub host: String,
    pub mac: String,
    pub model_code: u16,
    pub friendly_model: String,
    pub friendly_type: String,
    pub name: String,
    pub is_locked: bool,
    pub kind: String,
    pub supports_learning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadlinkCodeEntry {
    pub id: String,
    pub name: String,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub command: String,
    pub packet_base64: String,
    pub packet_length: usize,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnResult {
    pub packet_base64: String,
    pub packet_length: usize,
    pub code: Option<BroadlinkCodeEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub host: String,
    pub code_id: Option<String>,
    pub command: Option<String>,
    pub packet_length: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCodeRequest {
    pub name: String,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub command: String,
    pub packet_base64: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnCodeSaveRequest {
    pub name: String,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub command: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadlinkSecurityMode {
    None,
    Wep,
    Wpa,
    Wpa1,
    Wpa2,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StoredCodes {
    codes: Vec<BroadlinkCodeEntry>,
}

impl BroadlinkManager {
    pub fn new(codes_path: &Path, climate_state_path: &Path) -> Result<Self, AppError> {
        let codes = if codes_path.exists() {
            let content = std::fs::read_to_string(codes_path)?;
            load_stored_codes(&content)?
        } else {
            StoredCodes::default()
        };

        // A corrupt or unreadable climate-state file must never prevent the
        // backend from starting; the state is only a UI convenience.
        let mut climate_state = std::fs::read_to_string(climate_state_path)
            .ok()
            .and_then(|content| serde_json::from_str::<StoredClimateState>(content.trim()).ok());

        // Backfill parsed settings for files written before the field existed.
        if let Some(state) = climate_state.as_mut() {
            if state.settings.is_none() {
                state.settings = state
                    .last_on_command
                    .as_deref()
                    .and_then(mitsubishi_ir::parse_climate_settings);
            }
        }

        Ok(Self {
            codes_path: Arc::new(codes_path.to_path_buf()),
            codes: Arc::new(RwLock::new(codes)),
            climate_state_path: Arc::new(climate_state_path.to_path_buf()),
            climate_state: Arc::new(RwLock::new(climate_state)),
            discovered_devices: Arc::new(RwLock::new(None)),
            operation_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn climate_state(&self) -> Option<StoredClimateState> {
        self.climate_state.read().await.clone()
    }

    pub async fn discover(
        &self,
        local_ip: Option<String>,
        force_refresh: bool,
    ) -> Result<Vec<BroadlinkDiscoveredDevice>, AppError> {
        if !force_refresh {
            if let Some(cached) = self.discovered_devices.read().await.clone() {
                return Ok(cached);
            }
        }

        let _guard = self.operation_lock.lock().await;
        if !force_refresh {
            if let Some(cached) = self.discovered_devices.read().await.clone() {
                return Ok(cached);
            }
        }

        let local_ip = parse_optional_ipv4(local_ip.as_deref())?;
        let discover_result = tokio::task::spawn_blocking(move || {
            Device::list(local_ip)
                .map_err(AppError::service_unavailable)
                .map(|devices| devices.into_iter().map(map_discovered_device).collect::<Vec<_>>())
        })
        .await?;

        let devices = match discover_result {
            Ok(devices) => devices,
            Err(error) if is_address_in_use_discovery_error(&error) => {
                if let Some(cached) = self.discovered_devices.read().await.clone() {
                    return Ok(cached);
                }
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };

        *self.discovered_devices.write().await = Some(devices.clone());

        Ok(devices)
    }

    pub async fn provision(
        &self,
        ssid: String,
        password: Option<String>,
        security_mode: BroadlinkSecurityMode,
    ) -> Result<(), AppError> {
        let _guard = self.operation_lock.lock().await;
        let task = tokio::task::spawn_blocking(move || {
            let password = password.unwrap_or_default();
            let network = match security_mode {
                BroadlinkSecurityMode::None => WirelessConnection::None(&ssid),
                BroadlinkSecurityMode::Wep => WirelessConnection::WEP(&ssid, &password),
                BroadlinkSecurityMode::Wpa => WirelessConnection::WPA(&ssid, &password),
                BroadlinkSecurityMode::Wpa1 => WirelessConnection::WPA1(&ssid, &password),
                BroadlinkSecurityMode::Wpa2 => WirelessConnection::WPA2(&ssid, &password),
            };

            Device::connect_to_network(&network)
                .map(|_| ())
                .map_err(AppError::service_unavailable)
        });

        task.await??;
        Ok(())
    }

    pub async fn learn_ir(
        &self,
        host: String,
        local_ip: Option<String>,
        timeout_secs: Option<u64>,
        save_request: Option<LearnCodeSaveRequest>,
    ) -> Result<LearnResult, AppError> {
        let _guard = self.operation_lock.lock().await;
        let local_ip = parse_optional_ipv4(local_ip.as_deref())?;
        let host_ip = parse_ipv4(&host)?;
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_LEARN_TIMEOUT_SECS));
        let join = tokio::task::spawn_blocking(move || learn_ir_blocking(host_ip, local_ip));
        let packet = tokio::time::timeout(timeout, join)
            .await
            .map_err(|_| AppError::service_unavailable("Timed out while waiting for an IR code"))???;

        let packet_base64 = STANDARD.encode(&packet);
        let code = if let Some(save_request) = save_request {
            Some(
                self.save_code(SaveCodeRequest {
                    name: save_request.name,
                    brand: save_request.brand,
                    model: save_request.model,
                    command: save_request.command,
                    packet_base64: packet_base64.clone(),
                    tags: save_request.tags,
                })
                .await?,
            )
        } else {
            None
        };

        Ok(LearnResult {
            packet_length: packet.len(),
            packet_base64,
            code,
        })
    }

    pub async fn send_packet(
        &self,
        host: String,
        local_ip: Option<String>,
        packet_base64: String,
        code_id: Option<String>,
        command: Option<String>,
    ) -> Result<SendResult, AppError> {
        let _guard = self.operation_lock.lock().await;
        let packet = decode_packet(&packet_base64)?;
        let packet_length = packet.len();
        let local_ip = parse_optional_ipv4(local_ip.as_deref())?;
        let host_ip = parse_ipv4(&host)?;

        tokio::task::spawn_blocking(move || send_packet_blocking(host_ip, local_ip, packet))
            .await??;

        Ok(SendResult {
            host,
            code_id,
            command,
            packet_length,
        })
    }

    pub async fn list_codes(&self) -> Vec<BroadlinkCodeEntry> {
        let mut codes = self.codes.read().await.codes.clone();
        codes.sort_by(|left, right| left.name.cmp(&right.name).then(left.command.cmp(&right.command)));
        codes
    }

    pub async fn list_mitsubishi_codes(&self, model: Option<&str>) -> Vec<BroadlinkCodeEntry> {
        let requested_model = model.map(normalize_lookup_value);
        let mut codes = self
            .codes
            .read()
            .await
            .codes
            .iter()
            .filter(|entry| is_mitsubishi_entry(entry))
            .filter(|entry| {
                requested_model.as_deref().map_or(true, |model| {
                    entry.model.as_deref().map(normalize_lookup_value).as_deref() == Some(model)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        codes.sort_by(|left, right| left.command.cmp(&right.command).then(left.name.cmp(&right.name)));
        codes
    }

    pub async fn save_code(&self, request: SaveCodeRequest) -> Result<BroadlinkCodeEntry, AppError> {
        let packet = decode_packet(&request.packet_base64)?;
        let now = Utc::now();
        let normalized_command = normalize_lookup_value(&request.command);
        let normalized_brand = request.brand.as_deref().map(normalize_lookup_value);
        let normalized_model = request.model.as_deref().map(normalize_lookup_value);
        let normalized_name = request.name.trim();

        if normalized_name.is_empty() {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "name is required",
            ));
        }

        if normalized_command.is_empty() {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "command is required",
            ));
        }

        let entry = {
            let mut codes = self.codes.write().await;
            if let Some(existing) = codes.codes.iter_mut().find(|entry| {
                entry.command == normalized_command
                    && entry.brand.as_deref() == normalized_brand.as_deref()
                    && entry.model.as_deref() == normalized_model.as_deref()
                    && entry.name == normalized_name
            }) {
                existing.packet_base64 = request.packet_base64.clone();
                existing.packet_length = packet.len();
                existing.tags = normalize_tags(request.tags);
                existing.updated_at = now;
                existing.clone()
            } else {
                let entry = BroadlinkCodeEntry {
                    id: Uuid::new_v4().to_string(),
                    name: normalized_name.to_string(),
                    brand: normalized_brand.filter(|value| !value.is_empty()),
                    model: normalized_model.filter(|value| !value.is_empty()),
                    command: normalized_command,
                    packet_base64: request.packet_base64,
                    packet_length: packet.len(),
                    tags: normalize_tags(request.tags),
                    created_at: now,
                    updated_at: now,
                };
                codes.codes.push(entry.clone());
                entry
            }
        };

        self.persist_codes().await?;
        Ok(entry)
    }

    pub async fn send_saved_code(
        &self,
        host: String,
        local_ip: Option<String>,
        code_id: String,
    ) -> Result<SendResult, AppError> {
        let code = self
            .codes
            .read()
            .await
            .codes
            .iter()
            .find(|entry| entry.id == code_id)
            .cloned()
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Broadlink code not found"))?;

        self.send_packet(
            host,
            local_ip,
            code.packet_base64,
            Some(code.id),
            Some(code.command),
        )
        .await
    }

    pub async fn send_mitsubishi_command(
        &self,
        host: String,
        local_ip: Option<String>,
        command: String,
        model: Option<String>,
    ) -> Result<SendResult, AppError> {
        let normalized_command = normalize_lookup_value(&command);
        let clock_ticks = mitsubishi_ir::current_clock_ticks();
        if let Some(packet) =
            mitsubishi_ir::encode_mitsubishi_command(&normalized_command, clock_ticks)
                .map_err(|error| AppError::http(axum::http::StatusCode::BAD_REQUEST, error))?
        {
            let packet_base64 = STANDARD.encode(&packet);
            let result = self
                .send_packet(host, local_ip, packet_base64, None, Some(normalized_command.clone()))
                .await?;
            self.record_climate_state(&normalized_command, &result.host, model.as_deref())
                .await;
            return Ok(result);
        }

        let normalized_model = model.as_deref().map(normalize_lookup_value);

        let codes = self.codes.read().await;
        let code = codes
            .codes
            .iter()
            .find(|entry| {
                is_mitsubishi_entry(entry)
                    && entry.command == normalized_command
                    && normalized_model.as_deref().is_some_and(|model| {
                        entry.model.as_deref().map(normalize_lookup_value).as_deref() == Some(model)
                    })
            })
            .cloned()
            .or_else(|| {
                codes
                    .codes
                    .iter()
                    .find(|entry| is_mitsubishi_entry(entry) && entry.command == normalized_command)
                    .cloned()
            })
            .ok_or_else(|| {
                AppError::http(
                    axum::http::StatusCode::NOT_FOUND,
                    format!("No saved Mitsubishi code found for command '{normalized_command}'"),
                )
            })?;

        drop(codes);

        let result = self
            .send_packet(
                host,
                local_ip,
                code.packet_base64,
                Some(code.id),
                Some(code.command.clone()),
            )
            .await?;
        self.record_raw_climate_command(&code.command, &result.host, normalized_model.as_deref())
            .await;
        Ok(result)
    }

    /// Remembers the last commanded climate state and persists it. Failures
    /// are logged but never propagated: the IR command itself already went
    /// out, and the stored state is only used to restore the UI.
    async fn record_climate_state(&self, command: &str, host: &str, model: Option<&str>) {
        // The file write happens under the lock so concurrent sends cannot
        // persist out of order (rare, and the write is a few kilobytes).
        let mut state = self.climate_state.write().await;
        let power = command != "state-off";
        let (last_on_command, settings) = if power {
            (
                Some(command.to_string()),
                mitsubishi_ir::parse_climate_settings(command),
            )
        } else {
            state
                .as_ref()
                .map(|previous| (previous.last_on_command.clone(), previous.settings.clone()))
                .unwrap_or((None, None))
        };
        *state = Some(StoredClimateState {
            power,
            last_command: command.to_string(),
            last_on_command,
            settings,
            host: host.to_string(),
            model: model.map(str::to_string),
            updated_at: Utc::now(),
        });
        persist_climate_state(self.climate_state_path.as_path(), state.as_ref());
    }

    /// Records that a raw (learned, non-structured) command was sent. Its
    /// effect on the unit is unknown, so power/last-on state are left as-is;
    /// only the audit fields are refreshed.
    async fn record_raw_climate_command(&self, command: &str, host: &str, model: Option<&str>) {
        let mut state = self.climate_state.write().await;
        let Some(current) = state.as_mut() else {
            return;
        };
        current.last_command = command.to_string();
        current.host = host.to_string();
        if model.is_some() {
            current.model = model.map(str::to_string);
        }
        current.updated_at = Utc::now();
        persist_climate_state(self.climate_state_path.as_path(), state.as_ref());
    }

    async fn persist_codes(&self) -> Result<(), AppError> {
        let payload = {
            let codes = self.codes.read().await;
            serde_json::to_string_pretty(&*codes)?
        };
        write_string_to_path(self.codes_path.as_path(), &format!("{payload}\n"))
    }
}

fn persist_climate_state(path: &Path, state: Option<&StoredClimateState>) {
    let Some(state) = state else { return };
    match serde_json::to_string_pretty(state) {
        Ok(payload) => {
            if let Err(error) = write_string_to_path(path, &format!("{payload}\n")) {
                tracing::warn!(?error, "failed to persist climate state");
            }
        }
        Err(error) => tracing::warn!(?error, "failed to serialize climate state"),
    }
}

fn load_stored_codes(content: &str) -> Result<StoredCodes, AppError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(StoredCodes::default());
    }

    if let Ok(stored) = serde_json::from_str::<StoredCodes>(trimmed) {
        return Ok(stored);
    }

    if let Ok(codes) = serde_json::from_str::<Vec<BroadlinkCodeEntry>>(trimmed) {
        return Ok(StoredCodes { codes });
    }

    Err(serde_json::from_str::<StoredCodes>(trimmed).unwrap_err().into())
}

fn parse_ipv4(value: &str) -> Result<Ipv4Addr, AppError> {
    let addr = value.parse::<Ipv4Addr>().map_err(|_| {
        AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid IPv4 address: {value}"),
        )
    })?;
    validate_device_ip(addr)?;
    Ok(addr)
}

fn parse_optional_ipv4(value: Option<&str>) -> Result<Option<Ipv4Addr>, AppError> {
    value.map(parse_ipv4).transpose()
}

/// Validates that a device IP is safe to contact. Allows RFC1918 private
/// addresses (expected LAN devices) but rejects loopback, link-local,
/// unspecified, broadcast, and multicast addresses.
fn validate_device_ip(addr: Ipv4Addr) -> Result<(), AppError> {
    if addr.is_loopback()
        || addr.is_link_local()
        || addr.is_unspecified()
        || addr.is_broadcast()
        || addr.is_multicast()
    {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Device IP address is not allowed: {addr}"),
        ));
    }
    // Block the cloud metadata endpoint (169.254.169.254 is already caught by
    // is_link_local, but also reject the full /16 range for safety).
    Ok(())
}

fn map_discovered_device(device: Device) -> BroadlinkDiscoveredDevice {
    let kind = match &device {
        Device::Remote { .. } => "remote",
        Device::Hvac { .. } => "hvac",
    }
    .to_string();
    let supports_learning = matches!(device, Device::Remote { .. });
    let info = device.get_info();

    BroadlinkDiscoveredDevice {
        host: info.address.to_string(),
        mac: format_mac(&info.mac),
        model_code: info.model_code,
        friendly_model: info.friendly_model,
        friendly_type: info.friendly_type,
        name: info.name,
        is_locked: info.is_locked,
        kind,
        supports_learning,
    }
}

fn is_address_in_use_discovery_error(error: &AppError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("could not send discovery message")
        && message.contains("could not bind to any port")
        && message.contains("address in use")
}

fn learn_ir_blocking(host: Ipv4Addr, local_ip: Option<Ipv4Addr>) -> Result<Vec<u8>, AppError> {
    let device = Device::from_ip(host, local_ip).map_err(AppError::service_unavailable)?;
    match device {
        Device::Remote { remote } => remote.learn_ir().map_err(AppError::service_unavailable),
        Device::Hvac { .. } => Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "The selected Broadlink device does not support IR learning",
        )),
    }
}

fn send_packet_blocking(
    host: Ipv4Addr,
    local_ip: Option<Ipv4Addr>,
    packet: Vec<u8>,
) -> Result<(), AppError> {
    let device = Device::from_ip(host, local_ip).map_err(AppError::service_unavailable)?;
    match device {
        Device::Remote { remote } => remote
            .send_code(&packet)
            .map_err(AppError::service_unavailable),
        Device::Hvac { .. } => Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "The selected Broadlink device does not support IR code sending",
        )),
    }
}

fn decode_packet(packet_base64: &str) -> Result<Vec<u8>, AppError> {
    STANDARD.decode(packet_base64).map_err(|_| {
        AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "packetBase64 must be valid base64",
        )
    })
}

fn write_string_to_path(path: &Path, content: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    tags
}

fn normalize_lookup_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_mitsubishi_entry(entry: &BroadlinkCodeEntry) -> bool {
    entry.brand.as_deref().map(normalize_lookup_value).as_deref() == Some("mitsubishi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lookup_value_trims_and_lowercases() {
        assert_eq!(normalize_lookup_value("  Cool_22_Auto "), "cool_22_auto");
    }

    #[test]
    fn normalize_tags_deduplicates_and_sorts() {
        let tags = normalize_tags(vec![
            " bedroom ".to_string(),
            "mitsubishi".to_string(),
            "bedroom".to_string(),
            "".to_string(),
        ]);
        assert_eq!(tags, vec!["bedroom", "mitsubishi"]);
    }

    #[tokio::test]
    async fn save_code_normalizes_and_persists() {
        let temp_root = std::env::temp_dir()
            .join("maison-broadlink-tests")
            .join(Uuid::new_v4().to_string());
        let path = temp_root.join("broadlink-codes.json");
        let climate_path = temp_root.join("climate-state.json");
        let manager = BroadlinkManager::new(&path, &climate_path).expect("manager should build");

        let code = manager
            .save_code(SaveCodeRequest {
                name: "Salon AC 22C".to_string(),
                brand: Some(" Mitsubishi ".to_string()),
                model: Some(" MSZ-AP ".to_string()),
                command: " Cool_22_Auto ".to_string(),
                packet_base64: STANDARD.encode([1_u8, 2, 3, 4]),
                tags: vec!["living-room".to_string(), "living-room".to_string()],
            })
            .await
            .expect("code should save");

        assert_eq!(code.brand.as_deref(), Some("mitsubishi"));
        assert_eq!(code.model.as_deref(), Some("msz-ap"));
        assert_eq!(code.command, "cool_22_auto");
        assert_eq!(code.packet_length, 4);
        assert_eq!(code.tags, vec!["living-room"]);

        let saved = std::fs::read_to_string(&path).expect("codes file should exist");
        assert!(saved.contains("cool_22_auto"));
    }

    #[tokio::test]
    async fn climate_state_is_recorded_and_reloaded() {
        let temp_root = std::env::temp_dir()
            .join("maison-broadlink-tests")
            .join(Uuid::new_v4().to_string());
        let codes_path = temp_root.join("broadlink-codes.json");
        let climate_path = temp_root.join("climate-state.json");
        let manager =
            BroadlinkManager::new(&codes_path, &climate_path).expect("manager should build");

        let on_command = "state-cool-21-fan-auto-vane-auto-wide-center-stopin-180";
        manager
            .record_climate_state(on_command, "192.168.1.50", Some("msz-hj5va"))
            .await;
        manager
            .record_climate_state("state-off", "192.168.1.50", Some("msz-hj5va"))
            .await;

        let state = manager.climate_state().await.expect("state should exist");
        assert!(!state.power);
        assert_eq!(state.last_command, "state-off");
        // Turning off must not forget the last on-state settings.
        assert_eq!(state.last_on_command.as_deref(), Some(on_command));
        let settings = state.settings.as_ref().expect("settings should be parsed");
        assert_eq!(settings.mode, "cool");
        assert_eq!(settings.temperature, 21);
        assert_eq!(settings.stop_in_minutes, Some(180));

        // A fresh manager reloads the persisted state from disk.
        let reloaded =
            BroadlinkManager::new(&codes_path, &climate_path).expect("manager should build");
        let state = reloaded.climate_state().await.expect("state should persist");
        assert_eq!(state.last_on_command.as_deref(), Some(on_command));
    }
}

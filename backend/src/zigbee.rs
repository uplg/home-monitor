//! Zigbee lamp manager backed by the native EZSP/EmberZNet driver
//! (`zigbee_native`). Persists known lamps to `zigbee-lamps.json` so they
//! survive coordinator and backend restarts.

use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::RwLock,
    task::JoinHandle,
    time::Duration,
};
use tracing::warn;

use crate::{
    config::Config,
    error::AppError,
    zigbee_native::{NativeKnownDevice, NativeZigbeeCommand, NativeZigbeeRuntime, ZigbeeDeviceType},
};

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeLampState {
    #[serde(rename = "isOn")]
    pub is_on: bool,
    pub brightness: u8,
    pub temperature: Option<u8>,
    #[serde(rename = "temperatureMin")]
    pub temperature_min: Option<u8>,
    #[serde(rename = "temperatureMax")]
    pub temperature_max: Option<u8>,
    #[serde(rename = "colorX")]
    pub color_x: Option<f32>,
    #[serde(rename = "colorY")]
    pub color_y: Option<f32>,
    #[serde(rename = "colorMode")]
    pub color_mode: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeLampView {
    pub id: String,
    pub name: String,
    pub address: String,
    #[serde(rename = "friendlyName")]
    pub friendly_name: String,
    #[serde(rename = "linkQuality")]
    pub link_quality: Option<u16>,
    #[serde(rename = "interviewCompleted")]
    pub interview_completed: bool,
    pub model: Option<String>,
    pub manufacturer: String,
    pub firmware: Option<String>,
    pub connected: bool,
    pub reachable: bool,
    #[serde(rename = "supportsBrightness")]
    pub supports_brightness: bool,
    #[serde(rename = "supportsTemperature")]
    pub supports_temperature: bool,
    #[serde(rename = "supportsColor")]
    pub supports_color: bool,
    pub state: ZigbeeLampState,
    #[serde(rename = "lastSeen")]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeStats {
    pub total: usize,
    pub connected: usize,
    pub reachable: usize,
    pub disabled: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeePairingStatus {
    pub active: bool,
    #[serde(rename = "remainingSeconds")]
    pub remaining_seconds: u16,
    #[serde(rename = "permitJoinSeconds")]
    pub permit_join_seconds: u16,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredZigbeeLampConfig {
    id: String,
    name: String,
    friendly_name: String,
    ieee_address: String,
    node_id: Option<u16>,
    endpoint: Option<u8>,
    #[serde(default)]
    input_clusters: Vec<u16>,
    #[serde(default)]
    output_clusters: Vec<u16>,
    model: Option<String>,
    manufacturer: Option<String>,
    firmware: Option<String>,
    supports_brightness: bool,
    supports_temperature: bool,
    #[serde(default)]
    supports_color: bool,
    color_temp_min: Option<u16>,
    color_temp_max: Option<u16>,
    #[serde(default)]
    is_remote: bool,
}

#[derive(Clone)]
struct ZigbeeLampRuntime {
    config: StoredZigbeeLampConfig,
    state: RuntimeLampState,
    connected: bool,
    reachable: bool,
    link_quality: Option<u16>,
    last_seen: Option<String>,
    interview_completed: bool,
}

#[derive(Clone)]
struct RuntimeLampState {
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    temperature_min: Option<u8>,
    temperature_max: Option<u8>,
    color_x: Option<f32>,
    color_y: Option<f32>,
    color_mode: Option<u8>,
}

#[derive(Default)]
struct PairingRuntime {
    active: bool,
    deadline: Option<Instant>,
    message: Option<String>,
}

#[derive(Clone)]
struct ZigbeeStore {
    lamps_path: PathBuf,
    blacklist_path: PathBuf,
}

#[derive(Clone)]
pub struct ZigbeeManager {
    inner: Arc<ZigbeeManagerInner>,
}

struct ZigbeeManagerInner {
    store: ZigbeeStore,
    lamps: RwLock<HashMap<String, ZigbeeLampRuntime>>,
    /// IEEE addresses that must never appear as lamps, enforced both at load
    /// time and on every runtime discovery sync (a blacklisted device that is
    /// still on the mesh keeps announcing itself).
    blacklisted_addresses: HashSet<String>,
    pairing: RwLock<PairingRuntime>,
    runtime: NativeZigbeeRuntime,
    permit_join_seconds: u16,
    persist_task: Mutex<Option<JoinHandle<()>>>,
}

impl ZigbeeManager {
    pub fn new(config: &Config) -> Result<Self, AppError> {
        let store = ZigbeeStore {
            lamps_path: config.zigbee_lamps_path.clone(),
            blacklist_path: config.zigbee_lamps_blacklist_path.clone(),
        };
        let blacklisted_addresses = store.load_blacklist();
        let lamps: HashMap<String, ZigbeeLampRuntime> = store
            .load_lamps()?
            .into_iter()
            .filter(|lamp| !blacklisted_addresses.contains(&lamp.ieee_address))
            .map(|lamp| {
                let state = RuntimeLampState {
                    is_on: false,
                    brightness: 0,
                    temperature: None,
                    temperature_min: lamp.color_temp_min.map(|_| 0),
                    temperature_max: lamp.color_temp_max.map(|_| 100),
                    color_x: None,
                    color_y: None,
                    color_mode: None,
                };

                (
                    lamp.id.clone(),
                    ZigbeeLampRuntime {
                        config: lamp,
                        state,
                        connected: false,
                        reachable: false,
                        link_quality: None,
                        last_seen: None,
                        interview_completed: false,
                    },
                )
            })
            .collect();

        let adapter = env::var("ZIGBEE_ADAPTER").unwrap_or_else(|_| "ember".to_string());
        let serial_port = env::var("ZIGBEE_SERIAL_PORT").ok().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        // Records without a node_id cannot be seeded into the native driver
        // (typically leftovers from the removed zigbee2mqtt era). Surface
        // them loudly instead of leaving them silently invisible forever.
        let unseedable = lamps
            .values()
            .filter(|lamp| lamp.config.node_id.is_none())
            .map(|lamp| lamp.config.ieee_address.clone())
            .collect::<Vec<_>>();
        if !unseedable.is_empty() {
            warn!(
                addresses = ?unseedable,
                "zigbee lamps without a node_id cannot be driven natively; re-pair them (or remove them from zigbee-lamps.json)"
            );
        }

        let known_devices = lamps
            .values()
            .filter_map(|lamp| {
                Some(NativeKnownDevice {
                    node_id: lamp.config.node_id?,
                    eui64: lamp.config.ieee_address.clone(),
                    endpoint: lamp.config.endpoint,
                    input_clusters: lamp.config.input_clusters.clone(),
                    output_clusters: lamp.config.output_clusters.clone(),
                    model: lamp.config.model.clone(),
                    manufacturer: lamp.config.manufacturer.clone(),
                    supports_brightness: lamp.config.supports_brightness,
                    supports_temperature: lamp.config.supports_temperature,
                    device_type: if lamp.config.is_remote {
                        ZigbeeDeviceType::Remote
                    } else {
                        ZigbeeDeviceType::Lamp
                    },
                })
            })
            .collect();

        let runtime = NativeZigbeeRuntime::spawn(adapter, serial_port, known_devices);

        let manager = Self {
            inner: Arc::new(ZigbeeManagerInner {
                store,
                lamps: RwLock::new(lamps),
                blacklisted_addresses,
                pairing: RwLock::new(PairingRuntime::default()),
                runtime,
                permit_join_seconds: config.zigbee_permit_join_seconds,
                persist_task: Mutex::new(None),
            }),
        };

        manager.spawn_persist_task();
        Ok(manager)
    }

    pub async fn list_lamps(&self) -> Vec<ZigbeeLampView> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(error = %error, "failed to sync native zigbee lamps before listing");
        }
        let lamps = self.inner.lamps.read().await;
        let mut values = lamps
            .values()
            .filter(|lamp| !lamp.config.is_remote && lamp.interview_completed)
            .map(to_view)
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.name.cmp(&right.name));
        values
    }

    pub async fn get_lamp(&self, lamp_id: &str) -> Option<ZigbeeLampView> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before loading it");
        }
        let lamps = self.inner.lamps.read().await;
        lamps.get(lamp_id).map(to_view)
    }

    pub async fn stats(&self) -> ZigbeeStats {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(error = %error, "failed to sync native zigbee stats");
        }
        let lamps = self.inner.lamps.read().await;
        ZigbeeStats {
            total: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed).count(),
            connected: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed && lamp.connected).count(),
            reachable: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed && lamp.reachable).count(),
            disabled: false,
            message: self.inner.runtime.message().await,
        }
    }

    pub async fn pairing_status(&self) -> ZigbeePairingStatus {
        let mut pairing = self.inner.pairing.write().await;
        let remaining_seconds = remaining_seconds(&mut pairing);
        let fallback_message = self.inner.runtime.message().await;

        ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing
                .message
                .clone()
                .or(fallback_message),
        }
    }

    pub async fn start_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        let seconds = self.inner.permit_join_seconds;
        self.inner
            .runtime
            .send(NativeZigbeeCommand::PermitJoin { seconds })
            .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = true;
        pairing.deadline = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
        pairing.message = Some("Native Zigbee pairing window requested".to_string());
        let remaining_seconds = remaining_seconds(&mut pairing);

        Ok(ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: seconds,
            message: pairing.message.clone(),
        })
    }

    pub async fn stop_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        self.inner
            .runtime
            .send(NativeZigbeeCommand::PermitJoin { seconds: 0 })
            .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = false;
        pairing.deadline = None;
        pairing.message = Some("Native Zigbee pairing window closed".to_string());

        Ok(ZigbeePairingStatus {
            active: false,
            remaining_seconds: 0,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing.message.clone(),
        })
    }

    /// Initiate a Touchlink (ZLL) scan to discover and commission factory-new
    /// ZLL devices that don't respond to standard permit-join.
    pub async fn touchlink_scan(&self) -> Result<(), AppError> {
        self.inner
            .runtime
            .send(NativeZigbeeCommand::TouchlinkScan)
            .await
    }

    pub async fn set_power(&self, lamp_id: &str, enabled: bool) -> Result<ZigbeeLampState, AppError> {
        self.apply_command(lamp_id, NativeZigbeeCommand::SetPower {
            lamp_id: lamp_id.to_string(),
            enabled,
        })
        .await
    }

    pub async fn set_brightness(&self, lamp_id: &str, brightness: u8) -> Result<ZigbeeLampState, AppError> {
        self.apply_command(lamp_id, NativeZigbeeCommand::SetBrightness {
            lamp_id: lamp_id.to_string(),
            brightness,
        })
        .await
    }

    pub async fn set_temperature(&self, lamp_id: &str, temperature: u8) -> Result<ZigbeeLampState, AppError> {
        self.apply_command(lamp_id, NativeZigbeeCommand::SetTemperature {
            lamp_id: lamp_id.to_string(),
            temperature,
        })
        .await
    }

    pub async fn set_color(&self, lamp_id: &str, x: f32, y: f32) -> Result<ZigbeeLampState, AppError> {
        self.apply_command(lamp_id, NativeZigbeeCommand::SetColor {
            lamp_id: lamp_id.to_string(),
            x,
            y,
        })
        .await
    }

    pub async fn set_effect(&self, lamp_id: &str, effect: &str) -> Result<ZigbeeLampState, AppError> {
        self.apply_command(lamp_id, NativeZigbeeCommand::SetEffect {
            lamp_id: lamp_id.to_string(),
            effect: effect.to_string(),
        })
        .await
    }

    /// Send a state-changing command to the driver, then re-sync and return
    /// the lamp's refreshed state. The short delay gives the device time to
    /// apply the change before the state snapshot is taken.
    async fn apply_command(
        &self,
        lamp_id: &str,
        command: NativeZigbeeCommand,
    ) -> Result<ZigbeeLampState, AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee state before command");
        }
        self.inner.runtime.send(command).await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        // The radio command already went out: a failed refresh (e.g. the
        // persistence write) must not turn the request into an error.
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee state after command");
        }
        self.current_state(lamp_id).await
    }

    pub async fn rename_lamp(&self, lamp_id: &str, name: &str) -> Result<(), AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before rename");
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::http(
                StatusCode::BAD_REQUEST,
                "Lamp name cannot be empty",
            ));
        }

        let stored = {
            let mut lamps = self.inner.lamps.write().await;
            let lamp = lamps
                .get_mut(lamp_id)
                .ok_or_else(|| not_found("Zigbee lamp not found"))?;
            lamp.config.name = trimmed.to_string();
            lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>()
        };

        self.inner.store.save_lamps(&stored)
    }

    pub async fn shutdown(&self) {
        if let Some(handle) = self.inner.persist_task.lock().expect("native persist task mutex").take() {
            handle.abort();
        }
        self.inner.runtime.shutdown().await;
    }

    async fn sync_from_runtime(&self) -> Result<(), AppError> {
        self.inner.runtime.ensure_initialized().await;
        let discovered = self.inner.runtime.snapshot_devices().await;

        let mut lamps = self.inner.lamps.write().await;
        let mut seen = HashSet::new();
        let mut changed = false;

        for device in discovered {
            if self.inner.blacklisted_addresses.contains(&device.eui64) {
                continue;
            }

            // Skip devices that haven't completed their interview or discovery yet (no
            // endpoint means we don't know what the device is — it could be a
            // sleepy remote still being discovered).
            // Remotes are included so they get persisted to disk and survive
            // coordinator reboots.  They are filtered out at API/display time
            // in list_lamps() and stats() via the is_remote flag.
            if device.endpoint.is_none() {
                continue;
            }

            let id = device.id.clone();
            let previous = lamps.get(&id).cloned();
            let runtime = lamps.entry(id.clone()).or_insert_with(|| ZigbeeLampRuntime {
                config: StoredZigbeeLampConfig {
                    id: id.clone(),
                    name: device.eui64.clone(),
                    friendly_name: device.eui64.clone(),
                    ieee_address: device.eui64.clone(),
                    node_id: Some(device.node_id),
                    endpoint: device.endpoint,
                    input_clusters: device.input_clusters.clone(),
                    output_clusters: device.output_clusters.clone(),
                    model: device.model.clone(),
                    manufacturer: device.manufacturer.clone().or_else(|| Some("Native EZSP".to_string())),
                    firmware: None,
                    supports_brightness: device.supports_brightness,
                    supports_temperature: device.supports_temperature,
                    supports_color: device.supports_color,
                    color_temp_min: if device.supports_temperature { Some(153) } else { None },
                    color_temp_max: if device.supports_temperature { Some(500) } else { None },
                    is_remote: device.device_type == ZigbeeDeviceType::Remote,
                },
                state: RuntimeLampState {
                    is_on: device.is_on,
                    brightness: device.brightness,
                    temperature: device.temperature,
                    temperature_min: if device.supports_temperature { Some(0) } else { None },
                    temperature_max: if device.supports_temperature { Some(100) } else { None },
                    color_x: device.color_x,
                    color_y: device.color_y,
                    color_mode: device.color_mode,
                },
                connected: device.connected,
                reachable: device.reachable,
                link_quality: None,
                last_seen: device.last_seen.clone(),
                interview_completed: device.endpoint.is_some(),
            });

            runtime.config.ieee_address = device.eui64.clone();
            runtime.config.node_id = Some(device.node_id);
            runtime.config.endpoint = device.endpoint;
            runtime.config.input_clusters = device.input_clusters.clone();
            runtime.config.output_clusters = device.output_clusters.clone();
            runtime.config.model = device.model.clone().or(runtime.config.model.clone());
            runtime.config.manufacturer = device.manufacturer.clone().or(runtime.config.manufacturer.clone());
            runtime.config.supports_brightness = device.supports_brightness;
            runtime.config.supports_temperature = device.supports_temperature;
            runtime.config.supports_color = device.supports_color;
            runtime.config.color_temp_min = if device.supports_temperature { Some(153) } else { None };
            runtime.config.color_temp_max = if device.supports_temperature { Some(500) } else { None };
            runtime.config.is_remote = device.device_type == ZigbeeDeviceType::Remote;
            runtime.connected = device.connected;
            runtime.reachable = device.reachable;
            runtime.interview_completed = device.endpoint.is_some();
            if device.last_seen.is_some() {
                runtime.last_seen = device.last_seen.clone();
            }
            if device.connected {
                runtime.state.is_on = device.is_on;
                runtime.state.brightness = device.brightness;
            }
            runtime.state.temperature = device.temperature;
            runtime.state.temperature_min = if device.supports_temperature { Some(0) } else { None };
            runtime.state.temperature_max = if device.supports_temperature { Some(100) } else { None };
            runtime.state.color_x = device.color_x;
            runtime.state.color_y = device.color_y;
            runtime.state.color_mode = device.color_mode;
            if runtime.config.name.trim().is_empty() {
                runtime.config.name = device.eui64.clone();
            }
            if runtime.config.friendly_name.trim().is_empty() {
                runtime.config.friendly_name = device.eui64.clone();
            }

            if !previous.map(|value| native_runtime_equals(&value, runtime)).unwrap_or(false) {
                changed = true;
            }

            seen.insert(id);
        }

        for lamp in lamps.values_mut() {
            if !seen.contains(&lamp.config.id) {
                // Don't mark remotes as disconnected — they are sleepy end
                // devices and may not appear in every snapshot.
                if lamp.config.is_remote {
                    continue;
                }
                if lamp.connected || lamp.reachable {
                    changed = true;
                }
                lamp.connected = false;
                lamp.reachable = false;
            }
        }

        if changed {
            let stored = lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>();
            drop(lamps);
            self.inner.store.save_lamps(&stored)?;
        }

        Ok(())
    }

    fn spawn_persist_task(&self) {
        if self.inner.persist_task.lock().expect("native persist task mutex").is_some() {
            return;
        }

        let manager = self.clone();
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tick.tick().await;
                if let Err(error) = manager.sync_from_runtime().await {
                    warn!(error = %error, "failed to persist native zigbee runtime state");
                }
            }
        });

        *self.inner.persist_task.lock().expect("native persist task mutex") = Some(handle);
    }

    async fn current_state(&self, lamp_id: &str) -> Result<ZigbeeLampState, AppError> {
        let lamps = self.inner.lamps.read().await;
        let lamp = lamps
            .get(lamp_id)
            .ok_or_else(|| not_found("Zigbee lamp not found"))?;
        Ok(current_state(lamp))
    }
}

impl ZigbeeStore {
    fn load_lamps(&self) -> Result<Vec<StoredZigbeeLampConfig>, AppError> {
        read_json_file(&self.lamps_path)
    }

    fn save_lamps(&self, lamps: &[StoredZigbeeLampConfig]) -> Result<(), AppError> {
        write_json_file(&self.lamps_path, lamps)
    }

    fn load_blacklist(&self) -> HashSet<String> {
        read_json_file::<Vec<String>>(&self.blacklist_path)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

fn native_runtime_equals(left: &ZigbeeLampRuntime, right: &ZigbeeLampRuntime) -> bool {
    left.config.node_id == right.config.node_id
        && left.config.endpoint == right.config.endpoint
        && left.config.input_clusters == right.config.input_clusters
        && left.config.output_clusters == right.config.output_clusters
        && left.config.supports_brightness == right.config.supports_brightness
        && left.config.supports_temperature == right.config.supports_temperature
        && left.config.supports_color == right.config.supports_color
        && left.connected == right.connected
        && left.reachable == right.reachable
        && left.state.is_on == right.state.is_on
        && left.state.brightness == right.state.brightness
        && left.state.temperature == right.state.temperature
        && left.state.color_x == right.state.color_x
        && left.state.color_y == right.state.color_y
        && left.state.color_mode == right.state.color_mode
}

fn to_view(lamp: &ZigbeeLampRuntime) -> ZigbeeLampView {
    ZigbeeLampView {
        id: lamp.config.id.clone(),
        name: lamp.config.name.clone(),
        address: lamp.config.ieee_address.clone(),
        friendly_name: lamp.config.friendly_name.clone(),
        link_quality: lamp.link_quality,
        interview_completed: lamp.interview_completed,
        model: lamp.config.model.clone(),
        manufacturer: lamp
            .config
            .manufacturer
            .clone()
            .unwrap_or_else(|| "Unknown".to_string()),
        firmware: lamp.config.firmware.clone(),
        connected: lamp.connected,
        reachable: lamp.reachable,
        supports_brightness: lamp.config.supports_brightness,
        supports_temperature: lamp.config.supports_temperature,
        supports_color: lamp.config.supports_color,
        state: current_state(lamp),
        last_seen: lamp.last_seen.clone(),
    }
}

fn current_state(lamp: &ZigbeeLampRuntime) -> ZigbeeLampState {
    ZigbeeLampState {
        is_on: lamp.state.is_on,
        brightness: lamp.state.brightness,
        temperature: lamp.state.temperature,
        temperature_min: lamp.state.temperature_min,
        temperature_max: lamp.state.temperature_max,
        color_x: lamp.state.color_x,
        color_y: lamp.state.color_y,
        color_mode: lamp.state.color_mode,
    }
}

fn remaining_seconds(pairing: &mut PairingRuntime) -> u16 {
    if !pairing.active {
        pairing.deadline = None;
        return 0;
    }

    let Some(deadline) = pairing.deadline else {
        pairing.active = false;
        return 0;
    };

    let now = Instant::now();
    if deadline <= now {
        pairing.active = false;
        pairing.deadline = None;
        return 0;
    }

    deadline.saturating_duration_since(now).as_secs().min(u16::MAX as u64) as u16
}

fn read_json_file<T>(path: &Path) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let body = fs::read(path)?;
    if body.is_empty() {
        return Ok(T::default());
    }

    Ok(serde_json::from_slice(&body)?)
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<(), AppError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let body = serde_json::to_vec_pretty(value)?;
    fs::write(path, body)?;
    Ok(())
}

fn not_found(message: impl Into<String>) -> AppError {
    AppError::http(StatusCode::NOT_FOUND, message)
}

#[cfg(test)]
mod tests {
    use super::{StoredZigbeeLampConfig, ZigbeeManager};
    use crate::{config::Config, zigbee_native::{DriverLifecycle, NativeDiscoveredDevice, ZigbeeDeviceType}};
    use tempfile::tempdir;

    #[tokio::test]
    async fn native_set_power_reaches_runtime_send_path() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();

        std::fs::create_dir_all(root.join("frontend/dist")).expect("frontend dist");
        std::fs::write(root.join("frontend/dist/index.html"), "ok").expect("index");
        std::fs::write(root.join("users.json"), "[]").expect("users");
        std::fs::write(root.join("devices.json"), "[]").expect("devices");
        std::fs::write(root.join("device-cache.json"), "[]").expect("device-cache");
        std::fs::write(root.join("broadlink-codes.json"), r#"{"codes":[]}"#).expect("broadlink");
        std::fs::write(root.join("meross-devices.json"), "[]").expect("meross");
        std::fs::write(root.join("hue-lamps.json"), "[]").expect("hue");
        std::fs::write(root.join("hue-lamps-blacklist.json"), "[]").expect("hue blacklist");
        std::fs::write(root.join("zigbee-lamps-blacklist.json"), "[]").expect("zigbee blacklist");
        std::fs::write(
            root.join("zigbee-lamps.json"),
            serde_json::to_string(&vec![StoredZigbeeLampConfig {
                id: "4b8ec60801881700".to_string(),
                name: "Test Lamp".to_string(),
                friendly_name: "4b8ec60801881700".to_string(),
                ieee_address: "4b:8e:c6:08:01:88:17:00".to_string(),
                node_id: Some(0x2e34),
                endpoint: Some(11),
                input_clusters: vec![0, 3, 4, 5, 6, 8],
                output_clusters: vec![25],
                model: Some("LTG002".to_string()),
                manufacturer: Some("Signify Netherlands B.V.".to_string()),
                firmware: None,
                supports_brightness: true,
                supports_temperature: false,
                supports_color: false,
                color_temp_min: None,
                color_temp_max: None,
                is_remote: false,
            }])
            .expect("serialize zigbee lamps"),
        )
        .expect("zigbee lamps");

        let config = Config::for_tests(root.to_path_buf());
        std::env::set_var("ZIGBEE_SERIAL_PORT", "/dev/null");
        let manager = ZigbeeManager::new(&config).expect("native manager");

        manager.inner.runtime.test_seed_devices(vec![NativeDiscoveredDevice {
                id: "4b8ec60801881700".to_string(),
                node_id: 0x2e34,
                eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
                endpoint: Some(11),
                input_clusters: vec![0, 3, 4, 5, 6, 8],
                output_clusters: vec![25],
                supports_brightness: true,
                supports_temperature: false,
                supports_color: false,
                device_type: ZigbeeDeviceType::Lamp,
                connected: true,
                reachable: true,
                is_on: true,
                brightness: 100,
                temperature: None,
                color_x: None,
                color_y: None,
                color_mode: None,
                model: Some("LTG002".to_string()),
                manufacturer: Some("Signify Netherlands B.V.".to_string()),
                last_seen: None,
        }]).await;
        manager.inner.runtime.test_set_lifecycle(DriverLifecycle::Failed("boom".to_string())).await;
        manager.inner.runtime.test_set_network_state("joined").await;

        let error = manager
            .set_power("4b8ec60801881700", false)
            .await
            .expect_err("power change should surface runtime failure");

        assert!(error.to_string().contains("boom"), "unexpected error: {error}");
    }
}

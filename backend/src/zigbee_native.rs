use std::{collections::HashMap, num::NonZero, sync::Arc, time::Duration as StdDuration};

use ashv2::{Payload, ezsp::Receiver as AshEzspReceiver, start as start_ash};
use chrono::Utc;
use ezsp::{
    Callback, Client as EzspClient, Configuration, Connection as EzspConnection, Messaging,
    Networking, Security, Utilities, Zll,
    ember::{
        Eui64, NodeId,
        aps::{Frame as EzspApsFrame, Options as EzspApsOptions},
        device::Update as EmberDeviceUpdate,
        join::Method as EmberJoinMethod,
        key::Data as EmberKeyData,
        message::Destination,
        network::{Duration as NetworkDuration, Parameters as EmberNetworkParameters, Status as EmberNetworkStatus},
        node::Type as EmberNodeType,
        security::initial,
        zll::{self, InitialSecurityState as ZllInitialSecurityState, Network as ZllNetwork},
    },
    ezsp::{config, decision, network::InitBitmask as NetworkInitBitmask, policy, value, zll::NetworkOperation as ZllNetworkOperation},
    parameters,
};
use tokio_serial::SerialPortBuilderExt;
use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, timeout},
};
use tracing::{debug, error, info, warn};

use silizium::zigbee::security::man as security_man;

use axum::http::StatusCode;

use crate::error::AppError;

const DEFAULT_EZSP_PROTOCOL_VERSION: u8 = 13;
const EZSP_CHANNEL_SIZE: usize = 256;
const EZSP_INIT_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const POLL_INTERVAL: StdDuration = StdDuration::from_millis(200);
const DISCOVERY_RETRY_INTERVAL_TICKS: u32 = 10;
const ZDO_PROFILE_ID: u16 = 0x0000;
const ZCL_GLOBAL_FRAME_CONTROL: u8 = 0x00;
const ZCL_CLUSTER_COMMAND_FRAME_CONTROL: u8 = 0x11;
const ZCL_READ_ATTRIBUTES_COMMAND_ID: u8 = 0x00;
const ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID: u8 = 0x01;
const ZCL_ON_OFF_COMMAND_OFF: u8 = 0x00;
const ZCL_ON_OFF_COMMAND_ON: u8 = 0x01;
const ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL: u8 = 0x04;
const ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR: u8 = 0x07;
const ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE: u8 = 0x0a;
const IDENTIFY_CLUSTER_ID: u16 = 0x0003;
const ZCL_IDENTIFY_TRIGGER_EFFECT_COMMAND_ID: u8 = 0x40;
/// Philips manufacturer-specific cluster for Hue effects (candle, fireplace, colorloop, etc.).
const PHILIPS_MANU_SPECIFIC_CLUSTER_ID: u16 = 0xFC03;
const PHILIPS_MANUFACTURER_CODE: u16 = 0x100B;
const PHILIPS_MULTI_COLOR_COMMAND_ID: u8 = 0x00;
/// ZCL frame control byte for manufacturer-specific cluster-specific commands (client → server).
const ZCL_MANU_CLUSTER_COMMAND_FRAME_CONTROL: u8 = 0x05;
const BASIC_CLUSTER_ID: u16 = 0x0000;
const HOME_AUTOMATION_PROFILE_ID: u16 = 0x0104;
const SIMPLE_DESC_REQ_CLUSTER_ID: u16 = 0x0004;
const ACTIVE_EP_REQ_CLUSTER_ID: u16 = 0x0005;
const DEVICE_ANNCE_CLUSTER_ID: u16 = 0x0013;
const BIND_REQ_CLUSTER_ID: u16 = 0x0021;
const SIMPLE_DESC_RSP_CLUSTER_ID: u16 = 0x8004;
const ACTIVE_EP_RSP_CLUSTER_ID: u16 = 0x8005;
const ON_OFF_CLUSTER_ID: u16 = 0x0006;
const LEVEL_CONTROL_CLUSTER_ID: u16 = 0x0008;
const COLOR_CONTROL_CLUSTER_ID: u16 = 0x0300;
const DEFAULT_SOURCE_ENDPOINT: u8 = 1;
const DEFAULT_HOME_GATEWAY_DEVICE_ID: u16 = 0x0050;

/// ZCL Level Control cluster-specific command IDs (client → server).
const ZCL_LEVEL_CONTROL_COMMAND_MOVE: u8 = 0x01;
const ZCL_LEVEL_CONTROL_COMMAND_STEP: u8 = 0x02;
const ZCL_LEVEL_CONTROL_COMMAND_STOP: u8 = 0x03;
const ZCL_LEVEL_CONTROL_COMMAND_MOVE_WITH_ON_OFF: u8 = 0x05;
const ZCL_LEVEL_CONTROL_COMMAND_STEP_WITH_ON_OFF: u8 = 0x06;
const ZCL_LEVEL_CONTROL_COMMAND_STOP_WITH_ON_OFF: u8 = 0x07;

/// Brightness step applied per dimmer button press (in percentage points, 0–100 scale).
const DIMMER_BRIGHTNESS_STEP: u8 = 15;
/// Manufacturer-specific cluster used by Philips Hue remotes for button
/// notifications (`hueNotification` command).
const PHILIPS_SPECIFIC_CLUSTER_ID: u16 = 0xFC00;
/// Window during which duplicate remote power/brightness commands are suppressed.
/// A single physical button press on a Philips Hue dimmer generates commands on
/// both the standard ZCL cluster and the Philips-specific cluster; without
/// deduplication the second one can hit lamps that changed state in between.
const REMOTE_DEDUP_WINDOW: StdDuration = StdDuration::from_secs(3);
const DEFAULT_STACK_PROFILE: u16 = 2;
const DEFAULT_SECURITY_LEVEL: u16 = 5;
const DEFAULT_NETWORK_CHANNEL: u8 = 11;
const DEFAULT_NETWORK_TX_POWER: i8 = 8;
const DEFAULT_LOCAL_INPUT_CLUSTERS: &[u16] = &[0x0000, 0x0006, 0x0008, 0x0300, 0x0403, 0x0201, 0xFC00];
const DEFAULT_LOCAL_OUTPUT_CLUSTERS: &[u16] = &[0x0000, 0x0006, 0x0008, 0x0300, 0x0403];
const ZIGBEE_ALLIANCE09_LINK_KEY: EmberKeyData = *b"ZigBeeAlliance09";

/// ZLL (Touchlink) constants.
/// EZSP policy decision: enable ZLL message processing.
const ZLL_POLICY_ENABLED: u8 = 0x00;
/// Primary channel mask for ZLL Touchlink scan (channels 11, 15, 20, 25).
const ZLL_PRIMARY_CHANNEL_MASK: u32 = (1 << 11) | (1 << 15) | (1 << 20) | (1 << 25);
/// Extended channel mask: all Zigbee channels 11–26 (fallback if primary scan finds nothing).
const ZLL_ALL_CHANNELS_MASK: u32 = 0x07FFF800;
/// ZLL master encryption key (shared by all ZLL certified devices).
/// This is the well-known "ZigBee Light Link Master Key" used to decrypt the
/// network key during Touchlink commissioning.
const ZLL_MASTER_KEY: [u8; 16] = [
    0x9F, 0x55, 0x95, 0xF1, 0x02, 0x57, 0xC8, 0xA4,
    0x69, 0xCB, 0xF4, 0x2B, 0xC9, 0x3F, 0xEE, 0x31,
];
/// ZLL InitialSecurityState bitmask — reserved for future use per EZSP docs.
const ZLL_SECURITY_KEY_BITMASK: u32 = 0;

/// Maximum time to wait for a Touchlink scan to complete.
const TOUCHLINK_SCAN_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Zigbee device type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZigbeeDeviceType {
    /// A controllable light (has On/Off, Level Control, or Color Control input clusters).
    Lamp,
    /// A remote control / dimmer switch (sends On/Off and Level Control commands via output clusters).
    Remote,
    /// Any other device type we don't specifically handle.
    Unknown,
}

/// How often the availability checker runs, in tick counts (5 ticks * 200ms = 1 second).
const AVAILABILITY_CHECK_INTERVAL_TICKS: u32 = 5;

/// Base timeout for active (router/lamp) devices.  If no message has been received
/// from the device within this window, an active ping is sent to check reachability.
/// Kept short (60s) because our small network has very little passive Zigbee
/// traffic — wall-switch power cuts are only detected when our ping gets no reply.
const AVAILABILITY_TIMEOUT: StdDuration = StdDuration::from_secs(60);

/// Delay between consecutive ping attempts for a single device.
const PING_RETRY_DELAY: StdDuration = StdDuration::from_secs(3);

/// Maximum exponential-backoff multiplier for offline devices.  After repeated
/// ping failures the effective timeout grows as `AVAILABILITY_TIMEOUT * multiplier`,
/// capped at this value so we still re-check dead devices periodically.
const MAX_BACKOFF_MULTIPLIER: f64 = 12.0;

/// Duration after which the EZSP pipeline is considered stuck if no EZSP activity
/// (successful command or callback) has been observed.  Triggers a full reconnect.
const WATCHDOG_TIMEOUT: StdDuration = StdDuration::from_secs(180);

/// Per-EZSP-command timeout.  If a single `send_unicast` / `communicate` call takes
/// longer than this, we assume the pipeline is dead and trigger a full reconnect.
/// This is safe ONLY because we tear down the entire pipeline afterward — we never
/// reuse a desynchronised EZSP channel.
const EZSP_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Touchlink scans legitimately run two 15-second radio passes plus
/// per-device delays, so they get a larger budget than regular commands.
const TOUCHLINK_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(90);

/// Budget for one desired-state restore sweep (multiple devices, several
/// unicasts each). Exceeding it pauses the sweep until the next tick — it
/// does not tear the pipeline down.
const RESTORE_SWEEP_TIMEOUT: StdDuration = StdDuration::from_secs(60);

/// Upper bound for the full network bring-up (endpoint + stack configuration,
/// network init/form, security state). Prevents the driver from being stuck
/// in `Starting` forever when the NCP hangs mid-initialization.
const NETWORK_INIT_TIMEOUT: StdDuration = StdDuration::from_secs(60);

/// Upper bound a caller of `NativeZigbeeRuntime::send` may wait for the reply.
/// Must exceed the worst per-command driver timeout plus queueing behind other
/// commands; it exists so HTTP handlers can never hang indefinitely. Keep the
/// sum with `COMMAND_ENQUEUE_TIMEOUT` below the 180s global HTTP timeout so
/// callers see the specific error, not a generic gateway timeout.
const COMMAND_REPLY_TIMEOUT: StdDuration = StdDuration::from_secs(120);

/// Upper bound for getting a command *into* the driver queue. If the 32-slot
/// queue stays full this long, the driver is wedged or saturated — fail fast
/// rather than stacking more work behind it.
const COMMAND_ENQUEUE_TIMEOUT: StdDuration = StdDuration::from_secs(5);

/// Delay before attempting to reconnect after a pipeline failure.
const RECONNECT_DELAY: StdDuration = StdDuration::from_secs(2);

/// Longest delay between reconnect attempts once backoff has kicked in.
const MAX_RECONNECT_DELAY: StdDuration = StdDuration::from_secs(300);

/// After this many consecutive failed attempts the driver reports
/// `DriverLifecycle::Failed` (so API calls fail fast with the underlying
/// error), but it KEEPS retrying in the background — the radio must never
/// require a process restart to come back.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

#[derive(Debug, Clone)]
pub enum NativeZigbeeCommand {
    PermitJoin { seconds: u16 },
    DiscoverDevices,
    SetPower { lamp_id: String, enabled: bool },
    SetBrightness { lamp_id: String, brightness: u8 },
    SetTemperature { lamp_id: String, temperature: u8 },
    SetColor { lamp_id: String, x: f32, y: f32 },
    SetEffect { lamp_id: String, effect: String },
    /// Initiate a Touchlink (ZLL) scan to discover and commission factory-new
    /// ZLL devices (e.g. Hue Lightstrip Plus) that don't respond to standard
    /// NWK-level permit-join.
    TouchlinkScan,
}

#[derive(Debug, Clone)]
pub enum NativeZigbeeEvent {
    NetworkState { status: String },
    DeviceJoined { node_id: u16, eui64: String },
    DeviceAnnounced { node_id: u16, eui64: String },
    DeliveryFailed { node_id: u16 },
    IncomingMessage { node_id: u16, cluster_id: u16, payload: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct NativeDiscoveredDevice {
    pub id: String,
    pub node_id: u16,
    pub eui64: String,
    pub endpoint: Option<u8>,
    pub input_clusters: Vec<u16>,
    pub output_clusters: Vec<u16>,
    pub supports_brightness: bool,
    pub supports_temperature: bool,
    pub supports_color: bool,
    pub device_type: ZigbeeDeviceType,
    pub connected: bool,
    pub reachable: bool,
    pub is_on: bool,
    pub brightness: u8,
    pub temperature: Option<u8>,
    pub color_x: Option<f32>,
    pub color_y: Option<f32>,
    pub color_mode: Option<u8>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NativeKnownDevice {
    pub node_id: u16,
    pub eui64: String,
    pub endpoint: Option<u8>,
    pub input_clusters: Vec<u16>,
    pub output_clusters: Vec<u16>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub supports_brightness: bool,
    pub supports_temperature: bool,
    pub device_type: ZigbeeDeviceType,
}

#[derive(Debug, Default)]
pub(crate) struct NativeZigbeeStatus {
    message: Option<String>,
    last_error: Option<String>,
    devices: Vec<NativeDiscoveredDevice>,
}

struct DriverRequest {
    command: NativeZigbeeCommand,
    reply_tx: oneshot::Sender<Result<(), AppError>>,
}

pub(crate) enum DriverLifecycle {
    Starting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverNetworkState {
    Unknown,
    NoNetwork,
    Joined,
}

#[derive(Clone)]
pub struct NativeZigbeeRuntime {
    status: Arc<RwLock<NativeZigbeeStatus>>,
    command_tx: mpsc::Sender<DriverRequest>,
    command_rx: Arc<Mutex<Option<mpsc::Receiver<DriverRequest>>>>,
    task: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
    init_once: Arc<Mutex<bool>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
    network_state: Arc<RwLock<DriverNetworkState>>,
    adapter: Arc<String>,
    serial_port: Arc<Option<String>>,
    known_devices: Arc<Vec<NativeKnownDevice>>,
}

impl NativeZigbeeRuntime {
    pub fn spawn(adapter: String, serial_port: Option<String>, known_devices: Vec<NativeKnownDevice>) -> Self {
        let adapter_label = adapter.clone();
        let serial_port_label = serial_port.clone();
        let status = Arc::new(RwLock::new(NativeZigbeeStatus {
            message: Some(match &serial_port {
                Some(port) => format!("Native Zigbee adapter queued for initialization on {port} ({adapter})"),
                None => format!("Native Zigbee adapter {adapter} selected; set ZIGBEE_SERIAL_PORT to enable it"),
            }),
            last_error: None,
            devices: Vec::new(),
        }));

        let (command_tx, command_rx) = mpsc::channel(32);

        Self {
            status,
            command_tx,
            command_rx: Arc::new(Mutex::new(Some(command_rx))),
            task: Arc::new(std::sync::Mutex::new(None)),
            init_once: Arc::new(Mutex::new(false)),
            lifecycle: Arc::new(RwLock::new(DriverLifecycle::Starting)),
            network_state: Arc::new(RwLock::new(DriverNetworkState::Unknown)),
            adapter: Arc::new(adapter_label),
            serial_port: Arc::new(serial_port_label),
            known_devices: Arc::new(known_devices),
        }
    }

    pub async fn message(&self) -> Option<String> {
        let status = self.status.read().await;
        status
            .message
            .clone()
            .or_else(|| status.last_error.clone())
    }

    pub async fn snapshot_devices(&self) -> Vec<NativeDiscoveredDevice> {
        self.status.read().await.devices.clone()
    }

    #[cfg(test)]
    pub(crate) async fn test_seed_devices(&self, devices: Vec<NativeDiscoveredDevice>) {
        let mut status = self.status.write().await;
        status.devices = devices;
        status.message = None;
        status.last_error = None;
    }

    #[cfg(test)]
    pub(crate) async fn test_set_lifecycle(&self, lifecycle: DriverLifecycle) {
        *self.lifecycle.write().await = lifecycle;
    }

    #[cfg(test)]
    pub(crate) async fn test_set_network_state(&self, state: &str) {
        *self.network_state.write().await = match state {
            "joined" => DriverNetworkState::Joined,
            "no-network" => DriverNetworkState::NoNetwork,
            _ => DriverNetworkState::Unknown,
        };
    }

    pub async fn ensure_initialized(&self) {
        self.start_task_if_needed().await;

        let mut guard = self.init_once.lock().await;
        if *guard {
            return;
        }

        if let Err(error) = wait_for_driver_ready(&self.lifecycle).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee driver did not become ready");
            let mut status = self.status.write().await;
            status.last_error = Some(error.to_string());
            status.message = Some("Native Zigbee initialization timed out".to_string());
            return;
        }

        if let Err(error) = wait_for_joined_network(&self.network_state).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee network did not become joined before discovery");
            let mut status = self.status.write().await;
            status.last_error = Some(error.to_string());
            status.message = Some(format!("Native Zigbee network not joined: {error}"));
            return;
        }

        if let Err(error) = self.send(NativeZigbeeCommand::DiscoverDevices).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee lazy initialization failed");
            let mut status = self.status.write().await;
            status.last_error = Some(error.to_string());
            status.message = Some("Native Zigbee lazy initialization failed".to_string());
            return;
        }

        *guard = true;
    }

    pub async fn send(&self, command: NativeZigbeeCommand) -> Result<(), AppError> {
        self.start_task_if_needed().await;

        match &*self.lifecycle.read().await {
            DriverLifecycle::Starting => {
                return Err(AppError::service_unavailable(
                    "Native Zigbee adapter is still initializing",
                ));
            }
            DriverLifecycle::Failed(message) => {
                return Err(AppError::service_unavailable(message.clone()));
            }
            DriverLifecycle::Ready => {}
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        // Both steps are bounded so an HTTP caller can never hang forever,
        // even if the driver event loop wedges while reporting Ready.
        timeout(
            COMMAND_ENQUEUE_TIMEOUT,
            self.command_tx.send(DriverRequest { command, reply_tx }),
        )
        .await
        .map_err(|_| {
            AppError::service_unavailable("Native Zigbee driver is not accepting commands (queue full)")
        })?
        .map_err(|_| AppError::service_unavailable("Native Zigbee driver task is not running"))?;

        timeout(COMMAND_REPLY_TIMEOUT, reply_rx)
            .await
            .map_err(|_| {
                AppError::service_unavailable("Native Zigbee driver did not answer in time")
            })?
            .map_err(|_| AppError::service_unavailable("Native Zigbee driver dropped the command response"))?
    }

    async fn start_task_if_needed(&self) {
        if self.task.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            return;
        }

        let mut command_rx_guard = self.command_rx.lock().await;
        let Some(command_rx) = command_rx_guard.take() else {
            return;
        };

        let adapter = (*self.adapter).clone();
        let serial_port = (*self.serial_port).clone();
        let known_devices = (*self.known_devices).clone();
        let task_status = Arc::clone(&self.status);
        let lifecycle = Arc::clone(&self.lifecycle);
        let network_state = Arc::clone(&self.network_state);

        let task = tokio::spawn(async move {
            run_native_driver(adapter, serial_port, known_devices, task_status, lifecycle, network_state, command_rx).await;
        });

        *self.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
    }

    pub async fn shutdown(&self) {
        if let Some(handle) = self.task.lock().unwrap_or_else(|e| e.into_inner()).take() {
            handle.abort();
        }
    }
}

/// Join handles for the four pipeline tasks (ASH transmitter/receiver and
/// EZSP transmitter/receiver actors). All spawned by us, so teardown is a
/// bounded abort + join.
struct PipelineTasks {
    ash_transmitter: JoinHandle<()>,
    ash_receiver: JoinHandle<()>,
    ezsp_transmitter: JoinHandle<()>,
    ezsp_receiver: JoinHandle<()>,
}

impl PipelineTasks {
    /// All four tasks must be running for the pipeline to be usable; any
    /// finished task (serial unplug, channel closure, panic) means the whole
    /// stack must be rebuilt.
    fn is_alive(&self) -> bool {
        !self.ash_transmitter.is_finished()
            && !self.ash_receiver.is_finished()
            && !self.ezsp_transmitter.is_finished()
            && !self.ezsp_receiver.is_finished()
    }

    async fn shutdown(self) {
        for handle in [
            self.ezsp_transmitter,
            self.ezsp_receiver,
            self.ash_transmitter,
            self.ash_receiver,
        ] {
            handle.abort();
            let _ = handle.await;
        }
    }

}

struct EzspContext {
    connection: EzspConnection,
    tasks: PipelineTasks,
    callbacks_rx: mpsc::Receiver<Callback>,
    joined_devices: Vec<DiscoveredDevice>,
    next_global_sequence: u8,
    next_device_sequence: HashMap<u16, u8>,
    /// Updated on every successful EZSP command or callback.  Used by the watchdog
    /// to detect a silently-dead pipeline.
    last_activity: Instant,
    /// The coordinator's own EUI64, fetched once at startup.
    /// Needed for ZDO Bind_req destination addresses.
    coordinator_eui64: Option<Eui64>,
    /// Deduplication for remote power commands: a single physical button press on
    /// a Philips Hue dimmer generates commands on both the standard On/Off cluster
    /// (0x0006) and the Philips-specific cluster (0xFC00).  We record the last
    /// broadcast power action and its timestamp so the duplicate is suppressed.
    last_remote_power: Option<(bool, Instant)>,
    /// Same deduplication for brightness step commands.
    last_remote_brightness_step: Option<(bool, Instant)>,
    /// Touchlink scan state: collects ZLL networks found during a scan.
    touchlink_found_networks: Vec<TouchlinkFoundNetwork>,
    /// Set to `true` while a Touchlink scan is in progress, `false` when ScanComplete fires.
    touchlink_scan_in_progress: bool,
    /// Number of consecutive command errors without any success.  Used to detect
    /// a silently-broken pipeline that returns errors instead of timing out.
    consecutive_errors: u32,
    /// Non-blocking availability ping state machine.  Tracks the in-progress ping
    /// so that the event loop can process user commands between ping attempts.
    availability_state: AvailabilityState,
    /// Queue of devices whose availability timer has expired and need pinging.
    /// Populated by [`tick_device_availability`] when in `Idle` state.
    availability_queue: Vec<AvailabilityTarget>,
}

/// Target device for an availability ping, snapshotted from `joined_devices`.
#[derive(Debug, Clone)]
struct AvailabilityTarget {
    node_id: u16,
    endpoint: u8,
    was_reachable: bool,
    eui64: String,
}

/// Non-blocking state machine for availability pings.
///
/// Instead of blocking the event loop with `sleep()` calls, the state machine
/// advances one step per tick (~1 second).  User commands can be processed
/// between steps.
#[derive(Debug)]
enum AvailabilityState {
    /// No ping in progress.  On the next availability tick, scan for expired
    /// devices and start pinging the first one.
    Idle,
    /// A ping has been sent.  Waiting for the response (via callback).
    WaitingForResponse {
        node_id: u16,
        eui64: String,
        was_reachable: bool,
        attempt: u32,
        max_attempts: u32,
        /// Timestamp just before the ping was sent.  If `last_seen` is updated
        /// to a value after this, the device responded.
        before_ping: Instant,
        /// When to check for the response (after PING_RETRY_DELAY).
        check_at: Instant,
    },
    /// Cooldown between pinging different devices (2 seconds).
    CoolDown {
        resume_at: Instant,
    },
}

/// A ZLL device discovered during a Touchlink scan.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields retained for future diagnostics/logging
struct TouchlinkFoundNetwork {
    network_info: ZllNetwork,
    device_endpoint: Option<u8>,
    device_id: Option<u16>,
    rssi: i8,
}

#[derive(Debug, Clone)]
struct DiscoveredDevice {
    node_id: u16,
    eui64: String,
    endpoint: Option<u8>,
    input_clusters: Vec<u16>,
    output_clusters: Vec<u16>,
    device_type: ZigbeeDeviceType,
    supports_brightness: bool,
    supports_temperature: bool,
    has_color_control_cluster: bool,
    /// True only if the device supports CIE XY color (bit 3 of ZCL colorCapabilities 0x400A).
    /// White Ambiance lamps have Color Control cluster for temperature only — this stays false.
    supports_xy_color: bool,
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    /// CIE 1931 color X (0.0–1.0), read from attribute 0x0003.
    color_x: Option<f32>,
    /// CIE 1931 color Y (0.0–1.0), read from attribute 0x0004.
    color_y: Option<f32>,
    /// ZCL colorMode attribute (0x0008): 0=HS, 1=XY, 2=ColorTemperature.
    color_mode: Option<u8>,
    interview_completed: bool,
    model: Option<String>,
    manufacturer: Option<String>,
    connected: bool,
    reachable: bool,
    interview_attempts: u32,
    last_seen: Option<Instant>,
    /// Last user-requested brightness (0–100).  Re-applied when the device becomes reachable again.
    desired_brightness: Option<u8>,
    /// Last user-requested colour temperature (0–100).  Re-applied on reconnect.
    desired_temperature: Option<u8>,
    /// Last user-requested CIE XY color.  Re-applied on reconnect.
    desired_color_x: Option<f32>,
    desired_color_y: Option<f32>,
    /// Set to `false` when the device becomes unreachable; set back to `true` after
    /// `restore_desired_state` successfully re-sends the desired brightness/temperature.
    /// This prevents re-sending every 30 s — we only restore on the unreachable→reachable transition.
    desired_state_applied: bool,
    /// Timestamp of the last `request_known_device_discovery` call for this device.
    /// Used to rate-limit intercept-on-wake discovery so a chatty uninterviewed
    /// device cannot starve the command channel with endless EZSP round-trips.
    last_discovery_at: Option<Instant>,
    /// Number of consecutive failed availability ping cycles.  Drives exponential
    /// backoff: the effective availability timeout is multiplied by a factor
    /// derived from this counter.  Reset to 0 on any successful communication.
    failed_ping_cycles: u32,
}

async fn run_native_driver(
    adapter: String,
    serial_port: Option<String>,
    known_devices: Vec<NativeKnownDevice>,
    status: Arc<RwLock<NativeZigbeeStatus>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
    driver_network_state: Arc<RwLock<DriverNetworkState>>,
    mut command_rx: mpsc::Receiver<DriverRequest>,
) {
    let Some(serial_port) = serial_port else {
        warn!(adapter = %adapter, "native zigbee serial port is not configured");
        set_status(
            &status,
            Some(format!("Native Zigbee adapter {adapter} selected, but no serial port is configured")),
            None,
        )
        .await;
        *lifecycle.write().await = DriverLifecycle::Failed(
            "Set ZIGBEE_SERIAL_PORT before using the native Zigbee backend".to_string(),
        );
        drain_pending_requests(
            &mut command_rx,
            AppError::service_unavailable("Set ZIGBEE_SERIAL_PORT before using the native Zigbee backend"),
        )
        .await;
        return;
    };

    let seed_devices: Vec<DiscoveredDevice> = known_devices.into_iter().map(seed_known_device).collect();

    // ---- Outer reconnect loop ----
    // On the first iteration `saved_devices` comes from the config.
    // On subsequent iterations it comes from the old context's device list
    // (which includes runtime-discovered state, desired brightness, etc.).
    let mut saved_devices = seed_devices;
    let mut reconnect_attempts: u32 = 0;

    'reconnect: loop {
        // --- Open EZSP context ---
        let context_result = match adapter.as_str() {
            "ember" => open_ezsp_context(&serial_port).await,
            other => Err(AppError::service_unavailable(format!(
                "Unsupported native Zigbee adapter: {other}"
            ))),
        };

        let mut context = match context_result {
            Ok(context) => context,
            Err(error) => {
                reconnect_attempts += 1;
                warn!(
                    adapter = %adapter,
                    serial_port = %serial_port,
                    attempt = reconnect_attempts,
                    error = %error,
                    "failed to open EZSP context — retrying"
                );
                backoff_before_retry(
                    &status,
                    &lifecycle,
                    &serial_port,
                    reconnect_attempts,
                    error.to_string(),
                )
                .await;
                continue 'reconnect;
            }
        };

        context.joined_devices = saved_devices.clone();

        // The full bring-up is bounded: a hung NCP mid-initialization must
        // not leave the driver in `Starting` forever with no watchdog.
        let network_state = match timeout(
            NETWORK_INIT_TIMEOUT,
            ensure_coordinator_network(&mut context, &serial_port),
        )
        .await
        .unwrap_or_else(|_elapsed| {
            Err(AppError::service_unavailable(format!(
                "Native Zigbee network bring-up timed out after {NETWORK_INIT_TIMEOUT:?}"
            )))
        }) {
            Ok(state) => state,
            Err(error) => {
                reconnect_attempts += 1;
                warn!(
                    serial_port = %serial_port,
                    attempt = reconnect_attempts,
                    error = %error,
                    "network init failed — tearing down and retrying"
                );
                teardown_context(context).await;
                backoff_before_retry(
                    &status,
                    &lifecycle,
                    &serial_port,
                    reconnect_attempts,
                    error.to_string(),
                )
                .await;
                continue 'reconnect;
            }
        };

        // Successful init — reset reconnect counter.
        reconnect_attempts = 0;

        *driver_network_state.write().await = ember_network_state_to_driver_state(network_state);
        *lifecycle.write().await = DriverLifecycle::Ready;
        set_status(
            &status,
            Some(format!(
                "Native Zigbee connected on {serial_port} with network state {}",
                network_state_label(network_state)
            )),
            None,
        )
        .await;

        info!(adapter = %adapter, serial_port = %serial_port, "native zigbee EZSP stack initialized");

        // ---- Inner event loop ----
        let mut tick = interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut tick_count: u32 = 0;

        let reconnect_reason: Option<String> = 'event_loop: loop {
            tokio::select! {
                maybe_request = command_rx.recv() => {
                    let Some(request) = maybe_request else {
                        // Channel closed — clean shutdown.
                        teardown_context(context).await;
                        return;
                    };

                    // If the caller already gave up (HTTP timeout, dropped
                    // connection), executing the queued command now would be
                    // a surprise double execution after the user retried.
                    if request.reply_tx.is_closed() {
                        debug!("skipping queued zigbee command whose caller gave up");
                        continue;
                    }

                    // Wrap the command in a timeout.  If it fires, the EZSP
                    // pipeline is assumed dead — we tear everything down and
                    // reconnect.  This is safe because we never reuse the
                    // pipeline after a timeout.
                    let command_timeout = command_timeout_for(&request.command);
                    let result = match timeout(command_timeout, handle_command(&mut context, request.command)).await {
                        Ok(inner_result) => inner_result,
                        Err(_elapsed) => {
                            error!(
                                serial_port = %serial_port,
                                "EZSP command timed out after {:?} — triggering reconnect",
                                command_timeout,
                            );
                            let _ = request.reply_tx.send(Err(AppError::service_unavailable(
                                "EZSP command timed out — reconnecting pipeline",
                            )));
                            break 'event_loop Some(format!(
                                "EZSP command timed out after {:?}",
                                command_timeout,
                            ));
                        }
                    };

                    match &result {
                        Ok(()) => {
                            context.last_activity = Instant::now();
                            context.consecutive_errors = 0;
                            sync_status_devices(&status, &context.joined_devices).await;
                        }
                        Err(error) => {
                            context.consecutive_errors += 1;
                            let error_msg = error.to_string();
                            warn!(
                                serial_port = %serial_port,
                                error = %error_msg,
                                consecutive_errors = context.consecutive_errors,
                                "native zigbee command failed"
                            );

                            // Detect transport-level corruption that makes the
                            // pipeline permanently non-functional.  These errors
                            // mean the ASH serial framing is desynchronised and
                            // no subsequent command will succeed.
                            let is_transport_corruption =
                                error_msg.contains("frame too short")
                                || error_msg.contains("wrong CRC")
                                || error_msg.contains("communication failure");

                            if is_transport_corruption {
                                error!(
                                    serial_port = %serial_port,
                                    error = %error_msg,
                                    "EZSP transport corruption detected — triggering reconnect"
                                );
                                let reason = format!("EZSP transport corruption: {error_msg}");
                                let _ = request.reply_tx.send(result);
                                break 'event_loop Some(reason);
                            }

                            // Safety net: if commands fail repeatedly without
                            // any success in between, the pipeline is likely
                            // dead even if the error messages don't match our
                            // known corruption patterns.
                            const MAX_CONSECUTIVE_ERRORS: u32 = 5;
                            if context.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                error!(
                                    serial_port = %serial_port,
                                    consecutive_errors = context.consecutive_errors,
                                    "too many consecutive EZSP errors — triggering reconnect"
                                );
                                let reason = format!(
                                    "{} consecutive EZSP command failures",
                                    context.consecutive_errors,
                                );
                                let _ = request.reply_tx.send(result);
                                break 'event_loop Some(reason);
                            }
                        }
                    }
                    if let Err(error) = request.reply_tx.send(result) {
                        warn!(error = ?error, "native zigbee command response receiver dropped");
                    }
                }
                _ = tick.tick() => {
                    tick_count = tick_count.wrapping_add(1);

                    // --- Health check: ASH tasks alive? ---
                    if let Some(reason) = check_pipeline_health(&context) {
                        break 'event_loop Some(reason.to_string());
                    }

                    // --- Drain callbacks ---
                    // Each callback may issue EZSP round-trips (discovery,
                    // binding, broadcasts), so it gets the same timeout as a
                    // command: a hung callback must not freeze the event loop
                    // while the lifecycle still reports Ready.
                    while let Ok(callback) = context.callbacks_rx.try_recv() {
                        context.last_activity = Instant::now();
                        let handled = match timeout(EZSP_COMMAND_TIMEOUT, handle_callback(&mut context, callback)).await {
                            Ok(handled) => handled,
                            Err(_elapsed) => {
                                break 'event_loop Some(format!(
                                    "EZSP callback handling timed out after {EZSP_COMMAND_TIMEOUT:?}"
                                ));
                            }
                        };
                        if let Some(event) = handled {
                            debug!(event = ?event, "native zigbee callback handled");
                            match event {
                                NativeZigbeeEvent::NetworkState { status: network_status } => {
                                    *driver_network_state.write().await = match network_status.as_str() {
                                        "joined" => DriverNetworkState::Joined,
                                        "no-network" => DriverNetworkState::NoNetwork,
                                        _ => DriverNetworkState::Unknown,
                                    };
                                    set_status(&status, Some(format!("Native Zigbee network state: {network_status}")), None).await;
                                }
                                NativeZigbeeEvent::DeviceJoined { node_id, eui64 } => {
                                    set_status(&status, Some(format!("Native Zigbee device joined: {eui64} ({node_id:#06x})")), None).await;
                                    sync_status_devices(&status, &context.joined_devices).await;
                                }
                                NativeZigbeeEvent::DeviceAnnounced { node_id, eui64 } => {
                                    set_status(&status, Some(format!("Native Zigbee device announced: {eui64} ({node_id:#06x})")), None).await;
                                    sync_status_devices(&status, &context.joined_devices).await;
                                }
                                NativeZigbeeEvent::DeliveryFailed { .. } => {
                                    sync_status_devices(&status, &context.joined_devices).await;
                                }
                                NativeZigbeeEvent::IncomingMessage { node_id, cluster_id, payload } => {
                                    debug!(node_id, cluster_id, payload = %hex_bytes(&payload), "native zigbee incoming message");
                                }
                            }
                        }
                    }

                    if tick_count % DISCOVERY_RETRY_INTERVAL_TICKS == 0 {
                        // Wrap interview retries in a timeout — a hung send_unicast here
                        // would block the entire event loop.
                        if timeout(EZSP_COMMAND_TIMEOUT, retry_pending_interviews(&mut context)).await.is_err() {
                            break 'event_loop Some("interview retry timed out".to_string());
                        }
                        sync_status_devices(&status, &context.joined_devices).await;
                    }

                    if tick_count % AVAILABILITY_CHECK_INTERVAL_TICKS == 0 {
                        // Non-blocking availability state machine: advances one step
                        // per call.  The only blocking work is a single send_read_attributes
                        // (guarded by EZSP_COMMAND_TIMEOUT via the pipeline).  No sleeps.
                        if timeout(EZSP_COMMAND_TIMEOUT, tick_device_availability(&mut context)).await
                            .unwrap_or(false)
                        {
                            // Device state changed — sync to frontend and try to
                            // restore desired state on newly-reachable devices.
                            // Bounded, but a timeout is NOT treated as a dead
                            // pipeline: after a power outage many devices flip
                            // reachable at once and the sweep can legitimately
                            // exceed the budget on a healthy-but-busy link. The
                            // unapplied flags persist, so the sweep resumes next
                            // tick; genuine deadness is caught by the command
                            // timeout and the watchdog.
                            if timeout(RESTORE_SWEEP_TIMEOUT, restore_desired_state(&mut context)).await.is_err() {
                                warn!(serial_port = %serial_port, "desired-state restore sweep exceeded its budget — will resume next tick");
                            }
                            sync_status_devices(&status, &context.joined_devices).await;
                        }
                    }
                }
            }
        };

        // --- Reconnect path ---
        if let Some(reason) = reconnect_reason {
            saved_devices = context.joined_devices.clone();
            reconnect_attempts += 1;

            warn!(
                serial_port = %serial_port,
                reason = %reason,
                attempt = reconnect_attempts,
                "EZSP pipeline unhealthy — tearing down and reconnecting"
            );
            teardown_context(context).await;
            backoff_before_retry(&status, &lifecycle, &serial_port, reconnect_attempts, reason)
                .await;
            // continue 'reconnect — the outer loop re-opens the context
        }
    }
}

/// Update lifecycle/status and sleep before the next reconnect attempt.
///
/// The driver NEVER gives up permanently: early attempts keep the lifecycle
/// in `Starting` with a short delay; past `MAX_RECONNECT_ATTEMPTS` the
/// lifecycle flips to `Failed` so API calls fail fast with the underlying
/// error, while the loop keeps retrying with capped exponential backoff.
async fn backoff_before_retry(
    status: &Arc<RwLock<NativeZigbeeStatus>>,
    lifecycle: &Arc<RwLock<DriverLifecycle>>,
    serial_port: &str,
    attempts: u32,
    reason: String,
) {
    let exponent = attempts.saturating_sub(1).min(8);
    let delay = RECONNECT_DELAY
        .saturating_mul(1_u32 << exponent)
        .min(MAX_RECONNECT_DELAY);

    *lifecycle.write().await = if attempts > MAX_RECONNECT_ATTEMPTS {
        DriverLifecycle::Failed(format!(
            "Native Zigbee unavailable on {serial_port} ({reason}); still retrying every {delay:?}"
        ))
    } else {
        DriverLifecycle::Starting
    };

    set_status(
        status,
        Some(format!(
            "Reconnecting native Zigbee on {serial_port} (attempt {attempts}, next try in {delay:?})"
        )),
        Some(reason),
    )
    .await;

    tokio::time::sleep(delay).await;
}

/// Per-command timeout: touchlink scans are legitimately long-running, all
/// other commands are single EZSP round-trip sequences.
fn command_timeout_for(command: &NativeZigbeeCommand) -> StdDuration {
    match command {
        NativeZigbeeCommand::TouchlinkScan => TOUCHLINK_COMMAND_TIMEOUT,
        _ => EZSP_COMMAND_TIMEOUT,
    }
}

async fn open_ezsp_context(serial_port: &str) -> Result<EzspContext, AppError> {
    let protocol_version = std::env::var("ZIGBEE_EZSP_PROTOCOL_VERSION")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(DEFAULT_EZSP_PROTOCOL_VERSION);
    let attempts = [
        (115_200_u32, tokio_serial::FlowControl::None, "no-flow-control"),
        (57_600_u32, tokio_serial::FlowControl::Software, "xon-xoff"),
        (115_200_u32, tokio_serial::FlowControl::Hardware, "rts-cts"),
    ];

    let mut last_error = None;

    for (baud_rate, flow_control, label) in attempts {
        info!(serial_port = %serial_port, mode = %label, "opening EZSP serial transport");
        match try_open_ezsp_context(serial_port, baud_rate, flow_control, label, protocol_version).await {
            Ok(context) => return Ok(context),
            Err(error) => {
                warn!(serial_port = %serial_port, mode = %label, error = %error, "EZSP init attempt failed");
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::service_unavailable(format!("Unable to initialize EZSP on {serial_port}"))
    }))
}

async fn try_open_ezsp_context(
    serial_port: &str,
    baud_rate: u32,
    flow_control: tokio_serial::FlowControl,
    mode_label: &str,
    protocol_version: u8,
) -> Result<EzspContext, AppError> {
    let mut desired_version = NonZero::new(protocol_version).ok_or_else(|| {
        AppError::service_unavailable("ZIGBEE_EZSP_PROTOCOL_VERSION must be non-zero")
    })?;

    // Version negotiation is strict upstream: the NCP must report exactly the
    // requested protocol version. On mismatch (e.g. after a firmware upgrade)
    // we rebuild the pipeline once with the version the NCP announced, so the
    // driver adapts to the dongle without config changes.
    let (connection, callbacks_rx, tasks) = 'negotiate: {
        let mut last_error: Option<AppError> = None;

        for attempt in 0..2 {
            let stream = tokio_serial::new(serial_port, baud_rate)
                .flow_control(flow_control)
                .open_native_async()
                .map_err(|error| AppError::service_unavailable(format!(
                    "Unable to open Zigbee serial port {serial_port} in {mode_label} mode: {error}"
                )))?;

            let (reader, writer) = tokio::io::split(stream);
            let (payload_tx, payload_rx) = mpsc::channel::<Payload>(EZSP_CHANNEL_SIZE);
            let (ash_handle, ash_futures) = start_ash(reader, writer, payload_tx);
            let (client, ezsp_futures) = EzspClient::run(
                ash_handle,
                AshEzspReceiver::new(payload_rx),
                EZSP_CHANNEL_SIZE,
            );

            let tasks = PipelineTasks {
                ash_transmitter: tokio::spawn(ash_futures.transmitter),
                ash_receiver: tokio::spawn(ash_futures.receiver),
                ezsp_transmitter: tokio::spawn(ezsp_futures.transmitter),
                ezsp_receiver: tokio::spawn(ezsp_futures.receiver),
            };

            info!(
                serial_port = %serial_port,
                mode = %mode_label,
                protocol_version = desired_version.get(),
                "negotiating EZSP protocol version"
            );
            match timeout(EZSP_INIT_TIMEOUT, client.connect(desired_version)).await {
                Ok(Ok((connection, callbacks_rx))) => {
                    break 'negotiate (connection, callbacks_rx, tasks);
                }
                Ok(Err(error)) => {
                    tasks.shutdown().await;
                    if attempt == 0 {
                        if let ezsp::Error::ProtocolVersionMismatch { negotiated, .. } = &error {
                            let announced = NonZero::new(negotiated.protocol_version())
                                .filter(|version| *version != desired_version);
                            if let Some(version) = announced {
                                warn!(
                                    serial_port = %serial_port,
                                    desired = desired_version.get(),
                                    negotiated = version.get(),
                                    "NCP reports a different EZSP protocol version — retrying with it"
                                );
                                desired_version = version;
                                continue;
                            }
                        }
                    }
                    last_error = Some(AppError::service_unavailable(format!(
                        "Unable to initialize EZSP on {serial_port} in {mode_label} mode: {error}"
                    )));
                    break;
                }
                Err(_elapsed) => {
                    tasks.shutdown().await;
                    last_error = Some(AppError::service_unavailable(format!(
                        "Timed out initializing EZSP on {serial_port} in {mode_label} mode"
                    )));
                    break;
                }
            }
        }

        return Err(last_error.unwrap_or_else(|| {
            AppError::service_unavailable(format!(
                "Unable to initialize EZSP on {serial_port} in {mode_label} mode"
            ))
        }));
    };

    info!(serial_port = %serial_port, mode = %mode_label, "EZSP connection established");

    Ok(EzspContext {
        connection,
        tasks,
        callbacks_rx,
        joined_devices: Vec::new(),
        next_global_sequence: 1,
        next_device_sequence: HashMap::new(),
        last_activity: Instant::now(),
        coordinator_eui64: None,
        last_remote_power: None,
        last_remote_brightness_step: None,
        touchlink_found_networks: Vec::new(),
        touchlink_scan_in_progress: false,
        consecutive_errors: 0,
        availability_state: AvailabilityState::Idle,
        availability_queue: Vec::new(),
    })
}

async fn wait_for_driver_ready(lifecycle: &Arc<RwLock<DriverLifecycle>>) -> Result<(), AppError> {
    for _ in 0..30 {
        match &*lifecycle.read().await {
            DriverLifecycle::Ready => return Ok(()),
            DriverLifecycle::Failed(message) => {
                return Err(AppError::service_unavailable(message.clone()));
            }
            DriverLifecycle::Starting => tokio::time::sleep(StdDuration::from_millis(200)).await,
        }
    }

    Err(AppError::service_unavailable(
        "Native Zigbee adapter initialization timed out",
    ))
}

async fn wait_for_joined_network(network_state: &Arc<RwLock<DriverNetworkState>>) -> Result<(), AppError> {
    for _ in 0..30 {
        match *network_state.read().await {
            DriverNetworkState::Joined => return Ok(()),
            DriverNetworkState::NoNetwork | DriverNetworkState::Unknown => {
                tokio::time::sleep(StdDuration::from_millis(200)).await;
            }
        }
    }

    Err(AppError::service_unavailable(
        "Native Zigbee network did not report a joined state in time",
    ))
}

async fn ensure_coordinator_network(
    context: &mut EzspContext,
    serial_port: &str,
) -> Result<EmberNetworkStatus, AppError> {
    configure_local_endpoint(context).await?;
    configure_stack(context).await?;

    if let Err(error) = context.connection
        .network_init(NetworkInitBitmask::PARENT_INFO_IN_TOKEN)
        .await
    {
        warn!(serial_port = %serial_port, error = %error, "ezsp network_init failed");
    }

    let mut state = context.connection
        .network_state()
        .await
        .map_err(map_ezsp_error("read network state"))?;

    if state == EmberNetworkStatus::NoNetwork {
        info!(serial_port = %serial_port, "forming new Zigbee coordinator network");
        form_coordinator_network(context).await?;
        state = wait_for_network_ready(context).await?;
    } else {
        // The network already exists — try to refresh the security state so that
        // any flag changes take effect on the next join cycle.
        // This may fail with EmberInvalidCall on some firmware (security state
        // can only be set before the network is up) — that's OK, the runtime
        // policies set in configure_stack() are the ones that matter for join
        // behavior.
        info!(serial_port = %serial_port, "refreshing trust center security state on existing network");
        match ezsp::Security::set_initial_security_state(
            &mut context.connection,
            build_initial_security_state(),
        )
        .await
        {
            Ok(()) => info!("trust center security state refreshed successfully"),
            Err(error) => warn!(
                error = %error,
                "set_initial_security_state not accepted on running network (non-fatal — runtime policies still apply)"
            ),
        }
    }

    log_network_parameters(context, serial_port).await?;

    // Fetch and cache the coordinator's own EUI64 — needed for ZDO Bind_req.
    match context.connection.get_eui64().await {
        Ok(eui64) => {
            info!(eui64 = %format_eui64(eui64), "coordinator EUI64 cached");
            context.coordinator_eui64 = Some(eui64);
        }
        Err(error) => {
            warn!(error = %error, "failed to fetch coordinator EUI64 — remote binding will not work");
        }
    }

    Ok(state)
}

/// Tear down the EZSP pipeline: abort and join the four actor tasks and drop
/// the connection/serial handles. Bounded by construction (aborted tasks
/// resolve immediately), so no orphaned background tasks leak when we
/// reconnect and teardown can never hang the driver.
async fn teardown_context(context: EzspContext) {
    info!("tearing down EZSP pipeline");
    let EzspContext { connection, tasks, callbacks_rx, .. } = context;
    // Dropping the connection and callback receiver closes the actor
    // channels; aborting makes the teardown immediate regardless of state.
    drop(connection);
    drop(callbacks_rx);
    tasks.shutdown().await;
    info!("EZSP pipeline torn down");
}

/// Check whether the EZSP pipeline is healthy.  Returns a human-readable reason
/// if the pipeline should be torn down and rebuilt.
fn check_pipeline_health(context: &EzspContext) -> Option<&'static str> {
    if !context.tasks.is_alive() {
        return Some("EZSP pipeline task(s) died");
    }
    if context.last_activity.elapsed() > WATCHDOG_TIMEOUT {
        return Some("EZSP watchdog timeout — no activity");
    }
    None
}

async fn configure_local_endpoint(context: &mut EzspContext) -> Result<(), AppError> {
    context.connection
        .add_endpoint(
            DEFAULT_SOURCE_ENDPOINT,
            HOME_AUTOMATION_PROFILE_ID,
            DEFAULT_HOME_GATEWAY_DEVICE_ID,
            0,
            DEFAULT_LOCAL_INPUT_CLUSTERS.iter().copied().collect(),
            DEFAULT_LOCAL_OUTPUT_CLUSTERS.iter().copied().collect(),
        )
        .await
        .map_err(map_ezsp_error("add local endpoint"))
}

async fn configure_stack(context: &mut EzspContext) -> Result<(), AppError> {
    context.connection
        .set_configuration_value(config::Id::StackProfile, DEFAULT_STACK_PROFILE)
        .await
        .map_err(map_ezsp_error("set stack profile"))?;
    context.connection
        .set_configuration_value(config::Id::SecurityLevel, DEFAULT_SECURITY_LEVEL)
        .await
        .map_err(map_ezsp_error("set security level"))?;

    // Allow sleepy end devices (remotes, sensors) to join as children.
    // Default may be 0 on some firmware — explicitly set a reasonable limit.
    context.connection
        .set_configuration_value(config::Id::MaxEndDeviceChildren, 16)
        .await
        .map_err(map_ezsp_error("set max end device children"))?;

    // End device poll timeout: value 8 = 2^8 = 256 minutes (~4 hours).
    // Sleepy devices that don't poll within this window are removed from the
    // child table.  Remotes poll infrequently, so be generous.
    context.connection
        .set_configuration_value(config::Id::EndDevicePollTimeout, 8)
        .await
        .map_err(map_ezsp_error("set end device poll timeout"))?;

    // --- Trust Center policies ---
    // Allow new devices to join (ALLOW_JOINS) and accept unsecured rejoins
    // (ALLOW_UNSECURED_REJOINS).  The network key will be sent encrypted with
    // the joiner's link key (the well-known key imported into the transient
    // table at permit-join time).  This matches the Zigbee2MQTT Ember adapter
    // policy: bitmask 0x03.
    context.connection
        .set_policy(
            policy::Id::TrustCenter,
            (decision::Bitmask::ALLOW_JOINS | decision::Bitmask::ALLOW_UNSECURED_REJOINS).bits(),
        )
        .await
        .map_err(map_ezsp_error("set trust center policy"))?;

    // Allow devices to request the Trust Center link key (needed for Zigbee 3.0).
    context.connection
        .set_policy(
            policy::Id::TcKeyRequest,
            u8::from(decision::Id::AllowTcKeyRequestsAndSendCurrentKey),
        )
        .await
        .map_err(map_ezsp_error("set TC key request policy"))?;

    // Allow Trust Center rejoins using the well-known "ZigBeeAlliance09" key.
    // The Hue Dimmer v1 (and many Zigbee 3.0 devices) use this key after factory reset.
    // Value 0x01 = allow; timeout is controlled by TcRejoinsUsingWellKnownKeyTimeoutSec.
    context.connection
        .set_policy(policy::Id::TcJoinsUsingWellKnownKey, 0x01u8)
        .await
        .map_err(map_ezsp_error("set TC well-known key rejoin policy"))?;

    // Set the well-known key rejoin timeout to 600 seconds (10 minutes).
    context.connection
        .set_configuration_value(config::Id::TcRejoinsUsingWellKnownKeyTimeoutSec, 600)
        .await
        .map_err(map_ezsp_error("set TC well-known key rejoin timeout"))?;

    // Deny application key requests (matches Z2M: DENY_APP_KEY_REQUESTS).
    // Application link keys between devices are not needed for our use case.
    context.connection
        .set_policy(
            policy::Id::AppKeyRequest,
            u8::from(decision::Id::DenyAppKeyRequests),
        )
        .await
        .map_err(map_ezsp_error("set app key request policy (deny)"))?;

    // Set the transient key timeout to 300 seconds (5 minutes), matching Z2M.
    // This controls how long the NCP keeps a transient key entry before expiring it.
    let timeout_bytes: heapless::Vec<u8, 255, u8> = [0x2C, 0x01].into_iter().collect(); // 300 in LE u16
    context.connection
        .set_value(value::Id::TransientKeyTimeoutSec, timeout_bytes)
        .await
        .map_err(map_ezsp_error("set transient key timeout"))?;

    // Set extended security bitmask: JOINER_GLOBAL_LINK_KEY (0x0010) |
    // NWK_LEAVE_REQUEST_NOT_ALLOWED (0x0100) = 0x0110, matching Z2M.
    // - JOINER_GLOBAL_LINK_KEY: joiners use the global link key.
    // - NWK_LEAVE_REQUEST_NOT_ALLOWED: prevent rogue devices from forcing
    //   others off the network via NWK leave requests.
    let extended_bitmask: heapless::Vec<u8, 255, u8> = [0x10, 0x01].into_iter().collect(); // 0x0110 in LE u16
    context.connection
        .set_value(value::Id::ExtendedSecurityBitmask, extended_bitmask)
        .await
        .map_err(map_ezsp_error("set extended security bitmask"))?;

    context.connection
        .set_policy(
            policy::Id::MessageContentsInCallback,
            u8::from(decision::Id::MessageTagOnlyInCallback),
        )
        .await
        .map_err(map_ezsp_error("set message contents callback policy"))?;

    // Enable ZLL (Touchlink) message processing on the stack so that
    // Touchlink scan requests and responses are handled by the NCP.
    context.connection
        .set_policy(policy::Id::Zll, ZLL_POLICY_ENABLED)
        .await
        .map_err(map_ezsp_error("enable ZLL policy"))?;

    Ok(())
}

async fn form_coordinator_network(context: &mut EzspContext) -> Result<(), AppError> {
    let coordinator_eui64 = context.connection
        .get_eui64()
        .await
        .map_err(map_ezsp_error("get coordinator EUI64"))?;
    let channel = configured_network_channel();
    let tx_power = configured_network_tx_power();
    let pan_id = match configured_pan_id() {
        Some(value) => value,
        None => random_pan_id(
            context.connection
                .get_random_number()
                .await
                .map_err(map_ezsp_error("generate PAN ID"))?,
        ),
    };
    let extended_pan_id = configured_extended_pan_id().unwrap_or_else(|| derive_extended_pan_id(coordinator_eui64));

    info!(
        pan_id = format_args!("0x{pan_id:04x}"),
        extended_pan_id = %extended_pan_id,
        channel,
        tx_power,
        "forming Zigbee coordinator network"
    );

    ezsp::Security::set_initial_security_state(
        &mut context.connection,
        build_initial_security_state(),
    )
    .await
    .map_err(map_ezsp_error("set initial security state"))?;

    context.connection
        .form_network(EmberNetworkParameters::new(
            extended_pan_id,
            pan_id,
            tx_power,
            channel,
            EmberJoinMethod::MacAssociation,
            0,
            0,
            1_u32 << channel,
        ))
        .await
        .map_err(map_ezsp_error("form coordinator network"))?;

    Ok(())
}

async fn log_network_parameters(context: &mut EzspContext, serial_port: &str) -> Result<(), AppError> {
    let (node_type, parameters) = context.connection
        .get_network_parameters()
        .await
        .map_err(map_ezsp_error("get network parameters"))?;
    info!(
        serial_port = %serial_port,
        node_type = %node_type,
        pan_id = format_args!("0x{:04x}", parameters.pan_id()),
        extended_pan_id = %parameters.extended_pan_id(),
        channel = parameters.radio_channel(),
        tx_power = parameters.radio_tx_power(),
        join_method = ?parameters.join_method(),
        "native zigbee network parameters"
    );
    Ok(())
}

async fn wait_for_network_ready(context: &mut EzspContext) -> Result<EmberNetworkStatus, AppError> {
    for _ in 0..25 {
        while let Ok(callback) = context.callbacks_rx.try_recv() {
            let _ = handle_callback(context, callback).await;
        }

        let state = context.connection
            .network_state()
            .await
            .map_err(map_ezsp_error("read network state after form"))?;
        if state != EmberNetworkStatus::NoNetwork && state != EmberNetworkStatus::JoiningNetwork {
            return Ok(state);
        }

        tokio::time::sleep(StdDuration::from_millis(200)).await;
    }

    Err(AppError::service_unavailable(
        "Timed out waiting for the Zigbee coordinator network to come up",
    ))
}

fn build_initial_security_state() -> initial::State {
    // Matches Z2M's Ember adapter initial security state:
    //   TRUST_CENTER_GLOBAL_LINK_KEY | HAVE_PRECONFIGURED_KEY | HAVE_NETWORK_KEY
    //   | TRUST_CENTER_USES_HASHED_LINK_KEY | REQUIRE_ENCRYPTED_KEY
    //
    // REQUIRE_ENCRYPTED_KEY tells the NCP to only deliver the network key to
    // joiners that can prove they possess the link key (via the transient key
    // imported at permit-join time).  Without it, the NCP might send the
    // network key in the clear to devices that don't have a link key.
    //
    // TRUST_CENTER_USES_HASHED_LINK_KEY tells the NCP the preconfigured key is
    // a "hashed" global link key (the well-known ZigBeeAlliance09).
    initial::State::new(
        initial::Bitmask::TRUST_CENTER_GLOBAL_LINK_KEY
            | initial::Bitmask::HAVE_PRECONFIGURED_KEY
            | initial::Bitmask::HAVE_NETWORK_KEY
            | initial::Bitmask::TRUST_CENTER_USES_HASHED_LINK_KEY
            | initial::Bitmask::REQUIRE_ENCRYPTED_KEY,
        ZIGBEE_ALLIANCE09_LINK_KEY,
        [0; 16],
        0,
        Eui64::default(),
    )
}

fn configured_network_channel() -> u8 {
    parse_u8_env("ZIGBEE_CHANNEL")
        .filter(|value| (11..=26).contains(value))
        .unwrap_or(DEFAULT_NETWORK_CHANNEL)
}

fn configured_network_tx_power() -> i8 {
    let Ok(raw) = std::env::var("ZIGBEE_TX_POWER") else {
        return DEFAULT_NETWORK_TX_POWER;
    };
    // Radio power is signed dBm; negative values are legitimate.
    raw.trim().parse::<i8>().unwrap_or_else(|_| {
        warn!(
            value = %raw,
            default = DEFAULT_NETWORK_TX_POWER,
            "invalid ZIGBEE_TX_POWER (expected dBm in -128..=127), using default"
        );
        DEFAULT_NETWORK_TX_POWER
    })
}

fn configured_pan_id() -> Option<u16> {
    parse_u16_env("ZIGBEE_PAN_ID").map(random_pan_id)
}

fn configured_extended_pan_id() -> Option<Eui64> {
    let value = std::env::var("ZIGBEE_EXTENDED_PAN_ID").ok()?;
    let compact = value.chars().filter(char::is_ascii_hexdigit).collect::<String>();
    if compact.len() != 16 {
        return None;
    }

    let mut bytes = [0_u8; 8];
    for (index, chunk_start) in (0..16).step_by(2).enumerate() {
        bytes[index] = u8::from_str_radix(&compact[chunk_start..chunk_start + 2], 16).ok()?;
    }

    Some(Eui64::from(bytes))
}

fn derive_extended_pan_id(coordinator_eui64: Eui64) -> Eui64 {
    let mut bytes = coordinator_eui64.into_array();
    bytes[0] ^= 0x02;
    Eui64::from(bytes)
}

fn random_pan_id(value: u16) -> u16 {
    match value {
        0x0000 | 0xffff => 0x1a62,
        other => other,
    }
}

fn parse_u8_env(name: &str) -> Option<u8> {
    let value = std::env::var(name).ok()?;
    parse_u16_literal(&value).ok()?.try_into().ok()
}

fn parse_u16_env(name: &str) -> Option<u16> {
    let value = std::env::var(name).ok()?;
    parse_u16_literal(&value).ok()
}

fn parse_u16_literal(value: &str) -> Result<u16, std::num::ParseIntError> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u16>()
    }
}

async fn handle_command(context: &mut EzspContext, command: NativeZigbeeCommand) -> Result<(), AppError> {
    match command {
        NativeZigbeeCommand::PermitJoin { seconds } => {
            if seconds > 0 {
                // Import the well-known "ZigBeeAlliance09" link key into the transient
                // key table with a wildcard EUI64 (all 0xFF) BEFORE opening the network.
                // This tells the NCP: "any new device joining can use this key to
                // decrypt the network key transport."
                // Wildcard EUI64 = all 0xFF — matches ANY joining device.
                // Z2M uses 0xFFFFFFFFFFFFFFFF; all-zeros does NOT work as a wildcard.
                let blank_eui64 = Eui64::new(0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF);
                let key: security_man::Key = ZIGBEE_ALLIANCE09_LINK_KEY;

                match context.connection
                    .import_transient_key(blank_eui64, key, security_man::Flags::NONE)
                    .await
                {
                    Ok(()) => {
                        info!("transient key (ZigBeeAlliance09) imported for all joining devices (wildcard EUI64)");
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to import transient key — new devices may not be able to join");
                    }
                }

                // Set TC policy to allow new joins + unsecured rejoins (matching Z2M's
                // `emberSetJoinPolicy(USE_PRECONFIGURED_KEY)` = bitmask 0x03).
                match context.connection
                    .set_policy(
                        policy::Id::TrustCenter,
                        (decision::Bitmask::ALLOW_JOINS | decision::Bitmask::ALLOW_UNSECURED_REJOINS).bits(),
                    )
                    .await
                {
                    Ok(()) => {
                        info!("trust center policy set to ALLOW_JOINS | ALLOW_UNSECURED_REJOINS");
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to update trust center policy for joining");
                    }
                }
            } else {
                // Closing the network: clear all transient keys so no new devices can
                // join (they would need a matching key in the transient table).
                match context.connection.clear_transient_link_keys().await {
                    Ok(()) => {
                        info!("transient link keys cleared (network closed)");
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to clear transient link keys");
                    }
                }

                // Restrict TC policy to unsecured rejoins only (no new joins).
                match context.connection
                    .set_policy(
                        policy::Id::TrustCenter,
                        decision::Bitmask::ALLOW_UNSECURED_REJOINS.bits(),
                    )
                    .await
                {
                    Ok(()) => {
                        info!("trust center policy set to ALLOW_UNSECURED_REJOINS only (network closed)");
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to restrict trust center policy after closing");
                    }
                }
            }

            let duration = if seconds == 0 {
                NetworkDuration::Disable
            } else {
                NetworkDuration::try_from(StdDuration::from_secs(u64::from(seconds)))
                    .map_err(|error| AppError::service_unavailable(format!(
                        "Invalid permit-join duration {seconds}: {error}"
                    )))?
            };
            context.connection
                .permit_joining(duration)
                .await
                .map_err(map_ezsp_error("permit join"))?;
            info!(seconds, "native zigbee permit join updated");
            Ok(())
        }
        NativeZigbeeCommand::DiscoverDevices => {
            let targets = context.joined_devices.clone();
            info!(known_devices = targets.len(), "native zigbee discovery requested");
            if targets.is_empty() {
                debug!("native zigbee discovery skipped because no joined devices are known yet");
            }
            // NOTE: we intentionally do NOT wrap these EZSP calls in a timeout.
            // The EZSP UART protocol is strictly request-response: dropping a
            // `communicate()` future mid-`receive()` (as `timeout()` does) orphans
            // the NCP's response in the pipeline, permanently desynchronising all
            // subsequent commands.  If the NCP is slow, we wait.
            for target in targets {
                info!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, endpoint = ?target.endpoint, "probing known Zigbee device");
                if should_probe_active_endpoints(&target) {
                    match request_active_endpoints(context, target.node_id).await {
                        Ok(()) => {}
                        Err(error) => {
                            warn!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, error = %error, "native zigbee active endpoint probe failed");
                            continue;
                        }
                    }
                }
                if target.endpoint.is_some() {
                    if let Err(error) = refresh_device_state(context, &target).await {
                        warn!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, error = %error, "native zigbee state refresh failed during discovery");
                    }
                }
            }
            Ok(())
        }
        NativeZigbeeCommand::SetPower { lamp_id, enabled } => {
            let target = find_target_device(context, &lamp_id)?;

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            if !target.input_clusters.contains(&ON_OFF_CLUSTER_ID) {
                return Err(AppError::service_unavailable(format!(
                    "Lamp {lamp_id} does not expose the On/Off cluster yet"
                )));
            }

            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                ON_OFF_CLUSTER_ID,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_on_off_command_payload(enabled, sequence);

            context.connection
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast on/off"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.is_on = enabled;
            }

            Ok(())
        }
        NativeZigbeeCommand::SetBrightness { lamp_id, brightness } => {
            let target = find_target_device(context, &lamp_id)?;

            if !target.supports_brightness {
                return Err(AppError::service_unavailable(format!(
                    "Native brightness is not available for {lamp_id}; the lamp capabilities are incomplete"
                )));
            }

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_brightness_command_payload(brightness, sequence);
            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                0x0008,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );

            context.connection
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast brightness"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.brightness = brightness.min(100);
                device.is_on = brightness > 0;
                device.desired_brightness = Some(brightness.min(100));
            }

            Ok(())
        }
        NativeZigbeeCommand::SetTemperature { lamp_id, temperature } => {
            let target = find_target_device(context, &lamp_id)?;

            if !target.supports_temperature {
                return Err(AppError::service_unavailable(format!(
                    "Native color temperature is not available for {lamp_id}; the lamp capabilities are incomplete"
                )));
            }

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_color_temperature_command_payload(temperature, sequence);
            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                0x0300,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );

            context.connection
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast color temperature"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.temperature = Some(temperature.min(100));
                device.desired_temperature = Some(temperature.min(100));
                device.color_mode = Some(2); // Color Temperature mode
                // Clear desired color — the lamp is now in CT mode, so
                // re-sending a stale XY color on the next restore cycle would
                // revert it back to XY mode.
                device.desired_color_x = None;
                device.desired_color_y = None;
            }

            Ok(())
        }
        NativeZigbeeCommand::SetColor { lamp_id, x, y } => {
            let target = find_target_device(context, &lamp_id)?;

            if !target.supports_xy_color {
                return Err(AppError::service_unavailable(format!(
                    "Native color control is not available for {lamp_id}; the lamp does not support XY color"
                )));
            }

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_color_xy_command_payload(x, y, sequence);
            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                COLOR_CONTROL_CLUSTER_ID,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );

            context.connection
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast color xy"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.color_x = Some(x.clamp(0.0, 1.0));
                device.color_y = Some(y.clamp(0.0, 1.0));
                device.color_mode = Some(1); // XY mode
                device.desired_color_x = Some(x.clamp(0.0, 1.0));
                device.desired_color_y = Some(y.clamp(0.0, 1.0));
                // Clear desired temperature — the lamp is now in XY color
                // mode, so re-sending a stale temperature on the next restore
                // cycle would revert it back to CT mode.
                device.desired_temperature = None;
            }

            Ok(())
        }
        NativeZigbeeCommand::SetEffect { lamp_id, effect } => {
            let target = find_target_device(context, &lamp_id)?;

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;

            // Determine which cluster and payload to use based on the effect name.
            // Standard Identify cluster effects:
            //   blink, breathe, okay, channel_change, finish_effect, stop_effect
            // Philips Hue proprietary effects (cluster 0xFC03):
            //   candle, fireplace, colorloop, sunrise, sparkle, opal, glisten, stop_hue_effect
            let effect_lower = effect.to_ascii_lowercase();
            match effect_lower.as_str() {
                "blink" | "breathe" | "okay" | "channel_change" | "finish_effect" | "stop_effect" => {
                    let effect_id: u8 = match effect_lower.as_str() {
                        "blink" => 0x00,
                        "breathe" => 0x01,
                        "okay" => 0x02,
                        "channel_change" => 0x0B,
                        "finish_effect" => 0xFE,
                        "stop_effect" => 0xFF,
                        _ => unreachable!(),
                    };
                    let sequence = next_device_sequence(context, target.node_id);
                    let zcl_payload = build_identify_trigger_effect_payload(effect_id, sequence);
                    let aps_frame = EzspApsFrame::new(
                        HOME_AUTOMATION_PROFILE_ID,
                        IDENTIFY_CLUSTER_ID,
                        DEFAULT_SOURCE_ENDPOINT,
                        endpoint,
                        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                        0,
                        0,
                    );

                    context.connection
                        .send_unicast(
                            Destination::Direct(NodeId::from(target.node_id)),
                            aps_frame,
                            0,
                            zcl_payload.into_iter().collect(),
                        )
                        .await
                        .map(|_| ())
                        .map_err(map_ezsp_error("send unicast identify effect"))?;
                }
                "candle" | "fireplace" | "colorloop" | "sunrise" | "sparkle" | "opal" | "glisten" => {
                    let hue_effect_id: u8 = match effect_lower.as_str() {
                        "candle" => 0x01,
                        "fireplace" => 0x02,
                        "colorloop" => 0x03,
                        "sunrise" => 0x09,
                        "sparkle" => 0x0A,
                        "opal" => 0x0B,
                        "glisten" => 0x0C,
                        _ => unreachable!(),
                    };
                    let start_payload: &[u8] = &[0x21, 0x00, 0x01, hue_effect_id];
                    let sequence = next_device_sequence(context, target.node_id);
                    let zcl_payload = build_philips_hue_effect_payload(start_payload, sequence);
                    let aps_frame = EzspApsFrame::new(
                        HOME_AUTOMATION_PROFILE_ID,
                        PHILIPS_MANU_SPECIFIC_CLUSTER_ID,
                        DEFAULT_SOURCE_ENDPOINT,
                        endpoint,
                        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                        0,
                        0,
                    );

                    context.connection
                        .send_unicast(
                            Destination::Direct(NodeId::from(target.node_id)),
                            aps_frame,
                            0,
                            zcl_payload.into_iter().collect(),
                        )
                        .await
                        .map(|_| ())
                        .map_err(map_ezsp_error("send unicast hue effect"))?;
                }
                "stop_hue_effect" => {
                    let stop_payload: &[u8] = &[0x20, 0x00, 0x00];
                    let sequence = next_device_sequence(context, target.node_id);
                    let zcl_payload = build_philips_hue_effect_payload(stop_payload, sequence);
                    let aps_frame = EzspApsFrame::new(
                        HOME_AUTOMATION_PROFILE_ID,
                        PHILIPS_MANU_SPECIFIC_CLUSTER_ID,
                        DEFAULT_SOURCE_ENDPOINT,
                        endpoint,
                        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                        0,
                        0,
                    );

                    context.connection
                        .send_unicast(
                            Destination::Direct(NodeId::from(target.node_id)),
                            aps_frame,
                            0,
                            zcl_payload.into_iter().collect(),
                        )
                        .await
                        .map(|_| ())
                        .map_err(map_ezsp_error("send unicast stop hue effect"))?;
                }
                _ => {
                    return Err(AppError::http(
                        StatusCode::BAD_REQUEST,
                        format!("Unknown effect: {effect}. Supported: blink, breathe, okay, channel_change, finish_effect, stop_effect, candle, fireplace, colorloop, sunrise, sparkle, opal, glisten, stop_hue_effect"),
                    ));
                }
            }

            Ok(())
        }
        NativeZigbeeCommand::TouchlinkScan => {
            touchlink_scan(context).await
        }
    }
}

/// Perform a Touchlink (ZLL) scan and commission any found devices onto the
/// current Zigbee network.
///
/// Touchlink is required for factory-new ZLL devices (e.g. Hue Lightstrip Plus)
/// that do not respond to standard NWK-level `permit_join`.  The flow is:
///
/// 1. Set ZLL initial security state (master key + key index).
/// 2. Start a Touchlink scan on the primary ZLL channels.
/// 3. Wait for `NetworkFound` callbacks (one per discovered ZLL device).
/// 4. Wait for `ScanComplete` callback.
/// 5. For each found device, issue `network_ops(JoinTarget)` to tell it to
///    join our network.  The device will then appear via the normal
///    `TrustCenterJoin` / `ChildJoin` path.
async fn touchlink_scan(context: &mut EzspContext) -> Result<(), AppError> {
    info!("touchlink: starting ZLL Touchlink scan");

    // --- 1. Configure ZLL security state ---
    // We use the ZLL Master key (KeyIndex::Master = 0x04).
    // The encryption_key is the ZLL master key that will be used by the NCP
    // to encrypt the network key transport during commissioning.
    // The preconfigured_key is left zeroed — not needed for initiator mode.
    //
    // NOTE: We use `set_security_state_without_key` instead of
    // `set_initial_security_state` because the latter can only be called
    // *before* the network is formed, and our coordinator already has a
    // running network.  `set_security_state_without_key` updates the ZLL
    // security token on an established network without requiring a separate
    // network_key argument (the NCP already knows its own network key).
    let zll_security = ZllInitialSecurityState::new(
        ZLL_SECURITY_KEY_BITMASK,
        zll::KeyIndex::Master,
        ZLL_MASTER_KEY,
        [0u8; 16], // preconfigured_key — unused for initiator
    );

    ezsp::Zll::set_security_state_without_key(
        &mut context.connection,
        zll_security,
    )
    .await
    .map_err(map_ezsp_error("set ZLL security state"))?;
    info!("touchlink: ZLL security state configured (master key)");

    // --- 2. Set the channel mask for the scan ---
    context.connection
        .set_primary_channel_mask(ZLL_PRIMARY_CHANNEL_MASK)
        .await
        .map_err(map_ezsp_error("set ZLL primary channel mask"))?;
    info!(
        channel_mask = format_args!("0x{ZLL_PRIMARY_CHANNEL_MASK:08x}"),
        "touchlink: primary channel mask set"
    );

    // --- 3. Scan: first primary ZLL channels, then all channels if nothing found ---
    // Use maximum TX power (20 dBm) for Touchlink — the protocol relies on
    // RSSI-based proximity detection so we want the strongest possible signal.
    const TOUCHLINK_TX_POWER: i8 = 20;

    context.touchlink_found_networks.clear();
    context.touchlink_scan_in_progress = true;

    let channel_passes: &[(&str, u32)] = &[
        ("primary ZLL (11,15,20,25)", ZLL_PRIMARY_CHANNEL_MASK),
        ("all Zigbee (11-26)", ZLL_ALL_CHANNELS_MASK),
    ];

    for (label, channel_mask) in channel_passes {
        context.touchlink_found_networks.clear();
        context.touchlink_scan_in_progress = true;

        info!(
            channels = label,
            channel_mask = format_args!("0x{channel_mask:08x}"),
            tx_power = TOUCHLINK_TX_POWER,
            "touchlink: starting scan pass"
        );

        ezsp::Zll::start_scan(
            &mut context.connection,
            *channel_mask,
            TOUCHLINK_TX_POWER,
            EmberNodeType::Coordinator,
        )
        .await
        .map_err(map_ezsp_error("start ZLL Touchlink scan"))?;

        // Wait for ScanComplete callback or timeout
        let scan_deadline = Instant::now() + TOUCHLINK_SCAN_TIMEOUT;
        while context.touchlink_scan_in_progress {
            tokio::select! {
                Some(callback) = context.callbacks_rx.recv() => {
                    context.last_activity = Instant::now();
                    let _ = handle_callback(context, callback).await;
                }
                _ = tokio::time::sleep_until(scan_deadline) => { break; }
            }
        }

        if context.touchlink_scan_in_progress {
            warn!(channels = label, "touchlink: scan timed out after {:?}", TOUCHLINK_SCAN_TIMEOUT);
            context.touchlink_scan_in_progress = false;
        }

        let found_count = context.touchlink_found_networks.len();
        info!(found_count, channels = label, "touchlink: scan pass complete");

        if found_count > 0 {
            break; // Found devices, skip remaining passes
        }
    }

    let found_count = context.touchlink_found_networks.len();
    if found_count == 0 {
        info!("touchlink: no ZLL devices found on any channel");
        return Ok(());
    }

    // --- 4. Commission each found device: tell it to join our network ---
    let found_networks = context.touchlink_found_networks.drain(..).collect::<Vec<_>>();
    for (i, found) in found_networks.iter().enumerate() {
        let eui64 = found.network_info.eui64();
        info!(
            index = i,
            eui64 = %format_eui64(eui64),
            rssi = found.rssi,
            device_id = ?found.device_id,
            endpoint = ?found.device_endpoint,
            "touchlink: commissioning ZLL device (JoinTarget)"
        );

        match context.connection
            .network_ops(found.network_info.clone(), ZllNetworkOperation::JoinTarget, TOUCHLINK_TX_POWER)
            .await
        {
            Ok(()) => {
                info!(
                    eui64 = %format_eui64(eui64),
                    "touchlink: JoinTarget command sent successfully — device should join the network"
                );
                // Give the device time to process the join and send its
                // TrustCenterJoin / ChildJoin / Device Announce.
                tokio::time::sleep(StdDuration::from_secs(2)).await;
                // Drain any join-related callbacks that arrived.
                while let Ok(callback) = context.callbacks_rx.try_recv() {
                    context.last_activity = Instant::now();
                    let _ = handle_callback(context, callback).await;
                }
            }
            Err(error) => {
                warn!(
                    eui64 = %format_eui64(eui64),
                    error = %error,
                    "touchlink: JoinTarget failed for device"
                );
            }
        }
    }

    info!(commissioned = found_networks.len(), "touchlink: commissioning complete");
    Ok(())
}

fn find_target_device(context: &EzspContext, lamp_id: &str) -> Result<DiscoveredDevice, AppError> {
    let normalized_lamp_id = normalize_device_id(lamp_id, 0);
    context
        .joined_devices
        .iter()
        .find(|device| {
            device.eui64 == lamp_id
                || normalize_device_id(&device.eui64, device.node_id) == normalized_lamp_id
                || format!("{:016x}", device.node_id) == normalized_lamp_id
        })
        .cloned()
        .ok_or_else(|| AppError::service_unavailable(format!(
            "No discovered native Zigbee lamp matches {lamp_id}; discovery is not implemented yet"
        )))
}

async fn refresh_device_state(context: &mut EzspContext, target: &DiscoveredDevice) -> Result<(), AppError> {
    let endpoint = target
        .endpoint
        .ok_or_else(|| AppError::service_unavailable(format!(
            "Lamp {} has no discovered endpoint yet; run discovery first",
            target.eui64
        )))?;

    send_read_attributes(context, target.node_id, endpoint, BASIC_CLUSTER_ID, &[0x0004, 0x0005]).await?;

    if target.input_clusters.contains(&ON_OFF_CLUSTER_ID) {
        send_read_attributes(context, target.node_id, endpoint, ON_OFF_CLUSTER_ID, &[0x0000]).await?;
    }
    if target.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID) {
        send_read_attributes(context, target.node_id, endpoint, LEVEL_CONTROL_CLUSTER_ID, &[0x0000]).await?;
    }
    if target.has_color_control_cluster {
        send_read_attributes(context, target.node_id, endpoint, COLOR_CONTROL_CLUSTER_ID, &[0x0003, 0x0004, 0x0007, 0x0008, 0x400A]).await?;
    }

    Ok(())
}

async fn send_read_attributes(
    context: &mut EzspContext,
    node_id: u16,
    endpoint: u8,
    cluster_id: u16,
    attributes: &[u16],
) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let mut payload = vec![ZCL_GLOBAL_FRAME_CONTROL, sequence, ZCL_READ_ATTRIBUTES_COMMAND_ID];
    for attribute in attributes {
        payload.push((attribute & 0xff) as u8);
        payload.push((attribute >> 8) as u8);
    }
    let aps_frame = EzspApsFrame::new(
        HOME_AUTOMATION_PROFILE_ID,
        cluster_id,
        DEFAULT_SOURCE_ENDPOINT,
        endpoint,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context.connection
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send ZCL read attributes"))
}

async fn handle_callback(context: &mut EzspContext, callback: Callback) -> Option<NativeZigbeeEvent> {
    match callback {
        Callback::Networking(handler) => match handler {
            parameters::networking::handler::Handler::StackStatus(status) => {
                Some(NativeZigbeeEvent::NetworkState {
                    status: status
                        .result()
                        .map(network_state_from_stack_status)
                        .unwrap_or_else(|value| format!("unknown-stack-status-{value:#04x}")),
                })
            }
            parameters::networking::handler::Handler::ChildJoin(join) => {
                let eui64 = format_eui64(join.child_eui64());
                let node_id: u16 = join.child_id().into();
                let child_type = join.child_type();
                info!(
                    node_id = format_args!("0x{node_id:04x}"),
                    eui64 = %eui64,
                    joining = join.joining(),
                    child_type = ?child_type,
                    "ChildJoin callback received"
                );

                if join.joining() {
                    let is_sleepy = child_type == Ok(EmberNodeType::SleepyEndDevice);

                    ensure_joined_device(context, node_id, eui64.clone());

                    if is_sleepy {
                        // Sleepy end devices are typically remotes/sensors that
                        // never respond to ZDO requests.  However, some ZLL
                        // devices (e.g. Hue Lightstrip after Touchlink) may
                        // report as SleepyEndDevice while actually being a lamp.
                        //
                        // Strategy: still attempt ZDO discovery first.  If the
                        // device does not respond, the intercept-on-wake logic
                        // in handle_incoming_cluster will auto-classify it as
                        // Remote when it sends its first ZCL command.
                        let already_classified = context.joined_devices.iter()
                            .any(|d| d.node_id == node_id && d.interview_completed);
                        if already_classified {
                            debug!(
                                node_id = format_args!("0x{node_id:04x}"),
                                eui64 = %eui64,
                                "sleepy end device already interviewed — skipping re-discovery"
                            );
                        } else {
                            info!(
                                node_id = format_args!("0x{node_id:04x}"),
                                eui64 = %eui64,
                                "sleepy end device joined — attempting ZDO discovery before classifying"
                            );
                            request_known_device_discovery(context, node_id).await;
                        }
                    } else {
                        request_known_device_discovery(context, node_id).await;
                    }

                    Some(NativeZigbeeEvent::DeviceJoined {
                        node_id,
                        eui64,
                    })
                } else {
                    None
                }
            }
            _ => None,
        },
        Callback::TrustCenter(handler) => match handler {
            parameters::trust_center::handler::Handler::TrustCenterJoin(join) => {
                handle_trust_center_join(context, *join).await
            }
        },
        Callback::Messaging(handler) => match handler {
            parameters::messaging::handler::Handler::IncomingMessage(message) => {
                let node_id: u16 = message.sender().into();
                let cluster_id = message.aps_frame().cluster_id();
                let profile_id = message.aps_frame().profile_id();
                let payload = message.message().to_vec();
                handle_incoming_cluster(context, node_id, cluster_id, profile_id, &payload).await.or(Some(NativeZigbeeEvent::IncomingMessage {
                    node_id,
                    cluster_id,
                    payload,
                }))
            }
            parameters::messaging::handler::Handler::MessageSent(sent) => {
                let node_id = sent.index_or_destination();
                match sent.ack_received() {
                    Ok(false) => {
                        // DeliveryFailed — the NCP exhausted MAC-level retries
                        // without receiving an ACK.  Mark the device unreachable
                        // immediately so the frontend gets fast feedback.
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            if device.reachable {
                                info!(
                                    node_id = format_args!("0x{node_id:04x}"),
                                    eui64 = %device.eui64,
                                    "device marked unreachable after MAC delivery failure"
                                );
                                device.reachable = false;
                                device.desired_state_applied = false;
                            }
                        }
                        Some(NativeZigbeeEvent::DeliveryFailed { node_id })
                    }
                    _ => None,
                }
            }
            _ => None,
        },
        Callback::Zll(handler) => {
            handle_zll_callback(context, handler);
            None
        }
        _ => None,
    }
}

/// Handle ZLL (Touchlink) callbacks from the EZSP stack.
fn handle_zll_callback(context: &mut EzspContext, handler: parameters::zll::handler::Handler) {
    match handler {
        parameters::zll::handler::Handler::NetworkFound(found) => {
            let network = found.network_info();
            let eui64 = network.eui64();
            let rssi = found.last_hop_rssi();
            let device_info = found.device_info();

            info!(
                eui64 = %format_eui64(eui64),
                rssi,
                node_id = format_args!("0x{:04x}", u16::from(network.node_id())),
                number_sub_devices = network.number_sub_devices(),
                "touchlink: ZLL device found during scan"
            );

            let (device_endpoint, device_id) =
                if let Some(info) = device_info {
                    info!(
                        endpoint = info.endpoint_id(),
                        profile_id = format_args!("0x{:04x}", info.profile_id()),
                        device_id = format_args!("0x{:04x}", info.device_id()),
                        device_eui64 = %format_eui64(info.ieee_address()),
                        "touchlink: device info available"
                    );
                    (
                        Some(info.endpoint_id()),
                        Some(info.device_id()),
                    )
                } else {
                    debug!("touchlink: no device info in NetworkFound callback");
                    (None, None)
                };

            context.touchlink_found_networks.push(TouchlinkFoundNetwork {
                network_info: network.clone(),
                device_endpoint,
                device_id,
                rssi,
            });
        }
        parameters::zll::handler::Handler::ScanComplete(scan) => {
            let status = scan.result();
            info!(?status, found_count = context.touchlink_found_networks.len(), "touchlink: scan complete");
            context.touchlink_scan_in_progress = false;
        }
        parameters::zll::handler::Handler::AddressAssignment(assignment) => {
            let addr = assignment.address_info();
            info!(
                node_id = format_args!("0x{:04x}", u16::from(addr.node_id_())),
                "touchlink: address assignment received"
            );
        }
        parameters::zll::handler::Handler::TouchLinkTarget(target) => {
            let network = target.network_info();
            info!(
                eui64 = %format_eui64(network.eui64()),
                "touchlink: TouchLinkTarget callback (we are being touchlinked by another device)"
            );
        }
    }
}

async fn handle_trust_center_join(
    context: &mut EzspContext,
    join: parameters::trust_center::handler::TrustCenterJoin,
) -> Option<NativeZigbeeEvent> {
    let status = join.status().ok()?;
    let node_id: u16 = join.new_node_id().into();
    let eui64 = format_eui64(join.new_node_eui64());

    match status {
        EmberDeviceUpdate::StandardSecuritySecuredRejoin
        | EmberDeviceUpdate::StandardSecurityUnsecuredJoin
        | EmberDeviceUpdate::StandardSecurityUnsecuredRejoin => {
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                eui64 = %eui64,
                join_status = ?status,
                "TrustCenterJoin received"
            );
            ensure_joined_device(context, node_id, eui64.clone());

            // NOTE: We intentionally do NOT classify devices by OUI here.
            // The ChildJoin callback (which fires separately) already handles
            // the sleepy-vs-router distinction correctly.  Classifying all
            // Philips devices as remotes here would prevent Philips lamps
            // (Lightstrip Plus, etc.) from being discovered.
            //
            // For non-sleepy devices (lamps/routers), ChildJoin triggers ZDO
            // discovery.  For sleepy devices (remotes), ChildJoin classifies
            // them as Remote immediately.  TrustCenterJoin just ensures the
            // device is in the joined_devices list.
            request_known_device_discovery(context, node_id).await;

            Some(NativeZigbeeEvent::DeviceJoined { node_id, eui64 })
        }
        EmberDeviceUpdate::DeviceLeft => {
            // Use EUI64 as the primary identifier for removal — node_ids can
            // be reassigned to different devices after a rejoin.
            context.joined_devices.retain(|device| device.eui64 != eui64);
            context.next_device_sequence.remove(&node_id);
            None
        }
    }
}

fn ensure_joined_device(context: &mut EzspContext, node_id: u16, eui64: String) {
    // Prefer matching on EUI64 (stable hardware identifier).  Only fall back
    // to node_id if no EUI64 match exists — node_ids can be reassigned after
    // a device rejoin.
    let idx = context
        .joined_devices
        .iter()
        .position(|device| device.eui64 == eui64)
        .or_else(|| {
            context
                .joined_devices
                .iter()
                .position(|device| device.node_id == node_id)
        });
    if let Some(device) = idx.map(|i| &mut context.joined_devices[i]) {
        let was_reachable = device.reachable;
        device.node_id = node_id;
        device.eui64 = eui64;
        device.connected = true;
        device.reachable = true;
        device.last_seen = Some(Instant::now());
        device.failed_ping_cycles = 0;
        if was_reachable {
            // The device was already reachable — this is an adapter self-healing
            // reconnect, not a physical power cycle.  Keep the previous is_on state
            // so that lamps that were off stay off.
        } else {
            // The device was unreachable (wall-switch / physical power cycle) —
            // the lamp has physically powered back on, so reflect that.
            device.is_on = true;
            if device.brightness == 0 {
                device.brightness = 100;
            }
        }
        if device.endpoint.is_none() {
            device.interview_attempts = 0;
        }
    } else {
        info!(
            node_id = format_args!("0x{node_id:04x}"),
            eui64 = %eui64,
            "new device added to joined list — awaiting interview"
        );
        context.joined_devices.push(DiscoveredDevice {
            node_id,
            eui64,
            endpoint: None,
            input_clusters: Vec::new(),
            output_clusters: Vec::new(),
            device_type: ZigbeeDeviceType::Unknown,
            supports_brightness: false,
            supports_temperature: false,
            has_color_control_cluster: false,
            supports_xy_color: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            color_x: None,
            color_y: None,
            color_mode: None,
            interview_completed: false,
            model: None,
            manufacturer: None,
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: Some(Instant::now()),
            desired_brightness: None,
            desired_temperature: None,
            desired_color_x: None,
            desired_color_y: None,
            desired_state_applied: true,
            last_discovery_at: None,
            failed_ping_cycles: 0,
        });
    }
}

/// Compute the exponential-backoff multiplier for a device's availability timeout.
///
/// Mirrors the standard Zigbee availability progression: after the first failed ping cycle the
/// multiplier is 1.5, then 3, 6, 12 (capped by [`MAX_BACKOFF_MULTIPLIER`]).
/// A device that has never failed (or just recovered) uses multiplier 1.0.
fn backoff_multiplier(failed_ping_cycles: u32) -> f64 {
    if failed_ping_cycles == 0 {
        return 1.0;
    }
    let multiplier = 1.5 * 2.0_f64.powi(failed_ping_cycles as i32 - 1);
    multiplier.min(MAX_BACKOFF_MULTIPLIER)
}

/// Check all devices against their per-device availability timeout and actively
/// ping those whose timer has expired.  This replaces the old blind periodic
/// polling with a per-device timer approach.
///
/// **Non-blocking**: this function advances a state machine by one step per call.
/// It never sleeps or blocks — timing is handled by checking `Instant::now()`
/// against stored deadlines.  The event loop remains free to process user commands
/// between steps.
///
/// State machine:
///   `Idle` → scan for expired devices, send first ping → `WaitingForResponse`
///   `WaitingForResponse` → after PING_RETRY_DELAY, check `last_seen`:
///       responded → done, go to `CoolDown` (or `Idle` if queue empty)
///       not responded, attempts left → send another ping → `WaitingForResponse`
///       not responded, no attempts left → mark unreachable → `CoolDown`
///   `CoolDown` → after 2s → `Idle` (pick next device from queue)
///
/// Returns `true` if any device state changed (caller should sync to frontend).
async fn tick_device_availability(context: &mut EzspContext) -> bool {
    // Always drain pending callbacks so that recently-arrived responses
    // can update `last_seen`.
    drain_pending_callbacks(context).await;

    let now = Instant::now();
    let mut state_changed = false;

    // Take ownership of the current state to work with it.
    let current_state = std::mem::replace(&mut context.availability_state, AvailabilityState::Idle);

    context.availability_state = match current_state {
        AvailabilityState::Idle => {
            // If the queue is empty, scan for expired devices.
            if context.availability_queue.is_empty() {
                for device in &context.joined_devices {
                    let Some(endpoint) = device.endpoint else { continue };
                    if !device.interview_completed { continue; }
                    if !device.input_clusters.contains(&ON_OFF_CLUSTER_ID) { continue; }
                    if device.device_type == ZigbeeDeviceType::Remote { continue; }

                    let effective_timeout = AVAILABILITY_TIMEOUT.mul_f64(backoff_multiplier(device.failed_ping_cycles));
                    let is_expired = device
                        .last_seen
                        .map_or(true, |last_seen| now.duration_since(last_seen) > effective_timeout);

                    if is_expired {
                        context.availability_queue.push(AvailabilityTarget {
                            node_id: device.node_id,
                            endpoint,
                            was_reachable: device.reachable,
                            eui64: device.eui64.clone(),
                        });
                    }
                }
            }

            // Pick the next device to ping.
            if let Some(target) = context.availability_queue.first().cloned() {
                let max_attempts = if target.was_reachable { 2u32 } else { 1u32 };
                let before_ping = Instant::now();

                debug!(
                    node_id = format_args!("0x{:04x}", target.node_id),
                    eui64 = %target.eui64,
                    was_reachable = target.was_reachable,
                    attempts = max_attempts,
                    "availability ping started"
                );

                match send_read_attributes(context, target.node_id, target.endpoint, ON_OFF_CLUSTER_ID, &[0x0000]).await {
                    Ok(()) => {
                        debug!(
                            node_id = format_args!("0x{:04x}", target.node_id),
                            attempt = 1,
                            "availability ping sent"
                        );
                        AvailabilityState::WaitingForResponse {
                            node_id: target.node_id,
                            eui64: target.eui64,
                            was_reachable: target.was_reachable,
                            attempt: 1,
                            max_attempts,
                            before_ping,
                            check_at: Instant::now() + PING_RETRY_DELAY,
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{:04x}", target.node_id),
                            error = %error,
                            "availability ping send failed — skipping device"
                        );
                        // EZSP-level failure — mark unreachable immediately.
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == target.node_id) {
                            if device.reachable {
                                info!(
                                    node_id = format_args!("0x{:04x}", target.node_id),
                                    eui64 = %target.eui64,
                                    "device marked unreachable after EZSP send failure"
                                );
                                device.reachable = false;
                                device.desired_state_applied = false;
                                state_changed = true;
                            }
                            device.failed_ping_cycles += 1;
                        }
                        context.availability_queue.remove(0);
                        if context.availability_queue.is_empty() {
                            AvailabilityState::Idle
                        } else {
                            AvailabilityState::CoolDown { resume_at: Instant::now() + StdDuration::from_secs(2) }
                        }
                    }
                }
            } else {
                AvailabilityState::Idle
            }
        }

        AvailabilityState::WaitingForResponse {
            node_id, eui64, was_reachable, attempt, max_attempts, before_ping, check_at,
        } => {
            if now < check_at {
                // Not time to check yet — stay in this state.
                AvailabilityState::WaitingForResponse {
                    node_id, eui64, was_reachable, attempt, max_attempts, before_ping, check_at,
                }
            } else {
                // Time to check if the device responded.
                let device_responded = context
                    .joined_devices
                    .iter()
                    .find(|d| d.node_id == node_id)
                    .and_then(|d| d.last_seen)
                    .map_or(false, |ls| ls > before_ping);

                if device_responded {
                    debug!(
                        node_id = format_args!("0x{node_id:04x}"),
                        attempt,
                        "availability ping got response — device is online"
                    );
                    // Success — remove from queue, move on.
                    context.availability_queue.retain(|t| t.node_id != node_id);
                    state_changed = true;
                    if context.availability_queue.is_empty() {
                        AvailabilityState::Idle
                    } else {
                        AvailabilityState::CoolDown { resume_at: Instant::now() + StdDuration::from_secs(2) }
                    }
                } else if attempt < max_attempts {
                    // First attempt failed — mark unreachable immediately for fast
                    // frontend feedback, then retry to confirm before bumping backoff.
                    if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                        if device.reachable {
                            info!(
                                node_id = format_args!("0x{node_id:04x}"),
                                eui64 = %eui64,
                                attempt,
                                "device marked unreachable after first failed ping (retrying)"
                            );
                            device.reachable = false;
                            device.desired_state_applied = false;
                            state_changed = true;
                        }
                    }

                    // Retry — send another ping.
                    let endpoint = context.availability_queue.iter()
                        .find(|t| t.node_id == node_id)
                        .map(|t| t.endpoint)
                        .unwrap_or(1);
                    let before_retry = Instant::now();

                    match send_read_attributes(context, node_id, endpoint, ON_OFF_CLUSTER_ID, &[0x0000]).await {
                        Ok(()) => {
                            debug!(
                                node_id = format_args!("0x{node_id:04x}"),
                                attempt = attempt + 1,
                                "availability ping sent (retry)"
                            );
                            AvailabilityState::WaitingForResponse {
                                node_id,
                                eui64,
                                was_reachable,
                                attempt: attempt + 1,
                                max_attempts,
                                before_ping: before_retry,
                                check_at: Instant::now() + PING_RETRY_DELAY,
                            }
                        }
                        Err(error) => {
                            warn!(
                                node_id = format_args!("0x{node_id:04x}"),
                                attempt = attempt + 1,
                                error = %error,
                                "availability ping retry failed"
                            );
                            // Mark unreachable.
                            if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                                if device.reachable {
                                    info!(
                                        node_id = format_args!("0x{node_id:04x}"),
                                        eui64 = %eui64,
                                        failed_cycles = device.failed_ping_cycles + 1,
                                        "device marked unreachable after failed availability pings"
                                    );
                                    device.reachable = false;
                                    device.desired_state_applied = false;
                                    state_changed = true;
                                }
                                device.failed_ping_cycles += 1;
                            }
                            context.availability_queue.retain(|t| t.node_id != node_id);
                            if context.availability_queue.is_empty() {
                                AvailabilityState::Idle
                            } else {
                                AvailabilityState::CoolDown { resume_at: Instant::now() + StdDuration::from_secs(2) }
                            }
                        }
                    }
                } else {
                    // All attempts exhausted — mark as unreachable.
                    if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                        if device.reachable {
                            info!(
                                node_id = format_args!("0x{node_id:04x}"),
                                eui64 = %eui64,
                                failed_cycles = device.failed_ping_cycles + 1,
                                "device marked unreachable after failed availability pings"
                            );
                            device.reachable = false;
                            device.desired_state_applied = false;
                            state_changed = true;
                        }
                        device.failed_ping_cycles += 1;
                    }
                    context.availability_queue.retain(|t| t.node_id != node_id);
                    if context.availability_queue.is_empty() {
                        AvailabilityState::Idle
                    } else {
                        AvailabilityState::CoolDown { resume_at: Instant::now() + StdDuration::from_secs(2) }
                    }
                }
            }
        }

        AvailabilityState::CoolDown { resume_at } => {
            if now < resume_at {
                AvailabilityState::CoolDown { resume_at }
            } else {
                // Cooldown over — go back to Idle to pick next device.
                AvailabilityState::Idle
            }
        }
    };

    state_changed
}

/// Drain all currently-queued callbacks without waiting,
/// so that pending ZCL responses can update device state immediately.
async fn drain_pending_callbacks(context: &mut EzspContext) {
    while let Ok(callback) = context.callbacks_rx.try_recv() {
        handle_callback(context, callback).await;
    }
}

/// Re-apply desired brightness and/or colour temperature to devices that have just
/// transitioned from unreachable to reachable (i.e. `reachable && !desired_state_applied`).
///
/// This handles the physical wall-switch scenario: a user turns off a lamp at the wall,
/// then turns it back on.  The lamp powers up at factory defaults, so we push the last
/// user-set brightness and temperature to it.
///
/// The lamp is intentionally forced ON when it reappears after a wall-switch (handled
/// elsewhere via `ensure_joined_device` when the device was previously unreachable),
/// so we do **not** touch the on/off state here.
///
/// **IMPORTANT**: no `timeout()` wrapper — the caller handles timeouts.
async fn restore_desired_state(context: &mut EzspContext) {
    // Collect targets: devices that are reachable, have an endpoint, and have unapplied desired state.
    let targets: Vec<(u16, u8, Option<u8>, Option<u8>, Option<f32>, Option<f32>, bool, bool, bool, bool)> = context
        .joined_devices
        .iter()
        .filter(|d| d.reachable && !d.desired_state_applied && d.endpoint.is_some())
        .filter(|d| d.desired_brightness.is_some() || d.desired_temperature.is_some() || d.desired_color_x.is_some())
        .map(|d| {
            (
                d.node_id,
                d.endpoint.unwrap(),
                d.desired_brightness,
                d.desired_temperature,
                d.desired_color_x,
                d.desired_color_y,
                d.supports_brightness,
                d.supports_temperature,
                d.supports_xy_color,
                d.is_on,
            )
        })
        .collect();

    for (node_id, endpoint, desired_brightness, desired_temperature, desired_color_x, desired_color_y, supports_brightness, supports_temperature, has_color_control, was_on) in targets {
        // Restore brightness — but only if the lamp was supposed to be on.
        // Never send MoveToLevel(0) here: if the lamp was turned off (e.g. via
        // dimmer or wall switch), we should not re-send brightness 0 when it
        // reappears — that would turn it off again immediately after a physical
        // switch-on.
        if let Some(brightness) = desired_brightness {
            if supports_brightness && was_on && brightness > 0 {
                let sequence = next_device_sequence(context, node_id);
                let zcl_payload = build_brightness_command_payload(brightness, sequence);
                let aps_frame = EzspApsFrame::new(
                    HOME_AUTOMATION_PROFILE_ID,
                    LEVEL_CONTROL_CLUSTER_ID,
                    DEFAULT_SOURCE_ENDPOINT,
                    endpoint,
                    EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                    0,
                    0,
                );

                match context.connection
                    .send_unicast(
                        Destination::Direct(NodeId::from(node_id)),
                        aps_frame,
                        0,
                        zcl_payload.into_iter().collect(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            node_id = format_args!("0x{node_id:04x}"),
                            brightness,
                            "restored desired brightness after reconnect"
                        );
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            device.brightness = brightness;
                            device.is_on = brightness > 0;
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{node_id:04x}"),
                            error = %error,
                            "failed to restore desired brightness"
                        );
                        // Don't mark as applied — retry next cycle.
                        continue;
                    }
                }
            }
        }

        // Restore colour temperature
        if let Some(temperature) = desired_temperature {
            if supports_temperature {
                let sequence = next_device_sequence(context, node_id);
                let zcl_payload = build_color_temperature_command_payload(temperature, sequence);
                let aps_frame = EzspApsFrame::new(
                    HOME_AUTOMATION_PROFILE_ID,
                    COLOR_CONTROL_CLUSTER_ID,
                    DEFAULT_SOURCE_ENDPOINT,
                    endpoint,
                    EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                    0,
                    0,
                );

                match context.connection
                    .send_unicast(
                        Destination::Direct(NodeId::from(node_id)),
                        aps_frame,
                        0,
                        zcl_payload.into_iter().collect(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            node_id = format_args!("0x{node_id:04x}"),
                            temperature,
                            "restored desired colour temperature after reconnect"
                        );
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            device.temperature = Some(temperature);
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{node_id:04x}"),
                            error = %error,
                            "failed to restore desired colour temperature"
                        );
                        // Don't mark as applied — retry next cycle.
                        continue;
                    }
                }
            }
        }

        // Restore colour XY
        if let (Some(color_x), Some(color_y)) = (desired_color_x, desired_color_y) {
            if has_color_control {
                let sequence = next_device_sequence(context, node_id);
                let zcl_payload = build_color_xy_command_payload(color_x, color_y, sequence);
                let aps_frame = EzspApsFrame::new(
                    HOME_AUTOMATION_PROFILE_ID,
                    COLOR_CONTROL_CLUSTER_ID,
                    DEFAULT_SOURCE_ENDPOINT,
                    endpoint,
                    EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                    0,
                    0,
                );

                match context.connection
                    .send_unicast(
                        Destination::Direct(NodeId::from(node_id)),
                        aps_frame,
                        0,
                        zcl_payload.into_iter().collect(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            node_id = format_args!("0x{node_id:04x}"),
                            color_x,
                            color_y,
                            "restored desired colour XY after reconnect"
                        );
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            device.color_x = Some(color_x);
                            device.color_y = Some(color_y);
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{node_id:04x}"),
                            error = %error,
                            "failed to restore desired colour XY"
                        );
                        // Don't mark as applied — retry next cycle.
                        continue;
                    }
                }
            }
        }

        // All commands succeeded (or were skipped because unsupported) — mark as applied.
        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
            device.desired_state_applied = true;
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                eui64 = %device.eui64,
                "desired state fully restored after reconnect"
            );
        }

        // Drain callbacks so any responses update device state promptly.
        drain_pending_callbacks(context).await;
    }
}

async fn retry_pending_interviews(context: &mut EzspContext) {
    // Only retry a few times proactively.  Sleepy end devices (remotes) are
    // unlikely to respond to unsolicited requests, so we cap attempts here and
    // rely on intercept-on-wake (see handle_incoming_cluster) instead.
    const MAX_PROACTIVE_INTERVIEW_ATTEMPTS: u32 = 3;

    let retry_targets = context
        .joined_devices
        .iter()
        .filter(|device| device.connected && device.endpoint.is_none() && device.interview_attempts < MAX_PROACTIVE_INTERVIEW_ATTEMPTS)
        .map(|device| (device.node_id, device.eui64.clone(), device.interview_attempts))
        .collect::<Vec<_>>();

    for (node_id, eui64, attempts) in retry_targets {
        debug!(
            node_id = format_args!("0x{node_id:04x}"),
            eui64,
            attempts,
            "retrying native zigbee endpoint discovery"
        );
        if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
            device.interview_attempts = device.interview_attempts.saturating_add(1);
        }
        if let Err(error) = request_active_endpoints(context, node_id).await {
            warn!(node_id = format_args!("0x{node_id:04x}"), error = %error, "native zigbee endpoint discovery retry failed");
        }
        // Also probe common endpoints directly — some sleepy devices ignore
        // Active_EP_req but still respond to Simple_Desc_req.
        for endpoint in [1u8, 2] {
            let _ = request_simple_descriptor(context, node_id, endpoint).await;
        }
    }
}

async fn request_active_endpoints(context: &mut EzspContext, node_id: u16) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let payload = vec![sequence, (node_id & 0xff) as u8, (node_id >> 8) as u8];
    let aps_frame = EzspApsFrame::new(
        ZDO_PROFILE_ID,
        ACTIVE_EP_REQ_CLUSTER_ID,
        0,
        0,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context.connection
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Active_EP_req"))
}

async fn request_simple_descriptor(
    context: &mut EzspContext,
    node_id: u16,
    endpoint: u8,
) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let payload = vec![sequence, (node_id & 0xff) as u8, (node_id >> 8) as u8, endpoint];
    let aps_frame = EzspApsFrame::new(
        ZDO_PROFILE_ID,
        SIMPLE_DESC_REQ_CLUSTER_ID,
        0,
        0,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context.connection
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Simple_Desc_req"))
}

/// Parse a colon-separated EUI64 string (e.g. "00:17:88:01:08:0c:00:0b")
/// into raw bytes in network order (big-endian).
fn parse_eui64_bytes(eui64: &str) -> Option<[u8; 8]> {
    let parts: Vec<&str> = eui64.split(':').collect();
    if parts.len() != 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(bytes)
}

/// Send a ZDO Bind_req to `target_node_id`, telling it to bind its
/// `cluster_id` on `remote_endpoint` to our coordinator's endpoint 1.
///
/// The Bind_req payload (ZDP spec section 2.4.3.2.2):
/// ```text
/// [seq(1)] [src_addr(8, LE)] [src_ep(1)] [cluster(2, LE)]
/// [dst_addr_mode(1)] [dst_addr(8, LE)] [dst_ep(1)]
/// ```
/// `dst_addr_mode` 0x03 = 64-bit extended address.
async fn send_bind_request(
    context: &mut EzspContext,
    target_node_id: u16,
    remote_eui64: &str,
    remote_endpoint: u8,
    cluster_id: u16,
) -> Result<(), AppError> {
    let coordinator_eui64 = context.coordinator_eui64.ok_or_else(|| {
        AppError::service_unavailable("coordinator EUI64 not available for bind request")
    })?;

    let remote_eui64_bytes = parse_eui64_bytes(remote_eui64).ok_or_else(|| {
        AppError::service_unavailable(format!("invalid remote EUI64: {remote_eui64}"))
    })?;

    let coordinator_bytes = coordinator_eui64.into_array();
    let sequence = next_device_sequence(context, target_node_id);

    // Build the Bind_req payload.
    // EUI64 on the wire is little-endian (reversed from the display order).
    let mut payload = Vec::with_capacity(23);
    payload.push(sequence);
    // Source address: remote EUI64 in little-endian.
    for &byte in remote_eui64_bytes.iter().rev() {
        payload.push(byte);
    }
    // Source endpoint.
    payload.push(remote_endpoint);
    // Cluster ID (little-endian).
    payload.push((cluster_id & 0xff) as u8);
    payload.push((cluster_id >> 8) as u8);
    // Destination address mode: 0x03 = 64-bit unicast.
    payload.push(0x03);
    // Destination address: coordinator EUI64 in little-endian.
    for &byte in coordinator_bytes.iter().rev() {
        payload.push(byte);
    }
    // Destination endpoint.
    payload.push(DEFAULT_SOURCE_ENDPOINT);

    let aps_frame = EzspApsFrame::new(
        ZDO_PROFILE_ID,
        BIND_REQ_CLUSTER_ID,
        0,
        0,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    info!(
        target_node_id = format_args!("0x{target_node_id:04x}"),
        remote_eui64 = %remote_eui64,
        remote_endpoint,
        cluster_id = format_args!("0x{cluster_id:04x}"),
        coordinator_eui64 = %format_eui64(coordinator_eui64),
        "sending ZDO Bind_req to remote"
    );

    context.connection
        .send_unicast(
            Destination::Direct(NodeId::from(target_node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Bind_req"))
}

/// Bind a Hue Dimmer's output clusters to our coordinator.
///
/// Endpoint 1 → standard ZCL On/Off + Level Control (direct lamp commands).
/// Endpoint 2 → Philips manufacturer-specific cluster 0xFC00 (hueNotification
///              button events).
///
/// Both endpoints are bound so we receive button presses regardless of which
/// reporting path the remote firmware prefers.
async fn bind_remote_clusters(context: &mut EzspContext, node_id: u16, eui64: &str) {
    // Endpoint 1: standard HA clusters.
    for cluster_id in [ON_OFF_CLUSTER_ID, LEVEL_CONTROL_CLUSTER_ID] {
        match send_bind_request(context, node_id, eui64, 1, cluster_id).await {
            Ok(()) => {
                info!(
                    node_id = format_args!("0x{node_id:04x}"),
                    endpoint = 1,
                    cluster_id = format_args!("0x{cluster_id:04x}"),
                    "ZDO Bind_req sent successfully"
                );
            }
            Err(error) => {
                warn!(
                    node_id = format_args!("0x{node_id:04x}"),
                    endpoint = 1,
                    cluster_id = format_args!("0x{cluster_id:04x}"),
                    error = %error,
                    "ZDO Bind_req failed"
                );
            }
        }
    }

    // Endpoint 2: Philips-specific hueNotification cluster.
    match send_bind_request(context, node_id, eui64, 2, PHILIPS_SPECIFIC_CLUSTER_ID).await {
        Ok(()) => {
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                endpoint = 2,
                cluster_id = format_args!("0x{PHILIPS_SPECIFIC_CLUSTER_ID:04x}"),
                "ZDO Bind_req sent successfully (Philips cluster)"
            );
        }
        Err(error) => {
            warn!(
                node_id = format_args!("0x{node_id:04x}"),
                endpoint = 2,
                cluster_id = format_args!("0x{PHILIPS_SPECIFIC_CLUSTER_ID:04x}"),
                error = %error,
                "ZDO Bind_req failed (Philips cluster)"
            );
        }
    }
}

fn next_device_sequence(context: &mut EzspContext, node_id: u16) -> u8 {
    next_sequence_for_device(
        &mut context.next_device_sequence,
        &mut context.next_global_sequence,
        node_id,
    )
}

fn next_sequence_for_device(
    per_device: &mut HashMap<u16, u8>,
    next_global: &mut u8,
    node_id: u16,
) -> u8 {
    let current = per_device.entry(node_id).or_insert_with(|| {
        let value = *next_global;
        *next_global = next_global.wrapping_add(1);
        value
    });
    let sequence = *current;
    *current = current.wrapping_add(1);
    sequence
}

fn should_probe_active_endpoints(target: &DiscoveredDevice) -> bool {
    target.endpoint.is_none() || target.input_clusters.is_empty()
}

async fn request_known_device_discovery(context: &mut EzspContext, node_id: u16) {
    let target = context.joined_devices.iter().find(|device| device.node_id == node_id).cloned();
    let Some(target) = target else {
        return;
    };

    // Remotes are already classified at join time — no discovery needed.
    if target.device_type == ZigbeeDeviceType::Remote {
        return;
    }

    // Record the timestamp so intercept-on-wake can apply a cooldown.
    if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
        device.last_discovery_at = Some(Instant::now());
    }

    if should_probe_active_endpoints(&target) {
        // Send Active_EP_req (standard discovery path).
        let _ = request_active_endpoints(context, node_id).await;

        // Also send Simple_Desc_req directly for common endpoints (1 and 2).
        // Sleepy end devices like the Hue Dimmer Switch often ignore
        // Active_EP_req but still respond to direct Simple_Desc_req while
        // they are briefly awake after joining.
        for endpoint in [1u8, 2] {
            let _ = request_simple_descriptor(context, node_id, endpoint).await;
        }
    } else {
        let _ = refresh_device_state(context, &target).await;
    }
}

/// Handle a ZCL command received from a remote / dimmer switch and broadcast
/// the corresponding action to all connected lamps.
async fn handle_remote_command(
    context: &mut EzspContext,
    remote_node_id: u16,
    cluster_id: u16,
    payload: &[u8],
) {
    info!(
        remote_node_id = format_args!("0x{remote_node_id:04x}"),
        cluster_id = format_args!("0x{cluster_id:04x}"),
        payload = %hex_bytes(payload),
        payload_len = payload.len(),
        "remote: incoming command from remote device"
    );

    // ZCL frame: [frame_control, sequence, command_id, ...]
    if payload.len() < 3 {
        warn!(
            remote_node_id = format_args!("0x{remote_node_id:04x}"),
            payload = %hex_bytes(payload),
            "remote: payload too short (need >= 3 bytes for ZCL frame)"
        );
        return;
    }

    let frame_control = payload[0];

    // We only care about cluster-specific commands (direction: client → server).
    // Frame control bit 0-1 = 01 (cluster-specific), bit 3 = 0 (client → server).
    let is_cluster_specific = (frame_control & 0x03) == 0x01;
    if !is_cluster_specific {
        info!(
            remote_node_id = format_args!("0x{remote_node_id:04x}"),
            frame_control = format_args!("0x{frame_control:02x}"),
            "remote: ignoring non-cluster-specific frame (global command or report)"
        );
        return;
    }

    // Manufacturer-specific frames (frame_control bit 2 set) have a 2-byte
    // manufacturer code between the sequence number and the command ID:
    //   Standard:  [frame_control, sequence, command_id, ...]
    //   Mfr-spec:  [frame_control, sequence, mfr_lo, mfr_hi, command_id, ...]
    let is_manufacturer_specific = (frame_control & 0x04) != 0;
    let (command_id, zcl_header_len) = if is_manufacturer_specific {
        if payload.len() < 5 {
            warn!(
                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                payload = %hex_bytes(payload),
                "remote: manufacturer-specific payload too short (need >= 5 bytes)"
            );
            return;
        }
        (payload[4], 5)
    } else {
        (payload[2], 3)
    };

    // Update the remote's last_seen timestamp.
    if let Some(remote) = context.joined_devices.iter_mut().find(|d| d.node_id == remote_node_id) {
        remote.last_seen = Some(Instant::now());
        remote.reachable = true;
        remote.connected = true;
        remote.failed_ping_cycles = 0;
    }

    match cluster_id {
        ON_OFF_CLUSTER_ID => {
            match command_id {
                ZCL_ON_OFF_COMMAND_ON => {
                    if let Some((true, ts)) = context.last_remote_power {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate ON (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: ON button pressed — turning all lamps ON"
                    );
                    context.last_remote_power = Some((true, Instant::now()));
                    broadcast_power_to_all_lamps(context, true).await;
                }
                ZCL_ON_OFF_COMMAND_OFF => {
                    if let Some((false, ts)) = context.last_remote_power {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate OFF (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: OFF button pressed — turning all lamps OFF"
                    );
                    context.last_remote_power = Some((false, Instant::now()));
                    broadcast_power_to_all_lamps(context, false).await;
                }
                _ => {
                    debug!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        command_id,
                        "remote: unknown On/Off command"
                    );
                }
            }
        }
        LEVEL_CONTROL_CLUSTER_ID => {
            match command_id {
                ZCL_LEVEL_CONTROL_COMMAND_STEP | ZCL_LEVEL_CONTROL_COMMAND_STEP_WITH_ON_OFF => {
                    // Step payload (after ZCL header): [step_mode(1), step_size(1), transition_time(2)]
                    // step_mode: 0x00 = Up, 0x01 = Down
                    if payload.len() > zcl_header_len {
                        let step_mode = payload[zcl_header_len];
                        let step_up = step_mode == 0x00;
                        if let Some((last_up, ts)) = context.last_remote_brightness_step {
                            if last_up == step_up && ts.elapsed() < REMOTE_DEDUP_WINDOW {
                                debug!(
                                    remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                    "remote: suppressing duplicate brightness step (within dedup window)"
                                );
                                return;
                            }
                        }
                        info!(
                            remote_node_id = format_args!("0x{remote_node_id:04x}"),
                            direction = if step_up { "up" } else { "down" },
                            "remote: brightness step — adjusting all lamps"
                        );
                        context.last_remote_brightness_step = Some((step_up, Instant::now()));
                        broadcast_brightness_step_to_all_lamps(context, step_up).await;
                    }
                }
                ZCL_LEVEL_CONTROL_COMMAND_MOVE | ZCL_LEVEL_CONTROL_COMMAND_MOVE_WITH_ON_OFF => {
                    // Move payload (after ZCL header): [move_mode(1), rate(1)]
                    // move_mode: 0x00 = Up, 0x01 = Down
                    if payload.len() > zcl_header_len {
                        let move_mode = payload[zcl_header_len];
                        let step_up = move_mode == 0x00;
                        if let Some((last_up, ts)) = context.last_remote_brightness_step {
                            if last_up == step_up && ts.elapsed() < REMOTE_DEDUP_WINDOW {
                                debug!(
                                    remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                    "remote: suppressing duplicate brightness move (within dedup window)"
                                );
                                return;
                            }
                        }
                        info!(
                            remote_node_id = format_args!("0x{remote_node_id:04x}"),
                            direction = if step_up { "up" } else { "down" },
                            "remote: brightness move — adjusting all lamps"
                        );
                        context.last_remote_brightness_step = Some((step_up, Instant::now()));
                        broadcast_brightness_step_to_all_lamps(context, step_up).await;
                    }
                }
                ZCL_LEVEL_CONTROL_COMMAND_STOP | ZCL_LEVEL_CONTROL_COMMAND_STOP_WITH_ON_OFF => {
                    debug!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: level stop command (ignored)"
                    );
                }
                _ => {
                    debug!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        command_id,
                        "remote: unknown Level Control command"
                    );
                }
            }
        }
        PHILIPS_SPECIFIC_CLUSTER_ID => {
            // Philips hueNotification (command 0x00 on cluster 0xFC00, endpoint 2).
            // This is a manufacturer-specific cluster command.  The ZCL payload
            // (after the header) is:
            //   [button(1), unknown(3), action_type(1), unknown(1), time(1), unknown(1)]
            //
            // button:      1 = On, 2 = Dim Up, 3 = Dim Down, 4 = Off
            // action_type: 0 = initial_press, 1 = hold (repeat),
            //              2 = short_release, 3 = long_release
            let hue_payload = &payload[zcl_header_len..];
            if hue_payload.is_empty() {
                warn!(
                    remote_node_id = format_args!("0x{remote_node_id:04x}"),
                    "remote: hueNotification payload is empty"
                );
                return;
            }

            let button = hue_payload[0];
            let action_type = if hue_payload.len() >= 5 { hue_payload[4] } else { 0 };

            let button_name = match button {
                1 => "On",
                2 => "Dim Up",
                3 => "Dim Down",
                4 => "Off",
                _ => "Unknown",
            };
            let action_name = match action_type {
                0 => "initial_press",
                1 => "hold",
                2 => "short_release",
                3 => "long_release",
                _ => "unknown",
            };

            info!(
                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                button,
                button_name,
                action_type,
                action_name,
                hue_payload = %hex_bytes(hue_payload),
                "remote: hueNotification from Philips dimmer"
            );

            // Act on initial_press (0) and hold/repeat (1) only.
            // Ignore release events to avoid double-firing.
            if action_type > 1 {
                debug!(
                    remote_node_id = format_args!("0x{remote_node_id:04x}"),
                    action_name,
                    "remote: ignoring release event"
                );
                return;
            }

            match button {
                1 => {
                    if let Some((true, ts)) = context.last_remote_power {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate hue ON (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: hue ON button — turning all lamps ON"
                    );
                    context.last_remote_power = Some((true, Instant::now()));
                    broadcast_power_to_all_lamps(context, true).await;
                }
                4 => {
                    if let Some((false, ts)) = context.last_remote_power {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate hue OFF (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: hue OFF button — turning all lamps OFF"
                    );
                    context.last_remote_power = Some((false, Instant::now()));
                    broadcast_power_to_all_lamps(context, false).await;
                }
                2 => {
                    if let Some((true, ts)) = context.last_remote_brightness_step {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate hue DIM UP (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: hue DIM UP — increasing brightness"
                    );
                    context.last_remote_brightness_step = Some((true, Instant::now()));
                    broadcast_brightness_step_to_all_lamps(context, true).await;
                }
                3 => {
                    if let Some((false, ts)) = context.last_remote_brightness_step {
                        if ts.elapsed() < REMOTE_DEDUP_WINDOW {
                            debug!(
                                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                                "remote: suppressing duplicate hue DIM DOWN (within dedup window)"
                            );
                            return;
                        }
                    }
                    info!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        "remote: hue DIM DOWN — decreasing brightness"
                    );
                    context.last_remote_brightness_step = Some((false, Instant::now()));
                    broadcast_brightness_step_to_all_lamps(context, false).await;
                }
                _ => {
                    debug!(
                        remote_node_id = format_args!("0x{remote_node_id:04x}"),
                        button,
                        "remote: unknown hueNotification button"
                    );
                }
            }
        }
        _ => {
            debug!(
                remote_node_id = format_args!("0x{remote_node_id:04x}"),
                cluster_id = format_args!("0x{cluster_id:04x}"),
                command_id,
                payload = %hex_bytes(payload),
                "remote: unhandled cluster command"
            );
        }
    }
}

/// Send a power on/off command to every connected lamp.
async fn broadcast_power_to_all_lamps(context: &mut EzspContext, enabled: bool) {
    let targets: Vec<(u16, u8)> = context
        .joined_devices
        .iter()
        .filter(|d| d.device_type == ZigbeeDeviceType::Lamp && d.reachable && d.endpoint.is_some())
        .filter(|d| d.input_clusters.contains(&ON_OFF_CLUSTER_ID))
        .map(|d| (d.node_id, d.endpoint.unwrap()))
        .collect();

    info!(
        enabled,
        target_count = targets.len(),
        "remote broadcast: sending power command to all lamps"
    );

    for (lamp_node_id, endpoint) in targets {
        let sequence = next_device_sequence(context, lamp_node_id);
        let zcl_payload = build_on_off_command_payload(enabled, sequence);
        let aps_frame = EzspApsFrame::new(
            HOME_AUTOMATION_PROFILE_ID,
            ON_OFF_CLUSTER_ID,
            DEFAULT_SOURCE_ENDPOINT,
            endpoint,
            EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
            0,
            0,
        );

        match context.connection
            .send_unicast(
                Destination::Direct(NodeId::from(lamp_node_id)),
                aps_frame,
                0,
                zcl_payload.into_iter().collect(),
            )
            .await
        {
            Ok(_) => {
                if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == lamp_node_id) {
                    device.is_on = enabled;
                }
                debug!(
                    lamp_node_id = format_args!("0x{lamp_node_id:04x}"),
                    enabled,
                    "remote broadcast: power command sent"
                );
            }
            Err(error) => {
                warn!(
                    lamp_node_id = format_args!("0x{lamp_node_id:04x}"),
                    error = %error,
                    "remote broadcast: power command failed"
                );
            }
        }
    }
}

/// Adjust brightness of every connected lamp by a fixed step.
async fn broadcast_brightness_step_to_all_lamps(context: &mut EzspContext, step_up: bool) {
    let targets: Vec<(u16, u8, u8)> = context
        .joined_devices
        .iter()
        .filter(|d| d.device_type == ZigbeeDeviceType::Lamp && d.reachable && d.endpoint.is_some())
        .filter(|d| d.supports_brightness && d.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID))
        .map(|d| (d.node_id, d.endpoint.unwrap(), d.brightness))
        .collect();

    info!(
        direction = if step_up { "up" } else { "down" },
        step = DIMMER_BRIGHTNESS_STEP,
        target_count = targets.len(),
        "remote broadcast: sending brightness step to all lamps"
    );

    for (lamp_node_id, endpoint, current_brightness) in targets {
        let new_brightness = if step_up {
            current_brightness.saturating_add(DIMMER_BRIGHTNESS_STEP).min(100)
        } else {
            current_brightness.saturating_sub(DIMMER_BRIGHTNESS_STEP).max(1)
        };

        let sequence = next_device_sequence(context, lamp_node_id);
        let zcl_payload = build_brightness_command_payload(new_brightness, sequence);
        let aps_frame = EzspApsFrame::new(
            HOME_AUTOMATION_PROFILE_ID,
            LEVEL_CONTROL_CLUSTER_ID,
            DEFAULT_SOURCE_ENDPOINT,
            endpoint,
            EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
            0,
            0,
        );

        match context.connection
            .send_unicast(
                Destination::Direct(NodeId::from(lamp_node_id)),
                aps_frame,
                0,
                zcl_payload.into_iter().collect(),
            )
            .await
        {
            Ok(_) => {
                if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == lamp_node_id) {
                    device.brightness = new_brightness;
                    device.is_on = new_brightness > 0;
                    device.desired_brightness = Some(new_brightness);
                }
                debug!(
                    lamp_node_id = format_args!("0x{lamp_node_id:04x}"),
                    new_brightness,
                    "remote broadcast: brightness step sent"
                );
            }
            Err(error) => {
                warn!(
                    lamp_node_id = format_args!("0x{lamp_node_id:04x}"),
                    error = %error,
                    "remote broadcast: brightness step failed"
                );
            }
        }
    }
}

async fn handle_incoming_cluster(
    context: &mut EzspContext,
    node_id: u16,
    cluster_id: u16,
    profile_id: u16,
    payload: &[u8],
) -> Option<NativeZigbeeEvent> {
    // --- Intercept-on-wake / auto-classify: if this device hasn't completed
    // its interview, it must be awake right now (it just sent us something).
    // Check whether the incoming message is a ZCL cluster-specific command on
    // On/Off or Level Control — if so, the device is a remote control and will
    // *never* answer ZDO requests.  Classify it immediately instead of wasting
    // time on futile discovery.
    let needs_interview = context
        .joined_devices
        .iter()
        .any(|device| device.node_id == node_id && device.endpoint.is_none());

    // Debug: log the current classification of this device so we can trace
    // unexpected Remote classifications.
    if let Some(device) = context.joined_devices.iter().find(|d| d.node_id == node_id) {
        debug!(
            node_id = format_args!("0x{node_id:04x}"),
            cluster_id = format_args!("0x{cluster_id:04x}"),
            device_type = ?device.device_type,
            endpoint = ?device.endpoint,
            interview_completed = device.interview_completed,
            needs_interview,
            "incoming message: device state at dispatch time"
        );
    }

    if needs_interview {
        // ZDO responses (profile 0x0000) such as Active_EP_rsp (0x8005) and
        // Simple_Desc_rsp (0x8004) are replies to our OWN discovery requests.
        // They must NOT re-trigger discovery — doing so creates a feedback loop
        // where every ZDO response causes 3+ new EZSP round-trips whose
        // responses trigger yet more discovery, starving the command channel and
        // making HTTP requests hang indefinitely.
        let is_zdo_response = profile_id == ZDO_PROFILE_ID;

        let is_light_cluster = cluster_id == ON_OFF_CLUSTER_ID || cluster_id == LEVEL_CONTROL_CLUSTER_ID;
        let is_philips_cluster = cluster_id == PHILIPS_SPECIFIC_CLUSTER_ID;
        let is_cluster_specific_command = payload.len() >= 3 && (payload[0] & 0x03) == 0x01;

        if (is_light_cluster || is_philips_cluster) && is_cluster_specific_command {
            // The uninterviewed device is sending us On/Off or Level Control
            // cluster-specific commands — it's a remote / dimmer switch.
            // Classify it immediately so button presses work right away.
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                cluster_id = format_args!("0x{cluster_id:04x}"),
                payload = %hex_bytes(payload),
                "uninterviewed device sent ZCL remote command — auto-classifying as remote"
            );
            let remote_eui64 = context.joined_devices.iter().find(|d| d.node_id == node_id).map(|d| d.eui64.clone());
            if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                device.device_type = ZigbeeDeviceType::Remote;
                device.endpoint = Some(1);
                device.output_clusters = vec![ON_OFF_CLUSTER_ID, LEVEL_CONTROL_CLUSTER_ID];
                device.interview_completed = true;
                device.supports_brightness = false;
                device.supports_temperature = false;
                device.has_color_control_cluster = false;
                device.supports_xy_color = false;
            }
            // The device is awake now — send bind requests so future presses reach us.
            if let Some(eui64) = remote_eui64 {
                bind_remote_clusters(context, node_id, &eui64).await;
            }
            // Fall through to the remote routing below — don't trigger ZDO discovery.
        } else if !is_zdo_response {
            // The device sent an application-layer message (not a ZDO
            // response), so it is genuinely awake.  Rate-limit discovery to
            // at most once per 10 seconds to avoid flooding the EZSP pipeline.
            const DISCOVERY_COOLDOWN: StdDuration = StdDuration::from_secs(10);
            let should_discover = context
                .joined_devices
                .iter()
                .find(|d| d.node_id == node_id)
                .map(|d| {
                    d.last_discovery_at
                        .map_or(true, |ts| ts.elapsed() >= DISCOVERY_COOLDOWN)
                })
                .unwrap_or(false);

            if should_discover {
                info!(
                    node_id = format_args!("0x{node_id:04x}"),
                    cluster_id = format_args!("0x{cluster_id:04x}"),
                    "device is awake but uninterviewed — triggering discovery now"
                );
                if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                    device.last_discovery_at = Some(Instant::now());
                }
                request_known_device_discovery(context, node_id).await;
            } else {
                debug!(
                    node_id = format_args!("0x{node_id:04x}"),
                    cluster_id = format_args!("0x{cluster_id:04x}"),
                    "device is awake but discovery cooldown active — skipping"
                );
            }
        }
    }

    // Check if this message comes from a known remote device.
    let sender_is_remote = context
        .joined_devices
        .iter()
        .any(|device| device.node_id == node_id && device.device_type == ZigbeeDeviceType::Remote);

    if sender_is_remote {
        // Skip ZDO frames (Bind Response, etc.) — they are
        // not ZCL commands.  ZDO uses profile_id 0x0000 and its cluster IDs
        // overlap with ZCL application clusters (e.g. On/Off 0x0006), so we
        // MUST use the APS profile_id to distinguish them — not the cluster_id
        // range.
        // EXCEPTION: Device_annce (0x0013) is always processed — a device that
        // was previously classified as remote might have been factory-reset and
        // re-joined as a different device type.  We let it fall through to the
        // main match block so it gets proper ZDO handling (re-interview etc.).
        let is_zdo = profile_id == ZDO_PROFILE_ID;
        let is_device_announce = cluster_id == DEVICE_ANNCE_CLUSTER_ID;
        if is_device_announce {
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                cluster_id = format_args!("0x{cluster_id:04x}"),
                "Device_annce from remote-classified device — falling through to ZDO handler for re-interview"
            );
            // Do NOT return — fall through to the main match block below.
        } else if is_zdo {
            debug!(
                node_id = format_args!("0x{node_id:04x}"),
                cluster_id = format_args!("0x{cluster_id:04x}"),
                "ignoring ZDO frame from remote (not a ZCL command)"
            );
            return Some(NativeZigbeeEvent::IncomingMessage {
                node_id,
                cluster_id,
                payload: payload.to_vec(),
            });
        } else {
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                cluster_id = format_args!("0x{cluster_id:04x}"),
                "routing incoming message to remote command handler"
            );
            handle_remote_command(context, node_id, cluster_id, payload).await;
            return Some(NativeZigbeeEvent::IncomingMessage {
                node_id,
                cluster_id,
                payload: payload.to_vec(),
            });
        }
    }

    match cluster_id {
        DEVICE_ANNCE_CLUSTER_ID => {
            if let Some(announcement) = parse_device_announce(payload) {
                info!(
                    node_id = format_args!("0x{:04x}", announcement.node_id),
                    eui64 = %announcement.eui64,
                    "device announce received — starting discovery"
                );
                ensure_joined_device(context, announcement.node_id, announcement.eui64.clone());
                // Device_annce means the device just (re-)joined the network.
                // If it was previously classified (e.g. as Remote due to
                // misclassification), reset it so discovery can re-interview
                // from scratch.  This ensures ZDO responses (Active_EP_rsp,
                // Simple_Desc_rsp) won't be blocked by the sender_is_remote
                // filter above.
                if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == announcement.node_id) {
                    if device.interview_completed {
                        info!(
                            node_id = format_args!("0x{:04x}", announcement.node_id),
                            old_type = ?device.device_type,
                            old_endpoint = ?device.endpoint,
                            "device re-announced — resetting classification for fresh interview"
                        );
                        device.device_type = ZigbeeDeviceType::Unknown;
                        device.endpoint = None;
                        device.interview_completed = false;
                        device.interview_attempts = 0;
                        device.input_clusters = Vec::new();
                        device.output_clusters = Vec::new();
                    }
                }
                request_known_device_discovery(context, announcement.node_id).await;
                Some(NativeZigbeeEvent::DeviceAnnounced {
                    node_id: announcement.node_id,
                    eui64: announcement.eui64,
                })
            } else {
                None
            }
        }
        ACTIVE_EP_RSP_CLUSTER_ID => {
            if let Some(endpoints) = parse_active_ep_response(payload) {
                let mut ordered_endpoints = endpoints;
                ordered_endpoints.sort_by_key(|endpoint| if *endpoint == 242 { 1 } else { 0 });
                for endpoint in ordered_endpoints {
                    let _ = request_simple_descriptor(context, node_id, endpoint).await;
                }
            }
            None
        }
        SIMPLE_DESC_RSP_CLUSTER_ID => {
            if let Some(description) = parse_simple_desc_response(payload) {
                let classified_type = classify_device_type(&description);
                info!(
                    node_id = format_args!("0x{node_id:04x}"),
                    endpoint = description.endpoint,
                    profile_id = format_args!("0x{:04x}", description.profile_id),
                    device_id = format_args!("0x{:04x}", description.device_id),
                    input_clusters = ?description.input_clusters,
                    output_clusters = ?description.output_clusters,
                    device_type = ?classified_type,
                    "simple descriptor received — device classified"
                );
                let mut refresh_target = None;
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.connected = true;
                    device.reachable = true;
                    device.last_seen = Some(Instant::now());
                    device.failed_ping_cycles = 0;

                    let is_remote = classified_type == ZigbeeDeviceType::Remote;
                    let already_classified_as_lamp = device.device_type == ZigbeeDeviceType::Lamp;

                    // A Lamp endpoint always takes priority over a Remote
                    // endpoint.  Multi-endpoint devices like the Hue Lightstrip
                    // Plus (LCL001) expose both a utility endpoint (ep 1,
                    // classified Remote because it has On/Off in output clusters
                    // only) and the real light endpoint (ep 11).  If the Remote
                    // endpoint arrives first, the light endpoint must be allowed
                    // to correct the classification.  Conversely, if the Lamp
                    // endpoint was already stored, a subsequent Remote endpoint
                    // must NOT overwrite it.
                    let should_replace_endpoint = if is_remote && already_classified_as_lamp {
                        // Remote must NOT overwrite an existing Lamp.
                        false
                    } else if is_remote {
                        // First classification — accept the Remote.
                        true
                    } else {
                        (device.endpoint.is_none() && is_preferred_light_endpoint(&description))
                            || device.endpoint == Some(description.endpoint)
                            || (is_preferred_light_endpoint(&description)
                                && !device.input_clusters.contains(&ON_OFF_CLUSTER_ID)
                                && !device.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID)
                                && !device.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID))
                            // A Lamp endpoint always corrects a prior Remote
                            // misclassification, even if the current stored
                            // input_clusters happen to overlap.
                            || (is_preferred_light_endpoint(&description)
                                && device.device_type == ZigbeeDeviceType::Remote)
                    };

                    if should_replace_endpoint {
                        device.endpoint = Some(description.endpoint);
                        device.input_clusters = description.input_clusters.clone();
                        device.output_clusters = description.output_clusters.clone();
                        device.device_type = classified_type;
                        if is_remote {
                            device.supports_brightness = false;
                            device.supports_temperature = false;
                            device.has_color_control_cluster = false;
                            device.supports_xy_color = false;
                            info!(
                                node_id = format_args!("0x{node_id:04x}"),
                                eui64 = %device.eui64,
                                endpoint = description.endpoint,
                                "detected Zigbee remote / dimmer switch"
                            );
                        } else {
                            device.supports_brightness = description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID);
                            device.has_color_control_cluster = description.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID);
                            device.supports_temperature = device.supports_temperature && device.has_color_control_cluster;
                            // supports_xy_color stays false until we read colorCapabilities (0x400A)
                            // during refresh_device_state — don't set it from the cluster list alone.
                        }
                        device.interview_completed = true;
                        device.interview_attempts = 0;
                        if !is_remote {
                            refresh_target = Some(device.clone());
                        }
                    }
                }
                if let Some(target) = refresh_target {
                    let _ = refresh_device_state(context, &target).await;
                }
            }
            None
        }
        ON_OFF_CLUSTER_ID => {
            if payload.len() >= 8 && payload[2] == ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
                parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            } else if payload.len() >= 3 && payload[2] <= 0x02 {
                if let Some(value) = payload.last().copied() {
                    if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                        device.connected = true;
                        device.reachable = true;
                        device.last_seen = Some(Instant::now());
                        device.failed_ping_cycles = 0;
                        device.is_on = value != 0;
                    }
                }
            }
            None
        }
        LEVEL_CONTROL_CLUSTER_ID => {
            if payload.len() >= 8 && payload[2] == ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
                parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            } else if payload.len() >= 3 && payload[2] == 0x04 {
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.connected = true;
                    device.reachable = true;
                    device.last_seen = Some(Instant::now());
                    device.failed_ping_cycles = 0;
                    if let Some(level) = payload.get(3).copied() {
                        device.brightness = ((u16::from(level) * 100) / 254) as u8;
                        device.is_on = level > 0;
                    }
                }
            }
            None
        }
        COLOR_CONTROL_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            None
        }
        BASIC_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            None
        }
        _ => None,
    }
}

fn parse_zcl_read_attributes_response(
    context: &mut EzspContext,
    node_id: u16,
    cluster_id: u16,
    payload: &[u8],
) {
    if payload.len() < 3 || payload[2] != ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
        return;
    }

    debug!(
        node_id = format_args!("0x{node_id:04x}"),
        cluster_id = format_args!("0x{cluster_id:04x}"),
        payload = %hex_bytes(payload),
        "native zigbee read attributes response"
    );

    let mut offset = 3;
    while offset + 4 <= payload.len() {
        let attribute_id = u16::from(payload[offset]) | (u16::from(payload[offset + 1]) << 8);
        offset += 2;
        let status = payload[offset];
        offset += 1;
        if status != 0 || offset >= payload.len() {
            continue;
        }

        let data_type = payload[offset];
        offset += 1;
        let Some((value_len, value_bytes)) = parse_zcl_attribute_value(data_type, &payload[offset..]) else {
            break;
        };
        offset += value_len;

        if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
            device.connected = true;
            device.reachable = true;
            device.last_seen = Some(Instant::now());
            device.failed_ping_cycles = 0;
            match (cluster_id, attribute_id) {
                (ON_OFF_CLUSTER_ID, 0x0000) => {
                    if let Some(value) = value_bytes.first() {
                        device.is_on = *value != 0;
                    }
                }
                (LEVEL_CONTROL_CLUSTER_ID, 0x0000) => {
                    if let Some(value) = value_bytes.first() {
                        device.brightness = ((u16::from(*value) * 100) / 254) as u8;
                    }
                }
                (COLOR_CONTROL_CLUSTER_ID, 0x0003) if value_bytes.len() >= 2 => {
                    let raw = u16::from(value_bytes[0]) | (u16::from(value_bytes[1]) << 8);
                    device.color_x = Some(f32::from(raw) / 65535.0);
                }
                (COLOR_CONTROL_CLUSTER_ID, 0x0004) if value_bytes.len() >= 2 => {
                    let raw = u16::from(value_bytes[0]) | (u16::from(value_bytes[1]) << 8);
                    device.color_y = Some(f32::from(raw) / 65535.0);
                }
                (COLOR_CONTROL_CLUSTER_ID, 0x0007) if value_bytes.len() >= 2 => {
                    let raw = u16::from(value_bytes[0]) | (u16::from(value_bytes[1]) << 8);
                    let normalized = (((500_u16.saturating_sub(raw.min(500))) * 100) / (500 - 153)) as u8;
                    device.supports_temperature = true;
                    device.temperature = Some(normalized.min(100));
                }
                (COLOR_CONTROL_CLUSTER_ID, 0x0008) => {
                    if let Some(value) = value_bytes.first() {
                        device.color_mode = Some(*value);
                    }
                }
                // colorCapabilities (0x400A): 16-bit bitmap.
                // Bit 0 = Hue/Saturation, Bit 3 = XY, Bit 4 = Color Temperature.
                (COLOR_CONTROL_CLUSTER_ID, 0x400A) if value_bytes.len() >= 2 => {
                    let caps = u16::from(value_bytes[0]) | (u16::from(value_bytes[1]) << 8);
                    device.supports_xy_color = (caps & 0x08) != 0;
                    debug!(
                        node_id = format_args!("0x{:04x}", device.node_id),
                        caps = format_args!("0x{caps:04x}"),
                        supports_xy = device.supports_xy_color,
                        "parsed colorCapabilities"
                    );
                }
                (BASIC_CLUSTER_ID, 0x0004) => {
                    if let Ok(text) = String::from_utf8(value_bytes.to_vec()) {
                        device.manufacturer = Some(text);
                    }
                }
                (BASIC_CLUSTER_ID, 0x0005) => {
                    if let Ok(text) = String::from_utf8(value_bytes.to_vec()) {
                        device.model = Some(text);
                    }
                }
                _ => {}
            }
        }
    }
}

fn parse_zcl_attribute_value(data_type: u8, payload: &[u8]) -> Option<(usize, &[u8])> {
    match data_type {
        // 1-byte: boolean (0x10), 8-bit bitmap (0x18), uint8 (0x20), enum8 (0x30)
        0x10 | 0x18 | 0x20 | 0x30 => payload.first().map(|_| (1, &payload[..1])),
        // 2-byte: 16-bit bitmap (0x19), uint16 (0x21), enum16 (0x31)
        0x19 | 0x21 | 0x31 => (payload.len() >= 2).then_some((2, &payload[..2])),
        // length-prefixed octet string
        0x42 => {
            let len = *payload.first()? as usize;
            (payload.len() > len).then_some((1 + len, &payload[1..1 + len]))
        }
        _ => None,
    }
}

fn parse_active_ep_response(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 5 {
        return None;
    }
    let status = payload[1];
    if status != 0 {
        return None;
    }
    let count = payload[4] as usize;
    if payload.len() < 5 + count {
        return None;
    }
    Some(payload[5..5 + count].to_vec())
}

struct DeviceAnnouncement {
    node_id: u16,
    eui64: String,
}

fn parse_device_announce(payload: &[u8]) -> Option<DeviceAnnouncement> {
    if payload.len() < 11 {
        return None;
    }

    let node_id = u16::from(payload[1]) | (u16::from(payload[2]) << 8);
    // ZDP sends the IEEE address in little-endian (LSB first) byte order.
    // MacAddr8::from([u8; 8]) stores bytes as-is, but format_eui64 (and the
    // rest of the codebase via FromLeStream) expects big-endian (MSB first).
    // Reverse to match the convention used in ChildJoin / TrustCenterJoin.
    let mut eui64_bytes = [0_u8; 8];
    eui64_bytes.copy_from_slice(&payload[3..11]);
    eui64_bytes.reverse();

    Some(DeviceAnnouncement {
        node_id,
        eui64: format_eui64(Eui64::from(eui64_bytes)),
    })
}

fn build_on_off_command_payload(enabled: bool, sequence: u8) -> Vec<u8> {
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        if enabled {
            ZCL_ON_OFF_COMMAND_ON
        } else {
            ZCL_ON_OFF_COMMAND_OFF
        },
    ]
}

fn brightness_percent_to_level(brightness: u8) -> u8 {
    ((u16::from(brightness.min(100)) * 254) / 100).max(1) as u8
}

fn build_brightness_command_payload(brightness: u8, sequence: u8) -> Vec<u8> {
    let level = brightness_percent_to_level(brightness);
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL,
        level,
        0x00,
        0x00,
    ]
}

fn temperature_percent_to_mireds(temperature: u8) -> u16 {
    500_u16.saturating_sub((u16::from(temperature.min(100)) * (500 - 153)) / 100)
}

fn build_color_temperature_command_payload(temperature: u8, sequence: u8) -> Vec<u8> {
    let raw_temperature = temperature_percent_to_mireds(temperature);
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
        (raw_temperature & 0xff) as u8,
        (raw_temperature >> 8) as u8,
        0x00,
        0x00,
    ]
}

/// Build a ZCL `Move to Color` (0x07) payload for the Color Control cluster (0x0300).
///
/// `x` and `y` are CIE 1931 chromaticity coordinates in the 0.0–1.0 range.
/// They are scaled to uint16 (0–65535) for the ZCL wire format.
/// Transition time is set to 0 (immediate).
fn build_color_xy_command_payload(x: f32, y: f32, sequence: u8) -> Vec<u8> {
    let raw_x = (x.clamp(0.0, 1.0) * 65535.0) as u16;
    let raw_y = (y.clamp(0.0, 1.0) * 65535.0) as u16;
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR,
        (raw_x & 0xff) as u8,
        (raw_x >> 8) as u8,
        (raw_y & 0xff) as u8,
        (raw_y >> 8) as u8,
        0x00, // transition time low
        0x00, // transition time high
    ]
}

/// Build a ZCL `Trigger Effect` (0x40) payload for the Identify cluster (0x0003).
///
/// Standard ZCL effects: blink=0x00, breathe=0x01, okay=0x02,
/// channel_change=0x0B, finish_effect=0xFE, stop_effect=0xFF.
fn build_identify_trigger_effect_payload(effect_id: u8, sequence: u8) -> Vec<u8> {
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_IDENTIFY_TRIGGER_EFFECT_COMMAND_ID,
        effect_id,
        0x00, // effect variant (default)
    ]
}

/// Build a Philips manufacturer-specific `multiColor` command payload for
/// the Hue effects cluster (0xFC03, manufacturer code 0x100B).
///
/// `start_payload` should be either:
/// - Start: `[0x21, 0x00, 0x01, effectId]` (candle=1, fireplace=2, colorloop=3, etc.)
/// - Stop:  `[0x20, 0x00, 0x00]`
fn build_philips_hue_effect_payload(start_payload: &[u8], sequence: u8) -> Vec<u8> {
    let mut payload = vec![
        ZCL_MANU_CLUSTER_COMMAND_FRAME_CONTROL,
        (PHILIPS_MANUFACTURER_CODE & 0xff) as u8,
        (PHILIPS_MANUFACTURER_CODE >> 8) as u8,
        sequence,
        PHILIPS_MULTI_COLOR_COMMAND_ID,
    ];
    payload.extend_from_slice(start_payload);
    payload
}

struct SimpleDescriptor {
    endpoint: u8,
    profile_id: u16,
    device_id: u16,
    input_clusters: Vec<u16>,
    output_clusters: Vec<u16>,
}

fn parse_simple_desc_response(payload: &[u8]) -> Option<SimpleDescriptor> {
    if payload.len() < 8 {
        return None;
    }
    let status = payload[1];
    if status != 0 {
        return None;
    }

    let descriptor_length = payload[4] as usize;
    if payload.len() < 5 + descriptor_length || descriptor_length < 8 {
        return None;
    }

    let descriptor = &payload[5..5 + descriptor_length];
    let endpoint = descriptor[0];
    let profile_id = u16::from(descriptor[1]) | (u16::from(descriptor[2]) << 8);
    let device_id = u16::from(descriptor[3]) | (u16::from(descriptor[4]) << 8);
    let mut offset = 6;
    if descriptor.len() <= offset {
        return None;
    }
    let input_count = descriptor[offset] as usize;
    offset += 1;
    let mut input_clusters = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        if descriptor.len() < offset + 2 {
            return None;
        }
        input_clusters.push(u16::from(descriptor[offset]) | (u16::from(descriptor[offset + 1]) << 8));
        offset += 2;
    }
    if descriptor.len() <= offset {
        return None;
    }
    let output_count = descriptor[offset] as usize;
    offset += 1;
    let mut output_clusters = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        if descriptor.len() < offset + 2 {
            return None;
        }
        output_clusters.push(u16::from(descriptor[offset]) | (u16::from(descriptor[offset + 1]) << 8));
        offset += 2;
    }

    Some(SimpleDescriptor {
        endpoint,
        profile_id,
        device_id,
        input_clusters,
        output_clusters,
    })
}

fn is_preferred_light_endpoint(description: &SimpleDescriptor) -> bool {
    description.profile_id == HOME_AUTOMATION_PROFILE_ID
        && (description.input_clusters.contains(&ON_OFF_CLUSTER_ID)
            || description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID)
            || description.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID))
}

///
/// A remote / dimmer switch is identified by:
///   - ZHA profile (0x0104)
///   - device_id in the controller range (0x0820 = Non-Color Controller, 0x0830 = Color Controller,
///     or 0x0840 = Scene Controller), OR
///   - output clusters containing On/Off and/or Level Control while input clusters do NOT
///     contain On/Off (i.e. the device *sends* light commands but doesn't *receive* them)
fn classify_device_type(description: &SimpleDescriptor) -> ZigbeeDeviceType {
    if description.profile_id != HOME_AUTOMATION_PROFILE_ID {
        return ZigbeeDeviceType::Unknown;
    }

    // Well-known ZHA controller device IDs.
    let is_controller_device = matches!(description.device_id, 0x0820 | 0x0830 | 0x0840 | 0x0006);

    // Heuristic: the device has On/Off or Level Control in its *output* clusters
    // (meaning it sends those commands) but NOT in its *input* clusters.
    let sends_light_commands = description.output_clusters.contains(&ON_OFF_CLUSTER_ID)
        || description.output_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID);
    let receives_light_commands = description.input_clusters.contains(&ON_OFF_CLUSTER_ID)
        || description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID);

    if is_controller_device || (sends_light_commands && !receives_light_commands) {
        return ZigbeeDeviceType::Remote;
    }

    if is_preferred_light_endpoint(description) {
        return ZigbeeDeviceType::Lamp;
    }

    ZigbeeDeviceType::Unknown
}

async fn drain_pending_requests(command_rx: &mut mpsc::Receiver<DriverRequest>, error: AppError) {
    let error_message = error.to_string();
    // Use try_recv to drain only already-queued requests.  recv().await would
    // block forever if the sender side (NativeZigbeeRuntime) is still alive.
    while let Ok(request) = command_rx.try_recv() {
        let _ = request
            .reply_tx
            .send(Err(AppError::service_unavailable(error_message.clone())));
    }
}

async fn set_status(
    status: &Arc<RwLock<NativeZigbeeStatus>>,
    message: Option<String>,
    last_error: Option<String>,
) {
    let mut guard = status.write().await;
    guard.message = message;
    guard.last_error = last_error;
}

async fn sync_status_devices(
    status: &Arc<RwLock<NativeZigbeeStatus>>,
    devices: &[DiscoveredDevice],
) {
    let mut guard = status.write().await;
    guard.devices = devices
        .iter()
        .map(|device| NativeDiscoveredDevice {
            id: normalize_device_id(&device.eui64, device.node_id),
            node_id: device.node_id,
            eui64: device.eui64.clone(),
            endpoint: device.endpoint,
            input_clusters: device.input_clusters.clone(),
            output_clusters: device.output_clusters.clone(),
            supports_brightness: device.supports_brightness,
            supports_temperature: device.supports_temperature,
            supports_color: device.supports_xy_color,
            device_type: device.device_type,
            connected: device.connected,
            reachable: device.reachable,
            is_on: device.is_on,
            brightness: device.brightness,
            temperature: device.temperature,
            color_x: device.color_x,
            color_y: device.color_y,
            color_mode: device.color_mode,
            model: device.model.clone(),
            manufacturer: device.manufacturer.clone(),
            last_seen: device.last_seen.map(|instant| {
                let elapsed = instant.elapsed();
                let wall_clock = Utc::now() - chrono::Duration::from_std(elapsed).unwrap_or_default();
                wall_clock.to_rfc3339()
            }),
        })
        .collect();
}

fn network_state_label(state: EmberNetworkStatus) -> &'static str {
    match state {
        EmberNetworkStatus::NoNetwork => "no-network",
        EmberNetworkStatus::JoiningNetwork => "joining",
        EmberNetworkStatus::JoinedNetwork => "joined",
        EmberNetworkStatus::JoinedNetworkNoParent => "joined-no-parent",
        EmberNetworkStatus::LeavingNetwork => "leaving",
    }
}

fn ember_network_state_to_driver_state(state: EmberNetworkStatus) -> DriverNetworkState {
    match state {
        EmberNetworkStatus::JoinedNetwork => DriverNetworkState::Joined,
        EmberNetworkStatus::JoiningNetwork
        | EmberNetworkStatus::NoNetwork
        | EmberNetworkStatus::JoinedNetworkNoParent
        | EmberNetworkStatus::LeavingNetwork => DriverNetworkState::NoNetwork,
    }
}

fn network_state_from_stack_status(status: ezsp::ember::Status) -> String {
    match status {
        ezsp::ember::Status::NetworkUp => "joined".to_string(),
        ezsp::ember::Status::NetworkDown => "down".to_string(),
        ezsp::ember::Status::NetworkOpened => "open-for-join".to_string(),
        ezsp::ember::Status::NetworkClosed => "join-closed".to_string(),
        other => other.to_string(),
    }
}

fn map_ezsp_error(context: &'static str) -> impl FnOnce(ezsp::Error) -> AppError {
    move |error| AppError::service_unavailable(format!("EZSP {context} failed: {error}"))
}

fn format_eui64(eui64: Eui64) -> String {
    eui64
        .into_array()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn seed_known_device(device: NativeKnownDevice) -> DiscoveredDevice {
    DiscoveredDevice {
        node_id: device.node_id,
        eui64: device.eui64,
        endpoint: device.endpoint,
        has_color_control_cluster: device.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID),
        supports_xy_color: false, // will be set when colorCapabilities (0x400A) is read
        device_type: device.device_type,
        input_clusters: device.input_clusters,
        output_clusters: device.output_clusters,
        supports_brightness: device.supports_brightness,
        supports_temperature: device.supports_temperature,
        is_on: false,
        brightness: 0,
        temperature: None,
        color_x: None,
        color_y: None,
        color_mode: None,
        interview_completed: device.endpoint.is_some(),
        model: device.model,
        manufacturer: device.manufacturer,
        connected: false,
        reachable: false,
        interview_attempts: 0,
        last_seen: None,
        desired_brightness: None,
        desired_temperature: None,
        desired_color_x: None,
        desired_color_y: None,
        desired_state_applied: true,
        last_discovery_at: None,
        failed_ping_cycles: 0,
    }
}

fn normalize_device_id(eui64: &str, node_id: u16) -> String {
    let normalized = eui64
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();

    if normalized.is_empty() {
        format!("{:016x}", node_id)
    } else {
        normalized
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL, ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
        ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL, ZCL_ON_OFF_COMMAND_OFF, ZCL_ON_OFF_COMMAND_ON,
        build_brightness_command_payload, build_color_temperature_command_payload,
        build_on_off_command_payload, brightness_percent_to_level, next_sequence_for_device,
        should_probe_active_endpoints, temperature_percent_to_mireds,
    };
    use std::collections::HashMap;

    #[test]
    fn on_off_payload_uses_cluster_command_frame() {
        assert_eq!(
            build_on_off_command_payload(false, 0x34),
            vec![ZCL_CLUSTER_COMMAND_FRAME_CONTROL, 0x34, ZCL_ON_OFF_COMMAND_OFF]
        );
        assert_eq!(
            build_on_off_command_payload(true, 0x35),
            vec![ZCL_CLUSTER_COMMAND_FRAME_CONTROL, 0x35, ZCL_ON_OFF_COMMAND_ON]
        );
    }

    #[test]
    fn brightness_payload_maps_percent_to_move_to_level() {
        assert_eq!(brightness_percent_to_level(0), 1);
        assert_eq!(brightness_percent_to_level(50), 127);
        assert_eq!(brightness_percent_to_level(100), 254);
        assert_eq!(
            build_brightness_command_payload(50, 0x22),
            vec![
                ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
                0x22,
                ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL,
                127,
                0x00,
                0x00,
            ]
        );
    }

    #[test]
    fn color_temperature_payload_maps_percent_to_mireds() {
        assert_eq!(temperature_percent_to_mireds(0), 500);
        assert_eq!(temperature_percent_to_mireds(100), 153);
        assert_eq!(temperature_percent_to_mireds(50), 327);
        assert_eq!(
            build_color_temperature_command_payload(50, 0x44),
            vec![
                ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
                0x44,
                ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
                0x47,
                0x01,
                0x00,
                0x00,
            ]
        );
    }

    #[test]
    fn device_sequences_are_independent() {
        let mut per_device = HashMap::new();
        let mut next_global = 1;

        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 1);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 2);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x6cce), 2);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 3);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x6cce), 3);
    }

    #[test]
    fn startup_probe_only_runs_for_uninterviewed_devices() {
        let base = super::DiscoveredDevice {
            node_id: 0x8a4c,
            eui64: "ab:1e:d2:06:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8],
            output_clusters: vec![25],
            device_type: super::ZigbeeDeviceType::Lamp,
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: false,
            supports_xy_color: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            color_x: None,
            color_y: None,
            color_mode: None,
            interview_completed: true,
            model: Some("LWA001".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_color_x: None,
            desired_color_y: None,
            desired_state_applied: true,
            last_discovery_at: None,
            failed_ping_cycles: 0,
        };

        assert!(!should_probe_active_endpoints(&base));

        let mut missing_endpoint = base.clone();
        missing_endpoint.endpoint = None;
        assert!(should_probe_active_endpoints(&missing_endpoint));

        let mut missing_clusters = base;
        missing_clusters.input_clusters.clear();
        assert!(should_probe_active_endpoints(&missing_clusters));
    }

    #[test]
    fn announce_reuses_known_endpoint_without_reprobe() {
        let known = super::DiscoveredDevice {
            node_id: 0x2e34,
            eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8],
            output_clusters: vec![25],
            device_type: super::ZigbeeDeviceType::Lamp,
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: false,
            supports_xy_color: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            color_x: None,
            color_y: None,
            color_mode: None,
            interview_completed: true,
            model: Some("LTG002".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_color_x: None,
            desired_color_y: None,
            desired_state_applied: true,
            last_discovery_at: None,
            failed_ping_cycles: 0,
        };

        assert!(!should_probe_active_endpoints(&known));
    }

    #[test]
    fn color_control_cluster_does_not_imply_temperature_support() {
        let mut device = super::DiscoveredDevice {
            node_id: 0x2e34,
            eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8, super::COLOR_CONTROL_CLUSTER_ID],
            output_clusters: vec![25],
            device_type: super::ZigbeeDeviceType::Lamp,
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: true,
            supports_xy_color: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            color_x: None,
            color_y: None,
            color_mode: None,
            interview_completed: true,
            model: Some("LTG002".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_color_x: None,
            desired_color_y: None,
            desired_state_applied: true,
            last_discovery_at: None,
            failed_ping_cycles: 0,
        };

        device.supports_temperature = device.supports_temperature && device.has_color_control_cluster;

        assert!(device.has_color_control_cluster);
        assert!(!device.supports_temperature);
    }
}

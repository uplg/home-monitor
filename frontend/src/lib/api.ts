const API_BASE = "/api";

interface ApiOptions {
  method?: "GET" | "POST" | "PUT" | "DELETE";
  body?: unknown;
  /** Skip auto-refresh on 401 (used internally by the refresh call itself). */
  _skipRefresh?: boolean;
}

// Mutex to prevent concurrent refresh attempts.
let refreshPromise: Promise<void> | null = null;

async function attemptRefresh(): Promise<void> {
  const response = await fetch(`${API_BASE}/auth/refresh`, {
    method: "POST",
    credentials: "same-origin",
  });
  if (!response.ok) {
    throw new Error("Refresh failed");
  }
}

export async function api<T>(endpoint: string, options: ApiOptions = {}): Promise<T> {
  const headers: HeadersInit = {
    "Content-Type": "application/json",
  };

  const doFetch = () =>
    fetch(`${API_BASE}${endpoint}`, {
      method: options.method || "GET",
      headers,
      body: options.body ? JSON.stringify(options.body) : undefined,
      credentials: "same-origin",
    });

  let response = await doFetch();

  // On 401, attempt a single transparent refresh and retry the original request.
  if (response.status === 401 && !options._skipRefresh) {
    try {
      // Serialize concurrent refresh attempts — only one in-flight at a time.
      if (!refreshPromise) {
        refreshPromise = attemptRefresh().finally(() => {
          refreshPromise = null;
        });
      }
      await refreshPromise;

      // Retry the original request with the new access token cookie.
      response = await doFetch();
    } catch {
      // Refresh failed — fall through to the normal error handling below,
      // which will surface the original 401.
    }
  }

  if (!response.ok) {
    let errorMessage = `API request failed (${response.status})`;
    try {
      const data = await response.json();
      if (data.error) {
        errorMessage = data.error;
      }
    } catch {
      // Response body is not JSON (empty, HTML, etc.) — keep the default message.
    }
    throw new Error(errorMessage);
  }

  // Handle 204 No Content or empty bodies gracefully.
  const text = await response.text();
  if (!text) {
    return {} as T;
  }

  return JSON.parse(text);
}

// Auth API
export interface LoginResponse {
  success: boolean;
  user: {
    id: string;
    username: string;
    role: string;
  };
}

export interface VerifyResponse {
  success: boolean;
  user?: {
    id: string;
    username: string;
    role: string;
  };
  error?: string;
}

export const authApi = {
  login: (username: string, password: string) =>
    api<LoginResponse>("/auth/login", {
      method: "POST",
      body: { username, password },
    }),

  verify: () => api<VerifyResponse>("/auth/verify", { method: "POST" }),

  logout: () => api("/auth/logout", { method: "POST" }),

  refresh: () =>
    api<{ success: boolean; user: { id: string; username: string; role: string } }>(
      "/auth/refresh",
      { method: "POST", _skipRefresh: true },
    ),
};

// Device types
export interface Device {
  id: string;
  name: string;
  type: "feeder" | "litter-box" | "fountain" | "unknown";
  product_name?: string;
  model?: string;
  ip?: string;
  version?: string;
  connected: boolean;
  last_data?: unknown;
  parsed_data?: unknown;
}

export interface DevicesResponse {
  success: boolean;
  devices: Device[];
  total: number;
  message: string;
}

export interface DeviceStatusResponse {
  success: boolean;
  device: {
    id: string;
    name: string;
    type: string;
    connected: boolean;
  };
  parsed_status: unknown;
  raw_dps?: unknown;
  message: string;
}

// Feeder types
export interface FeederStatus {
  food_level: string;
  battery_level?: number;
  last_feed_time?: string;
  portions_today?: number;
}

export interface MealPlanEntry {
  days_of_week: string[];
  time: string;
  portion: number;
  status: "Enabled" | "Disabled";
}

export interface MealPlanResponse {
  success: boolean;
  device: { id: string; name: string };
  decoded: MealPlanEntry[] | null;
  meal_plan: string | null;
  message: string;
}

// Fountain types
export interface FountainStatus {
  power: boolean;
  uv_enabled: boolean;
  eco_mode: boolean;
  water_level: string;
  filter_life: number;
  pump_time: number;
}

// Litter box types
export interface LitterBoxStatus {
  clean_delay: number;
  sleep_mode: {
    enabled: boolean;
    start_time: string;
    end_time: string;
  };
  child_lock: boolean;
  kitten_mode: boolean;
  lighting: boolean;
  sand_level: number;
  last_use?: string;
}

// Devices API
export const devicesApi = {
  list: () => api<DevicesResponse>("/devices"),

  connect: (deviceId: string) => api(`/devices/${deviceId}/connect`, { method: "POST" }),

  connectAll: () => api("/devices/connect", { method: "POST" }),

  disconnect: (deviceId: string) => api(`/devices/${deviceId}/disconnect`, { method: "POST" }),

  disconnectAll: () => api("/devices/disconnect", { method: "POST" }),

  status: (deviceId: string) => api<DeviceStatusResponse>(`/devices/${deviceId}/status`),
};

// Feeder API
export const feederApi = {
  status: (deviceId: string) => api<DeviceStatusResponse>(`/devices/${deviceId}/feeder/status`),

  feed: (deviceId: string, portion: number = 1) =>
    api(`/devices/${deviceId}/feeder/feed`, {
      method: "POST",
      body: { portion },
    }),

  getMealPlan: (deviceId: string) => api<MealPlanResponse>(`/devices/${deviceId}/feeder/meal-plan`),

  setMealPlan: (deviceId: string, mealPlan: MealPlanEntry[]) =>
    api(`/devices/${deviceId}/feeder/meal-plan`, {
      method: "POST",
      body: { meal_plan: mealPlan },
    }),
};

// Fountain API
export const fountainApi = {
  status: (deviceId: string) => api<DeviceStatusResponse>(`/devices/${deviceId}/fountain/status`),

  power: (deviceId: string, enabled: boolean) =>
    api(`/devices/${deviceId}/fountain/power`, {
      method: "POST",
      body: { enabled },
    }),

  resetWater: (deviceId: string) =>
    api(`/devices/${deviceId}/fountain/reset/water`, { method: "POST" }),

  resetFilter: (deviceId: string) =>
    api(`/devices/${deviceId}/fountain/reset/filter`, { method: "POST" }),

  resetPump: (deviceId: string) =>
    api(`/devices/${deviceId}/fountain/reset/pump`, { method: "POST" }),

  setUV: (deviceId: string, enabled: boolean) =>
    api(`/devices/${deviceId}/fountain/uv`, {
      method: "POST",
      body: { enabled },
    }),

  // Mode éco: 1 = mode 1, 2 = mode 2
  setEcoMode: (deviceId: string, mode: number) =>
    api(`/devices/${deviceId}/fountain/eco-mode`, {
      method: "POST",
      body: { mode },
    }),
};

// Litter box API
export const litterBoxApi = {
  status: (deviceId: string) => api<DeviceStatusResponse>(`/devices/${deviceId}/litter-box/status`),

  clean: (deviceId: string) => api(`/devices/${deviceId}/litter-box/clean`, { method: "POST" }),

  settings: (
    deviceId: string,
    settings: {
      clean_delay?: number;
      sleep_mode?: {
        enabled?: boolean;
        start_time?: string;
        end_time?: string;
      };
      preferences?: {
        child_lock?: boolean;
        kitten_mode?: boolean;
        lighting?: boolean;
        prompt_sound?: boolean;
        automatic_homing?: boolean;
      };
      actions?: {
        reset_sand_level?: boolean;
        reset_factory_settings?: boolean;
      };
    },
  ) =>
    api(`/devices/${deviceId}/litter-box/settings`, {
      method: "POST",
      body: settings,
    }),
};

// Hue Lamp types
export interface HueLampState {
  isOn: boolean;
  brightness: number;
  temperature: number | null;
  temperatureMin: number | null;
  temperatureMax: number | null;
}

export interface HueLamp {
  id: string;
  name: string;
  address: string;
  model: string | null;
  manufacturer: string;
  firmware: string | null;
  connected: boolean;
  connecting: boolean;
  reachable: boolean;
  state: HueLampState;
  lastSeen: string | null;
}

export interface HueLampsResponse {
  success: boolean;
  lamps: HueLamp[];
  total: number;
  connected: number;
  reachable: number;
  message: string;
}

export interface HueLampStatusResponse {
  success: boolean;
  lamp?: HueLamp;
  message?: string;
  error?: string;
}

export interface HueLampActionResponse {
  success: boolean;
  state?: {
    isOn: boolean;
    brightness: number;
  };
  message?: string;
  error?: string;
}

// Hue Lamps API
export const hueLampsApi = {
  list: () => api<HueLampsResponse>("/hue-lamps"),

  scan: () => api("/hue-lamps/scan", { method: "POST" }),

  stats: () =>
    api<{
      success: boolean;
      total: number;
      connected: number;
      reachable: number;
      disabled?: boolean;
      message?: string;
    }>("/hue-lamps/stats"),

  connectAll: () => api("/hue-lamps/connect", { method: "POST" }),

  disconnectAll: () => api("/hue-lamps/disconnect", { method: "POST" }),

  status: (lampId: string) => api<HueLampStatusResponse>(`/hue-lamps/${lampId}`),

  connect: (lampId: string) => api(`/hue-lamps/${lampId}/connect`, { method: "POST" }),

  disconnect: (lampId: string) => api(`/hue-lamps/${lampId}/disconnect`, { method: "POST" }),

  power: (lampId: string, enabled: boolean) =>
    api<HueLampActionResponse>(`/hue-lamps/${lampId}/power`, {
      method: "POST",
      body: { enabled },
    }),

  brightness: (lampId: string, brightness: number) =>
    api<HueLampActionResponse>(`/hue-lamps/${lampId}/brightness`, {
      method: "POST",
      body: { brightness },
    }),

  temperature: (lampId: string, temperature: number) =>
    api<HueLampActionResponse>(`/hue-lamps/${lampId}/temperature`, {
      method: "POST",
      body: { temperature },
    }),

  state: (lampId: string, isOn: boolean, brightness?: number) =>
    api<HueLampActionResponse>(`/hue-lamps/${lampId}/state`, {
      method: "POST",
      body: { isOn, brightness },
    }),

  rename: (lampId: string, name: string) =>
    api(`/hue-lamps/${lampId}/rename`, {
      method: "POST",
      body: { name },
    }),

  blacklist: (lampId: string) =>
    api(`/hue-lamps/${lampId}/blacklist`, {
      method: "POST",
    }),
};

export interface ZigbeeLampState {
  isOn: boolean;
  brightness: number;
  temperature: number | null;
  temperatureMin: number | null;
  temperatureMax: number | null;
  colorX: number | null;
  colorY: number | null;
  colorMode: number | null;
}

export interface ZigbeeLamp {
  id: string;
  name: string;
  address: string;
  friendlyName: string;
  linkQuality: number | null;
  interviewCompleted: boolean;
  model: string | null;
  manufacturer: string;
  firmware: string | null;
  connected: boolean;
  reachable: boolean;
  supportsBrightness: boolean;
  supportsTemperature: boolean;
  supportsColor: boolean;
  state: ZigbeeLampState;
  lastSeen: string | null;
}

export interface ZigbeeLampsResponse {
  success: boolean;
  lamps: ZigbeeLamp[];
  total: number;
  connected: number;
  reachable: number;
  message: string;
}

export interface ZigbeeLampStatusResponse {
  success: boolean;
  lamp?: ZigbeeLamp;
  message?: string;
  error?: string;
}

export interface ZigbeePairingStatus {
  active: boolean;
  remainingSeconds: number;
  permitJoinSeconds: number;
  message?: string;
}

export interface ZigbeePairingResponse {
  success: boolean;
  pairing: ZigbeePairingStatus;
  message: string;
}

export interface ZigbeeLampActionResponse {
  success: boolean;
  state?: ZigbeeLampState;
  message?: string;
  error?: string;
}

export interface ZigbeeSimpleResponse {
  success: boolean;
  message: string;
}

export const zigbeeLampsApi = {
  list: () => api<ZigbeeLampsResponse>("/zigbee/lamps"),

  stats: () =>
    api<{
      success: boolean;
      total: number;
      connected: number;
      reachable: number;
      disabled?: boolean;
      message?: string;
    }>("/zigbee/lamps/stats"),

  status: (lampId: string) => api<ZigbeeLampStatusResponse>(`/zigbee/lamps/${lampId}`),

  pairingStatus: () => api<ZigbeePairingResponse>("/zigbee/lamps/pairing/status"),

  startPairing: () => api<ZigbeePairingResponse>("/zigbee/lamps/pairing/start", { method: "POST" }),

  stopPairing: () => api<ZigbeePairingResponse>("/zigbee/lamps/pairing/stop", { method: "POST" }),

  touchlinkScan: () =>
    api<ZigbeeSimpleResponse>("/zigbee/lamps/pairing/touchlink", { method: "POST" }),

  power: (lampId: string, enabled: boolean) =>
    api<ZigbeeLampActionResponse>(`/zigbee/lamps/${lampId}/power`, {
      method: "POST",
      body: { enabled },
    }),

  brightness: (lampId: string, brightness: number) =>
    api<ZigbeeLampActionResponse>(`/zigbee/lamps/${lampId}/brightness`, {
      method: "POST",
      body: { brightness },
    }),

  temperature: (lampId: string, temperature: number) =>
    api<ZigbeeLampActionResponse>(`/zigbee/lamps/${lampId}/temperature`, {
      method: "POST",
      body: { temperature },
    }),

  color: (lampId: string, x: number, y: number) =>
    api<ZigbeeLampActionResponse>(`/zigbee/lamps/${lampId}/color`, {
      method: "POST",
      body: { x, y },
    }),

  effect: (lampId: string, effect: string) =>
    api<ZigbeeLampActionResponse>(`/zigbee/lamps/${lampId}/effect`, {
      method: "POST",
      body: { effect },
    }),

  rename: (lampId: string, name: string) =>
    api<ZigbeeSimpleResponse>(`/zigbee/lamps/${lampId}/rename`, {
      method: "POST",
      body: { name },
    }),
};

// Meross Smart Plug types
export interface MerossPlug {
  id: string;
  name: string;
  ip: string;
  isOnline: boolean;
  isOn: boolean;
  lastPing: number;
}

export interface MerossPlugsResponse {
  success: boolean;
  devices: MerossPlug[];
  total: number;
  message: string;
}

export interface MerossPlugStatus {
  online: boolean;
  on: boolean;
  electricity: {
    voltage: number;
    current: number;
    power: number;
  } | null;
  hardware: {
    type: string;
    version: string;
    chipType: string;
    uuid: string;
    mac: string;
  } | null;
  firmware: {
    version: string;
    compileTime: string;
    innerIp: string;
  } | null;
  wifi: {
    signal: number | null;
  };
  lastUpdate: number;
}

export interface MerossPlugStatusResponse {
  success: boolean;
  device: { id: string; name: string };
  status: MerossPlugStatus;
  message: string;
}

export interface MerossElectricityResponse {
  success: boolean;
  device: { id: string; name: string };
  electricity: {
    voltage: string;
    current: string;
    power: string;
    raw: {
      channel: number;
      current: number;
      voltage: number;
      power: number;
      config?: { voltageRatio: number; electricityRatio: number };
    };
  };
  message: string;
}

export interface MerossToggleResponse {
  success: boolean;
  device: { id: string; name: string };
  on: boolean;
  message: string;
}

export interface MerossConsumptionResponse {
  success: boolean;
  device: { id: string; name: string };
  consumption: Array<{ date: string; time: number; value: number }>;
  summary: { days: number; totalWh: number; totalKwh: number };
  message: string;
}

// Meross Smart Plugs API
export const merossApi = {
  list: () => api<MerossPlugsResponse>("/meross"),

  status: (deviceId: string) => api<MerossPlugStatusResponse>(`/meross/${deviceId}/status`),

  electricity: (deviceId: string) =>
    api<MerossElectricityResponse>(`/meross/${deviceId}/electricity`),

  toggle: (deviceId: string, on: boolean) =>
    api<MerossToggleResponse>(`/meross/${deviceId}/toggle`, {
      method: "POST",
      body: { on },
    }),

  turnOn: (deviceId: string) =>
    api<MerossToggleResponse>(`/meross/${deviceId}/on`, { method: "POST" }),

  turnOff: (deviceId: string) =>
    api<MerossToggleResponse>(`/meross/${deviceId}/off`, { method: "POST" }),

  consumption: (deviceId: string) =>
    api<MerossConsumptionResponse>(`/meross/${deviceId}/consumption`),

  dnd: (deviceId: string, enabled: boolean) =>
    api(`/meross/${deviceId}/dnd`, {
      method: "POST",
      body: { enabled },
    }),
};

export interface BroadlinkDevice {
  host: string;
  mac: string;
  modelCode: number;
  friendlyModel: string;
  friendlyType: string;
  name: string;
  isLocked: boolean;
  kind: string;
  supportsLearning: boolean;
}

export interface BroadlinkCode {
  id: string;
  name: string;
  brand: string | null;
  model: string | null;
  command: string;
  packetBase64: string;
  packetLength: number;
  tags: string[];
  createdAt: string;
  updatedAt: string;
}

export interface BroadlinkDiscoverResponse {
  success: boolean;
  devices: BroadlinkDevice[];
  total: number;
  message: string;
}

export interface BroadlinkCodesResponse {
  success: boolean;
  codes: BroadlinkCode[];
  total: number;
  message: string;
}

export interface BroadlinkSendResponse {
  success: boolean;
  result: {
    host: string;
    codeId?: string;
    command?: string;
    packetLength: number;
  };
  message: string;
}

export interface BroadlinkClimateSettings {
  mode: string;
  temperature: number;
  fan: string;
  vane: string;
  econo: boolean;
  stopInMinutes: number | null;
}

export interface BroadlinkClimateState {
  power: boolean;
  lastCommand: string;
  lastOnCommand: string | null;
  /** Parsed form of lastOnCommand, provided by the backend. */
  settings: BroadlinkClimateSettings | null;
  host: string;
  model: string | null;
  updatedAt: string;
}

export interface BroadlinkClimateStateResponse {
  success: boolean;
  state: BroadlinkClimateState | null;
  message: string;
}

export const broadlinkApi = {
  discover: (localIp?: string, forceRefresh = false) =>
    api<BroadlinkDiscoverResponse>(
      `/broadlink/discover${buildBroadlinkDiscoverQuery(localIp, forceRefresh)}`,
    ),

  listCodes: () => api<BroadlinkCodesResponse>("/broadlink/codes"),

  listMitsubishiCodes: (model?: string) =>
    api<BroadlinkCodesResponse>(
      `/broadlink/mitsubishi/codes${model ? `?model=${encodeURIComponent(model)}` : ""}`,
    ),

  getMitsubishiState: () => api<BroadlinkClimateStateResponse>("/broadlink/mitsubishi/state"),

  sendMitsubishiCommand: (host: string, command: string, model?: string, localIp?: string) =>
    api<BroadlinkSendResponse>("/broadlink/mitsubishi/send", {
      method: "POST",
      body: {
        host,
        command,
        model,
        localIp,
      },
    }),
};

function buildBroadlinkDiscoverQuery(localIp?: string, forceRefresh = false) {
  const params = new URLSearchParams();
  if (localIp) params.set("localIp", localIp);
  if (forceRefresh) params.set("forceRefresh", "true");
  const query = params.toString();
  return query ? `?${query}` : "";
}

// Tempo types
export interface TempoTarifs {
  blue: { hc: number; hp: number };
  white: { hc: number; hp: number };
  red: { hc: number; hp: number };
  dateDebut: string;
}

export interface TempoData {
  success: boolean;
  today?: {
    date: string;
    color: "BLUE" | "WHITE" | "RED" | null;
  };
  tomorrow?: {
    date: string;
    color: "BLUE" | "WHITE" | "RED" | null;
  };
  tarifs?: TempoTarifs | null;
  lastUpdated?: string;
  cached?: boolean;
  error?: string;
  message: string;
}

// Tempo API (RTE electricity pricing colors)
export const tempoApi = {
  get: () => api<TempoData>("/tempo"),

  refresh: () =>
    api<TempoData>("/tempo/refresh", {
      method: "POST",
    }),

  getPredictions: () => api<TempoPredictionsData>("/tempo/predictions"),

  getState: () => api<TempoStateData>("/tempo/state"),

  getCalendar: (season?: string) =>
    api<TempoCalendarData>(`/tempo/calendar${season ? `?season=${season}` : ""}`),

  getHistory: (season?: string) =>
    api<TempoHistoryData>(`/tempo/history${season ? `?season=${season}` : ""}`),

  getCalibration: () => api<TempoCalibrationData>("/tempo/calibration"),
};

// Tempo prediction types
export interface TempoPrediction {
  date: string;
  predicted_color: "BLUE" | "WHITE" | "RED";
  probabilities: {
    BLUE: number;
    WHITE: number;
    RED: number;
  };
  confidence: number;
  constraints: {
    can_be_red: boolean;
    can_be_white: boolean;
    is_in_red_period: boolean;
  };
}

export interface TempoStateData {
  success: boolean;
  season?: string;
  stock_red_remaining?: number;
  stock_red_total?: number;
  stock_white_remaining?: number;
  stock_white_total?: number;
  consecutive_red?: number;
  error?: string;
  message: string;
}

export interface TempoPredictionsData {
  success: boolean;
  predictions?: TempoPrediction[];
  state?: {
    season: string;
    stock_red_remaining: number;
    stock_red_total: number;
    stock_white_remaining: number;
    stock_white_total: number;
  };
  model_version?: string;
  error?: string;
  message: string;
}

// Calendar day data
export interface TempoCalendarDay {
  date: string;
  color: "BLUE" | "WHITE" | "RED" | null;
  is_actual: boolean;
  is_prediction: boolean;
  probabilities?: {
    BLUE: number;
    WHITE: number;
    RED: number;
  };
  confidence?: number;
  constraints?: {
    can_be_red: boolean;
    can_be_white: boolean;
    is_in_red_period: boolean;
  };
}

export interface TempoCalendarData {
  success: boolean;
  season?: string;
  calendar?: TempoCalendarDay[];
  statistics?: {
    total_days: number;
    color_counts: {
      BLUE: number;
      WHITE: number;
      RED: number;
    };
    predictions_count: number;
  };
  stock?: {
    red_remaining: number;
    red_total: number;
    white_remaining: number;
    white_total: number;
  };
  error?: string;
  message?: string;
}

export interface TempoHistoryData {
  success: boolean;
  season?: string;
  history?: Array<{
    date: string;
    color: "BLUE" | "WHITE" | "RED";
    is_actual: boolean;
  }>;
  count?: number;
  error?: string;
  message?: string;
}

export interface TempoCalibrationData {
  success: boolean;
  calibrated?: boolean;
  params?: {
    thermosensitivity: number;
    base_consumption: number;
    temp_reference: number;
    calibration_date: string;
    calibration_accuracy: number;
    calibration_red_recall: number;
  };
  error?: string;
  message?: string;
}

// ---------------------------------------------------------------------------
// Nabaztag types
// ---------------------------------------------------------------------------

export interface NabaztagConfig {
  host: string;
  name: string;
  tempoEnabled: boolean;
}

export interface NabaztagConfigResponse {
  success: boolean;
  config: NabaztagConfig;
  message: string;
}

export interface NabaztagStatus {
  raw: Record<string, unknown>;
}

export interface NabaztagStatusResponse {
  success: boolean;
  status: NabaztagStatus;
  message: string;
}

export interface NabaztagSimpleResponse {
  success: boolean;
  message: string;
}

export interface NabaztagEarPosition {
  left: number;
  right: number;
}

export interface NabaztagEarsResponse {
  success: boolean;
  ears: NabaztagEarPosition;
  message: string;
}

export interface NabaztagForthResponse {
  success: boolean;
  output: string;
  message: string;
}

export interface NabaztagAnimationsResponse {
  success: boolean;
  animations: unknown;
  message: string;
}

export interface NabaztagTasksResponse {
  success: boolean;
  tasks: unknown;
  message: string;
}

export interface NabaztagTempoLedMapping {
  color: string;
  nose: string;
  left: string;
  center: string;
  right: string;
  base: string;
}

export interface NabaztagTempoPushResult {
  todayColor: string;
  tomorrowColor: string | null;
  todayLeds: NabaztagTempoLedMapping;
  tomorrowLeds: NabaztagTempoLedMapping | null;
  earLeftPosition: number;
  earRightPosition: number;
}

export interface NabaztagTempoPushResponse {
  success: boolean;
  result: NabaztagTempoPushResult;
  message: string;
}

export interface NabaztagLedColors {
  nose?: string;
  left?: string;
  center?: string;
  right?: string;
  base?: string;
  breathing?: boolean;
}

export interface NabaztagEarMoveRequest {
  ear: number;
  position: number;
  direction?: number;
}

export interface NabaztagSetupRequest {
  latitude?: string;
  longitude?: string;
  language?: string;
  taichi?: number;
  cityCode?: string;
  dst?: number;
  wakeUp?: number;
  goToBed?: number;
}

export interface NabaztagInfoServiceRequest {
  service: string;
  value: number;
}

// Nabaztag API
export const nabaztagApi = {
  // Configuration
  getConfig: () => api<NabaztagConfigResponse>("/nabaztag/config"),

  updateConfig: (config: NabaztagConfig) =>
    api<NabaztagConfigResponse>("/nabaztag/config", {
      method: "POST",
      body: config,
    }),

  // Status
  getStatus: () => api<NabaztagStatusResponse>("/nabaztag/status"),

  // Sleep / Wake
  sleep: () => api<NabaztagSimpleResponse>("/nabaztag/sleep", { method: "POST" }),
  wakeup: () => api<NabaztagSimpleResponse>("/nabaztag/wakeup", { method: "POST" }),

  // Ears
  getEars: () => api<NabaztagEarsResponse>("/nabaztag/ears"),
  moveEar: (req: NabaztagEarMoveRequest) =>
    api<NabaztagSimpleResponse>("/nabaztag/ears", {
      method: "POST",
      body: req,
    }),

  // LEDs
  setLeds: (colors: NabaztagLedColors) =>
    api<NabaztagSimpleResponse>("/nabaztag/leds", {
      method: "POST",
      body: colors,
    }),
  clearLeds: () => api<NabaztagSimpleResponse>("/nabaztag/leds/clear", { method: "POST" }),

  // Sound
  playUrl: (url: string) =>
    api<NabaztagSimpleResponse>("/nabaztag/play", {
      method: "POST",
      body: { url },
    }),
  say: (text: string) =>
    api<NabaztagSimpleResponse>("/nabaztag/say", {
      method: "POST",
      body: { text },
    }),
  soundCommunication: () =>
    api<NabaztagSimpleResponse>("/nabaztag/sound/communication", { method: "POST" }),
  soundAck: () => api<NabaztagSimpleResponse>("/nabaztag/sound/ack", { method: "POST" }),
  soundAbort: () => api<NabaztagSimpleResponse>("/nabaztag/sound/abort", { method: "POST" }),
  soundMinistop: () => api<NabaztagSimpleResponse>("/nabaztag/sound/ministop", { method: "POST" }),
  stop: () => api<NabaztagSimpleResponse>("/nabaztag/stop", { method: "POST" }),

  // Info services
  setInfoService: (req: NabaztagInfoServiceRequest) =>
    api<NabaztagSimpleResponse>("/nabaztag/info", {
      method: "POST",
      body: req,
    }),
  clearInfo: () => api<NabaztagSimpleResponse>("/nabaztag/info/clear", { method: "POST" }),

  // Utility
  taichi: () => api<NabaztagSimpleResponse>("/nabaztag/taichi", { method: "POST" }),
  surprise: () => api<NabaztagSimpleResponse>("/nabaztag/surprise", { method: "POST" }),
  reboot: () => api<NabaztagSimpleResponse>("/nabaztag/reboot", { method: "POST" }),
  updateTime: () => api<NabaztagSimpleResponse>("/nabaztag/update-time", { method: "POST" }),
  getAnimations: () => api<NabaztagAnimationsResponse>("/nabaztag/animations"),
  getTasks: () => api<NabaztagTasksResponse>("/nabaztag/tasks"),

  // Setup
  setup: (req: NabaztagSetupRequest) =>
    api<NabaztagSimpleResponse>("/nabaztag/setup", {
      method: "POST",
      body: req,
    }),

  // Forth interpreter
  executeForth: (code: string) =>
    api<NabaztagForthResponse>("/nabaztag/forth", {
      method: "POST",
      body: { code },
    }),

  // Tempo integration
  pushTempo: (forceRefresh = false) =>
    api<NabaztagTempoPushResponse>("/nabaztag/tempo/push", {
      method: "POST",
      body: { forceRefresh },
    }),
};

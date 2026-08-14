use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    auth::AuthenticatedUser,
    broadlink::{self, BroadlinkSecurityMode, LearnCodeSaveRequest, SaveCodeRequest},
    error::AppError,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoverQuery {
    local_ip: Option<String>,
    force_refresh: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MitsubishiQuery {
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionRequest {
    ssid: String,
    password: Option<String>,
    security_mode: BroadlinkSecurityMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LearnIrRequest {
    host: String,
    local_ip: Option<String>,
    timeout_secs: Option<u64>,
    save_code: Option<LearnCodeSaveBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LearnCodeSaveBody {
    name: String,
    brand: Option<String>,
    model: Option<String>,
    command: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendPacketRequest {
    host: String,
    local_ip: Option<String>,
    packet_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendCodeRequest {
    host: String,
    local_ip: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MitsubishiCommandRequest {
    host: String,
    local_ip: Option<String>,
    command: String,
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiscoverResponse {
    success: bool,
    devices: Vec<broadlink::BroadlinkDiscoveredDevice>,
    total: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct SimpleResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnResponse {
    success: bool,
    result: broadlink::LearnResult,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    success: bool,
    result: broadlink::SendResult,
    message: String,
}

#[derive(Debug, Serialize)]
struct CodesResponse {
    success: bool,
    codes: Vec<broadlink::BroadlinkCodeEntry>,
    total: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct ClimateStateResponse {
    success: bool,
    state: Option<broadlink::StoredClimateState>,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct CodeResponse {
    success: bool,
    code: broadlink::BroadlinkCodeEntry,
    message: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/discover", get(discover))
        .route("/provision", post(provision))
        .route("/learn/ir", post(learn_ir))
        .route("/send", post(send_packet))
        .route("/codes", get(list_codes).post(save_code))
        .route("/codes/{code_id}/send", post(send_code))
        .route("/mitsubishi/codes", get(list_mitsubishi_codes))
        .route("/mitsubishi/send", post(send_mitsubishi_command))
        .route("/mitsubishi/state", get(get_climate_state))
}

async fn discover(
    State(state): State<AppState>,
    Query(query): Query<DiscoverQuery>,
    user: AuthenticatedUser,
) -> Result<Json<DiscoverResponse>, AppError> {
    let _ = user.0;
    let devices = state
        .broadlink
        .discover(query.local_ip, query.force_refresh.unwrap_or(false))
        .await?;
    Ok(Json(DiscoverResponse {
        success: true,
        total: devices.len(),
        devices,
        message: "Broadlink discovery completed",
    }))
}

async fn provision(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<ProvisionRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    if body.ssid.trim().is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "ssid is required",
        ));
    }

    state
        .broadlink
        .provision(body.ssid.trim().to_string(), body.password, body.security_mode)
        .await?;

    Ok(Json(SimpleResponse {
        success: true,
        message: "Provisioning packet sent to Broadlink device in AP mode".to_string(),
    }))
}

async fn learn_ir(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<LearnIrRequest>,
) -> Result<Json<LearnResponse>, AppError> {
    let _ = user.0;
    let save_code = body.save_code.map(|save| LearnCodeSaveRequest {
        name: save.name,
        brand: save.brand,
        model: save.model,
        command: save.command,
        tags: save.tags,
    });
    let result = state
        .broadlink
        .learn_ir(body.host, body.local_ip, body.timeout_secs, save_code)
        .await?;
    Ok(Json(LearnResponse {
        success: true,
        message: "IR code learned successfully".to_string(),
        result,
    }))
}

async fn send_packet(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<SendPacketRequest>,
) -> Result<Json<SendResponse>, AppError> {
    let _ = user.0;
    let result = state
        .broadlink
        .send_packet(body.host, body.local_ip, body.packet_base64, None, None)
        .await?;
    Ok(Json(SendResponse {
        success: true,
        message: format!("Packet sent to Broadlink device {}", result.host),
        result,
    }))
}

async fn list_codes(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<CodesResponse>, AppError> {
    let _ = user.0;
    let codes = state.broadlink.list_codes().await;
    Ok(Json(CodesResponse {
        success: true,
        total: codes.len(),
        codes,
        message: "Broadlink codes retrieved",
    }))
}

async fn save_code(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<SaveCodeRequest>,
) -> Result<Json<CodeResponse>, AppError> {
    let _ = user.0;
    let code = state.broadlink.save_code(body).await?;
    Ok(Json(CodeResponse {
        success: true,
        message: format!("Code '{}' saved", code.name),
        code,
    }))
}

async fn send_code(
    State(state): State<AppState>,
    Path(code_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<SendCodeRequest>,
) -> Result<Json<SendResponse>, AppError> {
    let _ = user.0;
    let result = state
        .broadlink
        .send_saved_code(body.host, body.local_ip, code_id)
        .await?;
    Ok(Json(SendResponse {
        success: true,
        message: format!("Saved code sent to Broadlink device {}", result.host),
        result,
    }))
}

async fn list_mitsubishi_codes(
    State(state): State<AppState>,
    Query(query): Query<MitsubishiQuery>,
    user: AuthenticatedUser,
) -> Result<Json<CodesResponse>, AppError> {
    let _ = user.0;
    let codes = state
        .broadlink
        .list_mitsubishi_codes(query.model.as_deref())
        .await;
    Ok(Json(CodesResponse {
        success: true,
        total: codes.len(),
        codes,
        message: "Mitsubishi IR codes retrieved",
    }))
}

async fn get_climate_state(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ClimateStateResponse>, AppError> {
    let _ = user.0;
    let climate_state = state.broadlink.climate_state().await;
    Ok(Json(ClimateStateResponse {
        success: true,
        state: climate_state,
        message: "Last commanded Mitsubishi state",
    }))
}

async fn send_mitsubishi_command(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<MitsubishiCommandRequest>,
) -> Result<Json<SendResponse>, AppError> {
    let _ = user.0;
    let result = state
        .broadlink
        .send_mitsubishi_command(body.host, body.local_ip, body.command, body.model)
        .await?;
    Ok(Json(SendResponse {
        success: true,
        message: format!("Mitsubishi command sent via Broadlink device {}", result.host),
        result,
    }))
}

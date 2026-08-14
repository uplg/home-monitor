use axum::{
    Json, Router,
    extract::State,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    auth::AuthenticatedUser,
    error::AppError,
    nabaztag::{NabaztagConfig, TempoPushResult},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NabaztagStatusResponse {
    success: bool,
    config: NabaztagConfig,
    reachable: bool,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigRequest {
    host: Option<String>,
    #[serde(default = "default_true")]
    tempo_enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRequest {
    command: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TempoPushRequest {
    #[serde(default)]
    force_refresh: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TempoPushResponse {
    success: bool,
    message: String,
    result: TempoPushResult,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(status))
        .route("/config", put(set_config))
        .route("/ctl", post(send_command))
        .route("/tempo/push", post(push_tempo))
}

async fn status(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<NabaztagStatusResponse>, AppError> {
    let _ = user.0;
    let config = state.nabaztag.config().await;
    let rabbit_status = state.nabaztag.status().await.ok();
    Ok(Json(NabaztagStatusResponse {
        success: true,
        config,
        reachable: rabbit_status.is_some(),
        status: rabbit_status,
    }))
}

async fn set_config(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<ConfigRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state
        .nabaztag
        .set_config(NabaztagConfig {
            host: body.host.filter(|host| !host.trim().is_empty()),
            tempo_enabled: body.tempo_enabled,
        })
        .await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Nabaztag configuration saved".to_string(),
    }))
}

async fn send_command(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<CommandRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.send_command(&body.command).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Command sent: {}", body.command.trim()),
    }))
}

async fn push_tempo(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    body: Option<Json<TempoPushRequest>>,
) -> Result<Json<TempoPushResponse>, AppError> {
    let _ = user.0;
    let force_refresh = body.map(|b| b.force_refresh).unwrap_or(false);

    let (tempo_data, _) = state.tempo.get_tempo_data(force_refresh).await?;
    let today = tempo_data.today.color.as_deref().ok_or_else(|| {
        AppError::service_unavailable("Today's Tempo color is not available yet")
    })?;

    // Official color for tomorrow when published, prediction otherwise.
    let (tomorrow, _predicted) = state.tempo.tomorrow_color_or_predicted().await;

    let result = state
        .nabaztag
        .push_tempo(today, tomorrow.as_deref())
        .await?;

    Ok(Json(TempoPushResponse {
        success: true,
        message: format!(
            "Tempo pushed: today={}, tomorrow={}",
            result.today_color,
            result.tomorrow_color.as_deref().unwrap_or("unknown")
        ),
        result,
    }))
}

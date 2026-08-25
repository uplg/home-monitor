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
    ir::SwitchState,
    tv::{TvAmbilight, TvConfig, TvKey, TvPower, TvStatus, TvVolume},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    success: bool,
    config: TvConfig,
    status: TvStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigRequest {
    host: Option<String>,
    mac: Option<String>,
    box_host: Option<String>,
    box_wake_app: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PowerRequest {
    #[serde(default)]
    state: SwitchState,
    /// Also route the set to the box's HDMI input, which is almost always what
    /// is wanted: the TV otherwise comes back on whatever source it was left on.
    #[serde(default = "default_true")]
    switch_to_box: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyRequest {
    key: TvKey,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VolumeRequest {
    level: Option<u8>,
    muted: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AmbilightRequest {
    #[serde(default)]
    state: SwitchState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PowerResponse {
    success: bool,
    power: TvPower,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VolumeResponse {
    success: bool,
    volume: TvVolume,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AmbilightResponse {
    success: bool,
    ambilight: TvAmbilight,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleResponse {
    success: bool,
    message: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(status))
        .route("/config", put(set_config))
        .route("/power", post(power))
        .route("/key", post(send_key))
        .route("/volume", put(set_volume))
        .route("/ambilight", post(set_ambilight))
        .route("/ambilight/styles", get(ambilight_styles))
        .route("/source/box", post(switch_to_box))
}

/// Wakes the box and routes the set to its HDMI input.
///
/// Waking matters as much as the CEC: asserting One Touch Play against a
/// sleeping box turns the television on to a black screen, which then powers
/// itself back off for want of a signal. `wake()` does both, in that order.
///
/// The DIAL fallback works by *launching an app*, so it would yank the viewer
/// out of what they were watching — it is only worth it when ADB is down.
pub async fn route_to_box(state: &AppState) -> Result<(), AppError> {
    match state.androidtv.wake().await {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::debug!(%error, "CEC route failed, falling back to DIAL");
            state.tv.switch_to_box().await
        }
    }
}

/// Powers the set on, then optionally routes it to the box. The two are
/// separate steps so the input switch can go through CEC rather than the
/// app-launching DIAL path.
async fn power_on_and_route(state: &AppState, switch: bool) -> Result<TvPower, AppError> {
    let power = state.tv.power_on(false).await?;
    if switch {
        if let Err(error) = route_to_box(state).await {
            tracing::debug!(%error, "could not route the TV to the box");
        }
    }
    Ok(power)
}

/// Powers the set on and routes it to the box unless it is already on.
/// Shared with the Android TV routes, where launching an app on a dark screen
/// is never what the caller meant.
pub async fn ensure_on(state: &AppState) -> Result<(), AppError> {
    if state.tv.power().await == TvPower::On {
        return Ok(());
    }
    power_on_and_route(state, true).await.map(|_| ())
}

async fn status(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<StatusResponse>, AppError> {
    let _ = user.0;
    Ok(Json(StatusResponse {
        success: true,
        config: state.tv.config().await,
        status: state.tv.status().await,
    }))
}

async fn set_config(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<ConfigRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    let blank_to_none = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    state
        .tv
        .set_config(TvConfig {
            host: blank_to_none(body.host),
            mac: blank_to_none(body.mac),
            box_host: blank_to_none(body.box_host),
            box_wake_app: blank_to_none(body.box_wake_app),
        })
        .await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "TV configuration saved".to_string(),
    }))
}

async fn power(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<PowerRequest>,
) -> Result<Json<PowerResponse>, AppError> {
    let _ = user.0;
    let power = match body.state {
        SwitchState::On => power_on_and_route(&state, body.switch_to_box).await?,
        SwitchState::Off => state.tv.power_off().await?,
        SwitchState::Toggle => match state.tv.power().await {
            TvPower::On => state.tv.power_off().await?,
            _ => power_on_and_route(&state, body.switch_to_box).await?,
        },
    };
    Ok(Json(PowerResponse {
        success: true,
        power,
    }))
}

async fn send_key(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<KeyRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.tv.send_key(body.key).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Key sent".to_string(),
    }))
}

async fn set_volume(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<VolumeRequest>,
) -> Result<Json<VolumeResponse>, AppError> {
    let _ = user.0;
    let volume = match body.level {
        Some(level) => state.tv.set_volume(level, body.muted).await?,
        // A mute-only request still has to go through the absolute-volume
        // write, so read the current level and keep it.
        None => {
            let current = state.tv.volume().await?;
            match body.muted {
                Some(muted) => state.tv.set_volume(current.current, Some(muted)).await?,
                None => current,
            }
        }
    };
    Ok(Json(VolumeResponse {
        success: true,
        volume,
    }))
}

async fn set_ambilight(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<AmbilightRequest>,
) -> Result<Json<AmbilightResponse>, AppError> {
    let _ = user.0;
    let on = match body.state {
        SwitchState::On => true,
        SwitchState::Off => false,
        SwitchState::Toggle => !state.tv.ambilight().await?.power,
    };
    state.tv.set_ambilight_power(on).await?;
    Ok(Json(AmbilightResponse {
        success: true,
        ambilight: state.tv.ambilight().await?,
    }))
}

async fn ambilight_styles(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = user.0;
    state.tv.ambilight_styles().await.map(Json)
}

async fn switch_to_box(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    // CEC first: the DIAL fallback inside `route_to_box` wakes the box by
    // launching an app, which would interrupt whatever is playing.
    route_to_box(&state).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "TV routed to the box's HDMI input".to_string(),
    }))
}

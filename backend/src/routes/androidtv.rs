use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    androidtv::{AndroidApp, AndroidKey, AndroidTvConfig, AndroidTvStatus},
    auth::AuthenticatedUser,
    error::AppError,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    success: bool,
    config: AndroidTvConfig,
    status: AndroidTvStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigRequest {
    host: Option<String>,
    port: Option<u16>,
    #[serde(default)]
    favourite_apps: Vec<AndroidApp>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyRequest {
    key: AndroidKey,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchRequest {
    package: String,
    /// Power the television on and route it to the box first — launching an
    /// app on a set that is off or on another input is rarely what is meant.
    #[serde(default = "default_true")]
    ensure_tv_on: bool,
}

/// 96 MB covers any sideloaded app worth the name while staying survivable
/// on a 512 MB Pi.
const APK_SIZE_LIMIT: usize = 96 * 1024 * 1024;

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppsResponse {
    success: bool,
    packages: Vec<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(status))
        .route("/config", put(set_config))
        .route("/key", post(send_key))
        .route("/launch", post(launch))
        .route("/apps", get(apps))
        .route("/wake", post(wake))
        .route("/sleep", post(sleep))
        .route(
            "/apk",
            // APKs are large and the Pi is small; cap the upload well under
            // what the box and the backend can hold at once.
            post(install_apk).layer(DefaultBodyLimit::max(APK_SIZE_LIMIT)),
        )
}

async fn status(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<StatusResponse>, AppError> {
    let _ = user.0;
    Ok(Json(StatusResponse {
        success: true,
        config: state.androidtv.config().await,
        status: state.androidtv.status().await,
    }))
}

async fn set_config(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<ConfigRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state
        .androidtv
        .set_config(AndroidTvConfig {
            host: body.host.filter(|host| !host.trim().is_empty()),
            port: body.port,
            favourite_apps: body.favourite_apps,
        })
        .await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Android TV configuration saved".to_string(),
    }))
}

async fn send_key(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<KeyRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.androidtv.send_key(body.key).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Key sent".to_string(),
    }))
}

async fn launch(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<LaunchRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    if body.ensure_tv_on {
        // Best-effort: a dead TV link should not stop the app from launching.
        if let Err(error) = crate::routes::tv::ensure_on(&state).await {
            tracing::debug!(%error, "could not power the TV on before launching");
        }
    }
    state.androidtv.launch_app(&body.package).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Launched {}", body.package),
    }))
}

async fn apps(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<AppsResponse>, AppError> {
    let _ = user.0;
    Ok(Json(AppsResponse {
        success: true,
        packages: state.androidtv.apps().await?,
    }))
}

async fn wake(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.androidtv.wake().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Box woken (CEC should power the TV on)".to_string(),
    }))
}

async fn sleep(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.androidtv.sleep().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Box asleep (CEC should power the TV off)".to_string(),
    }))
}

async fn install_apk(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    mut multipart: Multipart,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;

    let mut apk: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("malformed upload: {error}"),
        )
    })? {
        if field.name() == Some("apk") {
            apk = Some(
                field
                    .bytes()
                    .await
                    .map_err(|error| {
                        AppError::http(
                            axum::http::StatusCode::BAD_REQUEST,
                            format!("could not read the APK: {error}"),
                        )
                    })?
                    .to_vec(),
            );
        }
    }

    let apk = apk.ok_or_else(|| {
        AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "no `apk` field in the upload",
        )
    })?;

    let message = state.androidtv.install_apk(&apk).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message,
    }))
}

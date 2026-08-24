pub mod auth;
pub mod broadlink;
pub mod config;
pub mod error;
#[cfg(feature = "bluetooth")]
pub mod hue;
#[cfg(not(feature = "bluetooth"))]
#[path = "hue_stub.rs"]
pub mod hue;
pub mod ir;
pub mod meross;
pub mod mitsubishi_ir;
pub mod nabaztag;
pub mod routes;
pub mod tempo;
pub mod tuya;
pub mod zigbee;
pub mod zigbee_native;

pub use tempo::TempoService;

use std::sync::Arc;

use axum::Router;
use axum::middleware::{self, Next};
use axum::http::header;
use axum::response::Response;
use auth::{AuthRateLimiter, RefreshTokenStore};
use broadlink::BroadlinkManager;
use config::Config;
use error::AppError;
use hue::HueManager;
use ir::IrManager;
use nabaztag::NabaztagManager;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::services::{ServeDir, ServeFile};
use routes::auth::{load_users, SharedUsers};
use meross::MerossManager;
use tuya::TuyaManager;
use tower_http::trace::TraceLayer;
use zigbee::ZigbeeManager;

#[derive(Clone)]
pub struct AppState {
    pub(crate) config: Arc<Config>,
    pub(crate) users: SharedUsers,
    pub(crate) auth_rate_limiter: AuthRateLimiter,
    pub(crate) refresh_store: RefreshTokenStore,
    pub(crate) broadlink: BroadlinkManager,
    pub(crate) hue: HueManager,
    pub(crate) ir: IrManager,
    pub(crate) meross: MerossManager,
    pub(crate) nabaztag: NabaztagManager,
    pub(crate) tempo: TempoService,
    pub(crate) tuya: TuyaManager,
    pub(crate) zigbee: ZigbeeManager,
}

pub fn app_from_env() -> Result<Router, AppError> {
    let config = Arc::new(Config::from_env());
    build_app_from_config(config)
}

pub fn app_parts_from_env() -> Result<(Router, AppState), AppError> {
    let config = Arc::new(Config::from_env());
    build_app_parts_from_config(config)
}

pub fn build_app_from_config(config: Arc<Config>) -> Result<Router, AppError> {
    let (app, _) = build_app_parts_from_config(config)?;
    Ok(app)
}

pub fn build_app_parts_from_config(config: Arc<Config>) -> Result<(Router, AppState), AppError> {
    let users = Arc::new(load_users(&config)?);
    let auth_rate_limiter = AuthRateLimiter::default();
    let refresh_store = RefreshTokenStore::load(&config.refresh_tokens_path);
    let broadlink =
        BroadlinkManager::new(&config.broadlink_codes_path, &config.climate_state_path)?;
    let hue = HueManager::new(config.as_ref())?;
    let ir = IrManager::new(&config.ir_keymap_path)?;
    let meross = MerossManager::new(&config.meross_devices_path)?;
    let nabaztag = NabaztagManager::new(
        &config.nabaztag_config_path,
        config.nabaztag_host.as_deref(),
    )?;
    let tempo = TempoService::new(config.source_root.clone())?;
    let tuya = TuyaManager::new(&config.devices_path, &config.device_cache_path)?;
    let zigbee = ZigbeeManager::new(config.as_ref())?;

    let state = AppState {
        config,
        users,
        auth_rate_limiter,
        refresh_store,
        broadlink,
        hue,
        ir,
        meross,
        nabaztag,
        tempo,
        tuya,
        zigbee,
    };

    let startup_tuya = state.tuya.clone();
    tokio::spawn(async move {
        let device_ids = startup_tuya
            .list_devices()
            .await
            .into_iter()
            .map(|device| device.id)
            .collect::<Vec<_>>();
        for device_id in device_ids {
            let _ = startup_tuya.connect_device(&device_id).await;
        }
    });

    // Mirror the daily Tempo colors on the Nabaztag. The push is two UDP
    // datagrams and idempotent, so it simply repeats every 15 minutes: that
    // also re-applies the colors after a rabbit reboot.
    let tempo_rabbit = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(900));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let config = tempo_rabbit.nabaztag.config().await;
            if !config.tempo_enabled || config.host.is_none() {
                continue;
            }
            match tempo_rabbit.tempo.get_tempo_data(false).await {
                Ok((data, _)) => {
                    if let Some(today) = data.today.color.as_deref() {
                        // Ears show tomorrow: the official color when RTE has
                        // published it, the model's prediction otherwise.
                        let (tomorrow, predicted) =
                            tempo_rabbit.tempo.tomorrow_color_or_predicted().await;
                        if predicted {
                            tracing::debug!(color = ?tomorrow, "using predicted color for tomorrow");
                        }
                        if let Err(error) = tempo_rabbit
                            .nabaztag
                            .push_tempo(today, tomorrow.as_deref())
                            .await
                        {
                            tracing::debug!(%error, "tempo push to the rabbit failed");
                        }
                    }
                }
                Err(error) => tracing::debug!(%error, "tempo data unavailable for rabbit push"),
            }
        }
    });

    let app = build_app(state.clone());
    Ok((app, state))
}

pub fn build_app(state: AppState) -> Router {
    let api_router = Router::<AppState>::new()
        .merge(routes::root::api_router())
        .nest("/auth", routes::auth::router())
        .nest("/broadlink", routes::broadlink::router())
        .nest("/devices", routes::devices::router())
        .nest("/hue-lamps", routes::hue::router())
        .nest("/ir", routes::ir::router())
        .nest("/meross", routes::meross::router())
        .nest("/nabaztag", routes::nabaztag::router())
        .nest("/tempo", routes::tempo::router())
        .nest("/zigbee", routes::zigbee::router());

    let app = Router::<AppState>::new()
        .merge(routes::root::health_router())
        .nest("/api", api_router);

    let app = if state.config.frontend_dist_dir.join("index.html").is_file() {
        app.fallback_service(
            ServeDir::new(state.config.frontend_dist_dir.clone())
                .not_found_service(ServeFile::new(state.config.frontend_dist_dir.join("index.html"))),
        )
    } else {
        app.merge(routes::root::router())
    };

    // CORS: reject all cross-origin requests. The frontend is served from the
    // same origin so legitimate requests never need CORS.
    app.layer(middleware::from_fn(security_headers))
        .layer(CorsLayer::new())
        // Last-resort guard so no request can hang a connection forever; the
        // limit sits above every legitimate long operation (IR learning,
        // device discovery, Zigbee command reply timeout of 120s).
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            std::time::Duration::from_secs(180),
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::extract::Request| {
                    tracing::info_span!(
                        "http",
                        method = %request.method(),
                        uri = %request.uri(),
                    )
                }),
        )
        .with_state(state)
}

async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=63072000; includeSubDomains".parse().unwrap(),
    );
    // Note: style-src 'unsafe-inline' is required because the React frontend uses
    // inline styles (e.g. dynamic positioning and color previews in device controls).
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; frame-ancestors 'none'".parse().unwrap(),
    );
    headers.insert(
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    response
}

impl AppState {
    pub fn validate_runtime_security(&self) -> Result<(), AppError> {
        if self.config.jwt_secret == "super-secret-cat-key-change-me" {
            return Err(AppError::http(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Refusing to start with the default JWT secret. Set JWT_SECRET in .env.",
            ));
        }

        Ok(())
    }

    pub async fn shutdown(&self) {
        self.hue.shutdown().await;
        self.zigbee.shutdown().await;
    }
}

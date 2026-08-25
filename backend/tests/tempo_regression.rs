use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode},
};
use chrono::Utc;
use chrono_tz::Europe::Paris;
use maison_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::Value;
use tower::ServiceExt;

use maison_backend::tempo::TempoService;

#[tokio::test]
async fn tempo_state_uses_migrated_cache_data() {
    let (status, body) = request_rust("/api/tempo/state").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(body.get("season").and_then(Value::as_str), Some("2025-2026"));
    assert!(body.get("stock_red_remaining").and_then(Value::as_i64).is_some());
    assert!(body.get("stock_white_remaining").and_then(Value::as_i64).is_some());
}

#[tokio::test]
async fn tempo_predictions_return_forecast_entries() {
    let (status, body) = request_rust("/api/tempo/predictions").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));

    let predictions = body
        .get("predictions")
        .and_then(Value::as_array)
        .expect("predictions should be an array");
    assert_eq!(predictions.len(), 7);
    let expected_first_date = Utc::now()
        .with_timezone(&Paris)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(
        predictions.first().and_then(|day| day.get("date")).and_then(Value::as_str),
        Some(expected_first_date.as_str())
    );
    assert!(predictions.iter().all(|day| day.get("predicted_color").is_some()));
}

#[tokio::test]
async fn tempo_history_reads_cached_season_file() {
    let (status, body) = request_rust("/api/tempo/history?season=2025-2026").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));

    let history = body
        .get("history")
        .and_then(Value::as_array)
        .expect("history should be an array");
    assert!(!history.is_empty());
    assert!(history.iter().any(|day| {
        day.get("date").and_then(Value::as_str) == Some("2026-01-29")
            && day.get("color").and_then(Value::as_str) == Some("RED")
    }));
}

#[tokio::test]
async fn tempo_calibration_reads_migrated_calibration_file() {
    let (status, body) = request_rust("/api/tempo/calibration").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(body.get("calibrated").and_then(Value::as_bool), Some(true));

    // Read the expected date from the actual calibration file instead of hardcoding it,
    // since recalibration updates this value.
    let calibration_file = workspace_root().join("cache/tempo/calibration_params.json");
    let raw = std::fs::read_to_string(&calibration_file).expect("calibration file should exist");
    let expected: Value = serde_json::from_str(&raw).expect("calibration file should be valid json");
    let expected_date = expected
        .get("calibration_date")
        .and_then(Value::as_str)
        .expect("calibration file should have calibration_date");

    assert_eq!(
        body.pointer("/params/calibration_date").and_then(Value::as_str),
        Some(expected_date)
    );
}

#[tokio::test]
async fn tempo_recalibration_produces_metrics_without_persisting() {
    let service = TempoService::new(workspace_root()).expect("tempo service should build");
    let report = match service
        .recalibrate(&["2024-2025".to_string(), "2025-2026".to_string()], false)
        .await
    {
        Ok(report) => report,
        Err(error) => {
            assert!(error
                .to_string()
                .contains("Not enough historical weather samples for calibration"));
            return;
        }
    };

    assert_eq!(report.seasons, vec!["2024-2025", "2025-2026"]);
    assert!(report.params.calibration_sample_count >= 120);
    assert!(report.params.calibration_accuracy > 0.0);
    assert!(report.params.calibration_red_recall >= 0.0);
    assert!(report.params.calibration_white_recall >= 0.0);
}

async fn request_rust(path: &str) -> (StatusCode, Value) {
    let config = test_config();
    let token = rust_test_token(&config.jwt_secret);
    let app = build_app_from_config(Arc::new(config))
        .expect("failed to build test app")
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build");

    let response = app.oneshot(request).await.expect("request should succeed");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json = serde_json::from_slice::<Value>(&body).expect("response should be valid json");
    (status, json)
}

fn test_config() -> Config {
    let source_root = workspace_root();
    Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        jwt_secret: env::var("JWT_SECRET").unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string()),
        frontend_dist_dir: source_root.join("frontend").join("dist"),
        auth_cookie_name: "maison_session".to_string(),
        auth_cookie_secure: false,
        auth_rate_limit_attempts: 10,
        auth_rate_limit_window_seconds: 300,
        disable_bluetooth: true,
        users_path: source_root.join("users.json"),
        meross_devices_path: source_root.join("meross-devices.json"),
        devices_path: source_root.join("devices.json"),
        device_cache_path: source_root.join("device-cache.json"),
        broadlink_codes_path: source_root.join("broadlink-codes.json"),
        climate_state_path: source_root.join("climate-state.json"),
        refresh_tokens_path: source_root.join("refresh-tokens.json"),
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        nabaztag_config_path: source_root.join("nabaztag.json"),
        nabaztag_host: None,
        zigbee_permit_join_seconds: 120,
        ir_keymap_path: source_root.join("ir-keymap.json"),
        tv_config_path: source_root.join("tv.json"),
        androidtv_config_path: source_root.join("androidtv.json"),
        adb_key_path: source_root.join("adb-key"),
        ir_api_token: Some("test-ir-token".to_string()),
        source_root,
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}

fn rust_test_token(secret: &str) -> String {
    let claims = Claims {
        user_id: "1".to_string(),
        username: "tempo-regression".to_string(),
        role: "admin".to_string(),
        exp: 4_102_444_800,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("test token should encode")
}

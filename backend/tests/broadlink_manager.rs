use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode},
};
use maison_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn broadlink_codes_can_be_saved_and_listed() {
    let app = test_app();

    let save = request_rust(
        &app,
        Method::POST,
        "/api/broadlink/codes",
        Some(json!({
            "name": "Salon AC 22C",
            "brand": "Mitsubishi",
            "model": "MSZ-AP",
            "command": "cool_22_auto",
            "packetBase64": "AQIDBA==",
            "tags": ["salon", "clim"]
        })),
    )
    .await;

    assert_eq!(save.0, StatusCode::OK);
    assert_eq!(save.1.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(save.1.pointer("/code/command").and_then(Value::as_str), Some("cool_22_auto"));

    let list = request_rust(&app, Method::GET, "/api/broadlink/codes", None).await;
    assert_eq!(list.0, StatusCode::OK);
    assert_eq!(list.1.get("total").and_then(Value::as_u64), Some(1));
    assert_eq!(list.1.pointer("/codes/0/brand").and_then(Value::as_str), Some("mitsubishi"));
}

#[tokio::test]
async fn broadlink_mitsubishi_filter_returns_only_matching_brand() {
    let app = test_app();

    let _ = request_rust(
        &app,
        Method::POST,
        "/api/broadlink/codes",
        Some(json!({
            "name": "Salon AC 22C",
            "brand": "Mitsubishi",
            "model": "MSZ-AP",
            "command": "cool_22_auto",
            "packetBase64": "AQIDBA=="
        })),
    )
    .await;
    let _ = request_rust(
        &app,
        Method::POST,
        "/api/broadlink/codes",
        Some(json!({
            "name": "TV Power",
            "brand": "Sony",
            "command": "power",
            "packetBase64": "BQYHCA=="
        })),
    )
    .await;

    let response = request_rust(&app, Method::GET, "/api/broadlink/mitsubishi/codes", None).await;
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1.get("total").and_then(Value::as_u64), Some(1));
    assert_eq!(response.1.pointer("/codes/0/brand").and_then(Value::as_str), Some("mitsubishi"));
}

fn test_app() -> axum::Router {
    let config = test_config();
    build_app_from_config(Arc::new(config))
        .expect("failed to build test app")
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))))
}

async fn request_rust(app: &axum::Router, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let token = rust_test_token();

    let mut builder = Request::builder().method(method).uri(path);
    builder = builder.header("Authorization", format!("Bearer {token}"));
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }

    let request = builder
        .body(
            body.map(|value| Body::from(serde_json::to_vec(&value).expect("body should encode")))
                .unwrap_or_else(Body::empty),
        )
        .expect("request should build");

    let response = app.clone().oneshot(request).await.expect("request should succeed");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json = serde_json::from_slice::<Value>(&body).expect("response should be valid json");
    (status, json)
}

fn test_config() -> Config {
    let source_root = workspace_root();
    let temp_root = std::env::temp_dir()
        .join("maison-rust-broadlink-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_root).expect("temp test dir should be created");

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
        broadlink_codes_path: temp_root.join("broadlink-codes.json"),
        climate_state_path: temp_root.join("climate-state.json"),
        refresh_tokens_path: temp_root.join("refresh-tokens.json"),
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

fn rust_test_token() -> String {
    let claims = Claims {
        user_id: "1".to_string(),
        username: "leonard".to_string(),
        role: "admin".to_string(),
        exp: 4_102_444_800,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(
            env::var("JWT_SECRET")
                .unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string())
                .as_bytes(),
        ),
    )
    .expect("test token should encode")
}

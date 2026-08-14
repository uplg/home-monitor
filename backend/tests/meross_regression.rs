#![cfg(feature = "live-runtime-tests")]

use std::{
    env,
    sync::{Arc, OnceLock},
};

use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
};
use cat_monitor_rust_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use tower::ServiceExt;

const LEGACY_BASE_URL: &str = "http://localhost:3033";
const LEGACY_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VySWQiOiIxIiwidXNlcm5hbWUiOiJsZW9uYXJkIiwicm9sZSI6ImFkbWluIiwiZXhwIjoxNzczODc0NTE1LCJpYXQiOjE3NzMyNjk3MTV9.iA2VDfv_KLmADqGHI-yXa2fPRom5LqfyKIT2mP3dh6g";
const DEVICE_ID: &str = "192.168.1.113";
static MEROSS_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[tokio::test]
async fn meross_list_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::GET, "/api/meross", None).await;
    let legacy = request_legacy(Method::GET, "/meross", None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_list(rust.1), &normalize_list(legacy.1));
}

#[tokio::test]
async fn meross_status_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::GET, &format!("/api/meross/{DEVICE_ID}/status"), None).await;
    let legacy = request_legacy(Method::GET, &format!("/meross/{DEVICE_ID}/status"), None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_status(rust.1), &normalize_status(legacy.1));
}

#[tokio::test]
async fn meross_electricity_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::GET, &format!("/api/meross/{DEVICE_ID}/electricity"), None).await;
    let legacy = request_legacy(Method::GET, &format!("/meross/{DEVICE_ID}/electricity"), None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_electricity(rust.1), &normalize_electricity(legacy.1));
}

#[tokio::test]
async fn meross_consumption_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::GET, &format!("/api/meross/{DEVICE_ID}/consumption"), None).await;
    let legacy = request_legacy(Method::GET, &format!("/meross/{DEVICE_ID}/consumption"), None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_consumption(rust.1), &normalize_consumption(legacy.1));
}

#[tokio::test]
async fn meross_toggle_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let body = json!({ "on": false });
    let rust = request_rust(Method::POST, &format!("/api/meross/{DEVICE_ID}/toggle"), Some(body.clone())).await;
    let legacy = request_legacy(Method::POST, &format!("/meross/{DEVICE_ID}/toggle"), Some(body)).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_toggle(rust.1), &normalize_toggle(legacy.1));
}

#[tokio::test]
async fn meross_turn_on_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::POST, &format!("/api/meross/{DEVICE_ID}/on"), None).await;
    let legacy = request_legacy(Method::POST, &format!("/meross/{DEVICE_ID}/on"), None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_toggle(rust.1), &normalize_toggle(legacy.1));
}

#[tokio::test]
async fn meross_turn_off_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let rust = request_rust(Method::POST, &format!("/api/meross/{DEVICE_ID}/off"), None).await;
    let legacy = request_legacy(Method::POST, &format!("/meross/{DEVICE_ID}/off"), None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_toggle(rust.1), &normalize_toggle(legacy.1));
}

#[tokio::test]
async fn meross_dnd_matches_legacy_contract() {
    let _guard = meross_test_lock().lock().await;
    let body = json!({ "enabled": true });
    let rust = request_rust(Method::POST, &format!("/api/meross/{DEVICE_ID}/dnd"), Some(body.clone())).await;
    let legacy = request_legacy(Method::POST, &format!("/meross/{DEVICE_ID}/dnd"), Some(body)).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    assert_json_eq(&normalize_dnd(rust.1), &normalize_dnd(legacy.1));
}

async fn request_rust(method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let config = test_config();
    let app = build_app_from_config(Arc::new(config)).expect("failed to build test app");
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

    let response = app.oneshot(request).await.expect("request should succeed");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json = serde_json::from_slice::<Value>(&body).expect("response should be valid json");
    (status, json)
}

async fn request_legacy(method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{LEGACY_BASE_URL}{path}"))
        .bearer_auth(LEGACY_TOKEN);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .expect("legacy request should succeed");
    let status = response.status();
    let json = response
        .json::<Value>()
        .await
        .expect("legacy response should be valid json");
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
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        zigbee_permit_join_seconds: 120,
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

fn meross_test_lock() -> &'static tokio::sync::Mutex<()> {
    MEROSS_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn normalize_list(value: Value) -> Value {
    let devices = value
        .get("devices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|device| {
            json!({
                "id": device.get("id").cloned().unwrap_or(Value::Null),
                "name": device.get("name").cloned().unwrap_or(Value::Null),
                "ip": device.get("ip").cloned().unwrap_or(Value::Null),
                "isOnline": device.get("isOnline").cloned().unwrap_or(Value::Null),
                "isOn": device.get("isOn").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let mut devices = devices;
    devices.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .cmp(&right.get("id").and_then(Value::as_str))
    });

    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "devices": devices,
        "total": value.get("total").cloned().unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_status(value: Value) -> Value {
    let status = value.get("status").cloned().unwrap_or(Value::Null);
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": value.get("device").cloned().unwrap_or(Value::Null),
        "status": {
            "online": status.get("online").cloned().unwrap_or(Value::Null),
            "on": status.get("on").cloned().unwrap_or(Value::Null),
            "electricity": status.get("electricity").cloned().unwrap_or(Value::Null),
            "hardware": status.get("hardware").cloned().unwrap_or(Value::Null),
            "firmware": status.get("firmware").cloned().unwrap_or(Value::Null),
            "wifi": {
                "signal": status
                    .get("wifi")
                    .and_then(|wifi| wifi.get("signal"))
                    .map(|signal| if signal.is_null() { Value::Null } else { Value::String("present".to_string()) })
                    .unwrap_or(Value::Null),
            },
        },
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_electricity(value: Value) -> Value {
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": value.get("device").cloned().unwrap_or(Value::Null),
        "electricity": value.get("electricity").cloned().unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_consumption(value: Value) -> Value {
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": value.get("device").cloned().unwrap_or(Value::Null),
        "consumption": value.get("consumption").cloned().unwrap_or(Value::Null),
        "summary": value.get("summary").cloned().unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_toggle(value: Value) -> Value {
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": value.get("device").cloned().unwrap_or(Value::Null),
        "on": value.get("on").cloned().unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_dnd(value: Value) -> Value {
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": value.get("device").cloned().unwrap_or(Value::Null),
        "dndMode": value.get("dndMode").cloned().unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_numbers(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_numbers).collect()),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, normalize_numbers(value)))
                .collect(),
        ),
        Value::Number(number) => {
            if let Some(value) = number.as_f64() {
                json!((value * 1_000_000.0).round() / 1_000_000.0)
            } else {
                Value::Number(number)
            }
        }
        other => other,
    }
}

fn assert_json_eq(left: &Value, right: &Value) {
    assert_eq!(left, right, "left:\n{}\n\nright:\n{}", pretty(left), pretty(right));
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("json should pretty print")
}

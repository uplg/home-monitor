#![cfg(feature = "live-runtime-tests")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use maison_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower::ServiceExt;

const LEGACY_BASE_URL: &str = "http://localhost:3033";
const LEGACY_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VySWQiOiIxIiwidXNlcm5hbWUiOiJsZW9uYXJkIiwicm9sZSI6ImFkbWluIiwiZXhwIjoxNzczODc0NTE1LCJpYXQiOjE3NzMyNjk3MTV9.iA2VDfv_KLmADqGHI-yXa2fPRom5LqfyKIT2mP3dh6g";
static TUYA_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
struct StoredFixture {
    status: u16,
    body: Value,
}

#[tokio::test]
async fn devices_list_matches_runtime_contract() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices").await;

    assert_eq!(rust.0, StatusCode::OK);
    let devices = rust
        .1
        .get("devices")
        .and_then(Value::as_array)
        .cloned()
        .expect("devices list should be present");

    assert_eq!(devices.len(), 3);
    assert!(devices.iter().any(|device| device.get("type") == Some(&Value::String("feeder".to_string()))));
    assert!(devices.iter().any(|device| device.get("type") == Some(&Value::String("litter-box".to_string()))));
    assert!(devices.iter().any(|device| device.get("type") == Some(&Value::String("fountain".to_string()))));
}

#[tokio::test]
async fn stats_reports_runtime_connection_fields() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/stats").await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.1.pointer("/success"), Some(&Value::Bool(true)));
    assert_eq!(rust.1.pointer("/total"), Some(&json!(3)));
    assert!(rust.1.pointer("/connected").is_some());
    assert!(rust.1.pointer("/disconnected").is_some());
    assert!(rust.1.pointer("/devices/0/connecting").is_some());
    assert!(rust.1.pointer("/devices/0/reconnectAttempts").is_some());
}

#[tokio::test]
async fn disconnect_then_connect_device_updates_runtime_stats() {
    let _guard = tuya_test_lock().lock().await;

    let app = build_test_app();

    let disconnect = request_with_app(&app, Method::GET, "/api/devices/bfe88591a492929ab380tm/disconnect", None).await;
    assert_eq!(disconnect.0, StatusCode::OK);

    let stats = request_with_app(&app, Method::GET, "/api/devices/stats", None).await;
    assert_eq!(stats.0, StatusCode::OK);
    let litter_entry = find_stats_device(&stats.1, "bfe88591a492929ab380tm");
    assert_eq!(litter_entry.pointer("/isConnected"), Some(&Value::Bool(false)));

    let connect = request_with_app(&app, Method::GET, "/api/devices/bfe88591a492929ab380tm/connect", None).await;
    assert_eq!(connect.0, StatusCode::OK);

    let stats = request_with_app(&app, Method::GET, "/api/devices/stats", None).await;
    assert_eq!(stats.0, StatusCode::OK);
    let litter_entry = find_stats_device(&stats.1, "bfe88591a492929ab380tm");
    let reconnect_attempts = litter_entry
        .pointer("/reconnectAttempts")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let is_connected = litter_entry.pointer("/isConnected").and_then(Value::as_bool);
    assert!(
        is_connected == Some(true) || reconnect_attempts > 0,
        "connect should either succeed immediately or register a retry attempt"
    );
}

#[tokio::test]
async fn scan_dps_reports_visible_range_summary() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/bfe88591a492929ab380tm/scan-dps?start=101&end=119&timeout=3000").await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.1.pointer("/scan_range"), Some(&Value::String("101-119".to_string())));
    assert!(rust.1.pointer("/available_dps/101").is_some());
    assert!(rust.1.pointer("/found_count").and_then(Value::as_u64).unwrap_or_default() > 0);
}

#[tokio::test]
async fn fountain_status_matches_runtime_contract() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/bf855a2e493e0257b1mebx/fountain/status").await;

    if rust.0 == StatusCode::SERVICE_UNAVAILABLE {
        assert!(
            rust.1
                .pointer("/error")
                .and_then(Value::as_str)
                .is_some(),
            "503 responses should still include an error body"
        );
        return;
    }

    assert_eq!(rust.0, StatusCode::OK);
    let normalized = normalize_typed_status(rust.1);
    assert_eq!(
        normalized.pointer("/raw_dps/12"),
        Some(&Value::String("level_3".to_string()))
    );
    assert_eq!(normalized.pointer("/raw_dps/1"), Some(&Value::Bool(true)));
    assert_eq!(normalized.pointer("/parsed_status/water_level"), Some(&Value::String("level_3".to_string())));
    assert_eq!(normalized.pointer("/parsed_status/power"), Some(&Value::Bool(true)));
}

#[tokio::test]
async fn litter_box_status_matches_fixture_contract() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/bfe88591a492929ab380tm/litter-box/status").await;
    let legacy = load_fixture("litter-box-status");

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(rust.0, legacy.0);
    let rust = normalize_litter_box_status(normalize_typed_status(rust.1));
    let legacy = normalize_litter_box_status(normalize_typed_status(legacy.1));
    assert_json_eq(&rust, &legacy);
}

#[tokio::test]
async fn devices_list_reports_cache_backed_fields() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices").await;
    assert_eq!(rust.0, StatusCode::OK);
    let rust = rust.1;

    let litter_box = rust
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices.iter().find(|device| {
                device
                    .get("id")
                    .and_then(Value::as_str)
                    == Some("bfe88591a492929ab380tm")
            })
        })
        .cloned()
        .expect("litter box should be present in devices list");

    assert_eq!(
        litter_box.pointer("/last_data/dps/112"),
        Some(&Value::String("full".to_string()))
    );

    let feeder = rust
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices.iter().find(|device| {
                device
                    .get("id")
                    .and_then(Value::as_str)
                    == Some("bfa64c250eb410189dy9gq")
            })
        })
        .cloned()
        .expect("feeder should be present in devices list");

    assert!(feeder.pointer("/last_data/dps/1").is_some());
}

#[tokio::test]
async fn feeder_status_matches_legacy_contract() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/bfa64c250eb410189dy9gq/feeder/status").await;

    assert_eq!(rust.0, StatusCode::OK);
    let normalized = normalize_typed_status(rust.1);
    assert_eq!(
        normalized.pointer("/parsed_status/feeding/manual_feed_enabled"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        normalized.pointer("/parsed_status/system/ip_address"),
        Some(&Value::String("192.168.1.174".to_string()))
    );
    assert!(normalized.pointer("/raw_dps/1").is_some());
}

#[tokio::test]
async fn feeder_meal_plan_reads_from_cache_contract() {
    let _guard = tuya_test_lock().lock().await;
    let rust = request_rust("/api/devices/bfa64c250eb410189dy9gq/feeder/meal-plan").await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(
        rust.1.pointer("/meal_plan"),
        Some(&Value::String("fwAAAwF/BgADAX8JAAIBfwwAAwF/DwACAQ==".to_string()))
    );
    assert_eq!(
        rust.1.pointer("/decoded/0/time"),
        Some(&Value::String("00:00".to_string()))
    );
}

#[tokio::test]
async fn feeder_feed_rejects_out_of_range_portion_without_touching_device() {
    let rust = request_rust_with_body(
        Method::POST,
        "/api/devices/bfa64c250eb410189dy9gq/feeder/feed",
        Some(json!({ "portion": 13 })),
    )
    .await;

    assert_eq!(rust.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        rust.1.pointer("/error"),
        Some(&Value::String("portion must be between 1 and 12".to_string()))
    );
}

#[tokio::test]
async fn feeder_meal_plan_update_rejects_invalid_entry_without_touching_device() {
    let body = json!({
        "meal_plan": [
            {
                "days_of_week": ["Funday"],
                "time": "08:30",
                "portion": 2,
                "status": "Enabled"
            }
        ]
    });

    let rust = request_rust_with_body(
        Method::POST,
        "/api/devices/bfa64c250eb410189dy9gq/feeder/meal-plan",
        Some(body),
    )
    .await;

    assert_eq!(rust.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        rust.1.pointer("/error"),
        Some(&Value::String("Invalid meal plan entry at index 0".to_string()))
    );
}

#[tokio::test]
#[ignore = "refreshes fixtures from a live legacy backend"]
async fn refresh_legacy_tuya_fixtures() {
    refresh_fixture("litter-box-status", "/litter-box/bfe88591a492929ab380tm/status").await;
}

async fn request_rust(path: &str) -> (StatusCode, Value) {
    let app = build_test_app();
    request_with_app(&app, Method::GET, path, None).await
}

async fn request_rust_with_body(method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let app = build_test_app();
    request_with_app(&app, method, path, body).await
}

fn build_test_app() -> Router {
    let config = test_config();
    build_app_from_config(Arc::new(config)).expect("failed to build test app")
}

async fn request_with_app(app: &Router, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
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

async fn request_legacy(path: &str) -> (StatusCode, Value) {
    let response = reqwest::Client::new()
        .get(format!("{LEGACY_BASE_URL}{path}"))
        .bearer_auth(LEGACY_TOKEN)
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

fn load_fixture(name: &str) -> (StatusCode, Value) {
    let path = fixture_path(name);
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    let fixture: StoredFixture = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("failed to parse fixture {}: {error}", path.display()));
    let status = StatusCode::from_u16(fixture.status)
        .unwrap_or_else(|error| panic!("fixture {} has invalid status: {error}", path.display()));
    (status, fixture.body)
}

async fn refresh_fixture(name: &str, path: &str) {
    let (status, body) = request_legacy(path).await;
    let stored = StoredFixture {
        status: status.as_u16(),
        body,
    };
    let output_path = fixture_path(name);
    let json = serde_json::to_string_pretty(&stored).expect("fixture should serialize");
    fs::write(&output_path, format!("{json}\n"))
        .unwrap_or_else(|error| panic!("failed to write fixture {}: {error}", output_path.display()));
}

fn test_config() -> Config {
    let source_root = workspace_root();
    let temp_root = std::env::temp_dir()
        .join("maison-rust-tests")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_root).expect("temp test dir should be created");
    let cache_fixture = fixture_path("device-cache");
    let cache_copy = temp_root.join("device-cache.json");
    fs::copy(&cache_fixture, &cache_copy).expect("device cache fixture should be copied");

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
        device_cache_path: cache_copy,
        broadlink_codes_path: temp_root.join("broadlink-codes.json"),
        climate_state_path: temp_root.join("climate-state.json"),
        refresh_tokens_path: temp_root.join("refresh-tokens.json"),
        nabaztag_config_path: temp_root.join("nabaztag.json"),
        nabaztag_host: None,
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

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tuya")
        .join(format!("{name}.json"))
}

fn tuya_test_lock() -> &'static tokio::sync::Mutex<()> {
    TUYA_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
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

fn normalize_typed_status(value: Value) -> Value {
    normalize_numbers(json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "device": json!({
            "id": value.get("device").and_then(|device| device.get("id")).cloned().unwrap_or(Value::Null),
            "name": value.get("device").and_then(|device| device.get("name")).cloned().unwrap_or(Value::Null),
        }),
        "parsed_status": value.get("parsed_status").cloned().unwrap_or(Value::Null),
        "raw_dps": value
            .get("raw_dps")
            .cloned()
            .or_else(|| value.get("raw_status").cloned())
            .unwrap_or(Value::Null),
        "message": value.get("message").cloned().unwrap_or(Value::Null),
    }))
}

fn normalize_litter_box_status(mut value: Value) -> Value {
    if let Some(raw_dps) = value.get_mut("raw_dps").and_then(Value::as_object_mut) {
        raw_dps.remove("107");
        raw_dps.remove("109");
    }
    if let Some(parsed_status) = value.get_mut("parsed_status").and_then(Value::as_object_mut) {
        if let Some(system) = parsed_status.get_mut("system").and_then(Value::as_object_mut) {
            system.remove("cleaning_in_progress");
            system.remove("state");
        }
    }
    value
}

fn find_stats_device<'a>(value: &'a Value, device_id: &str) -> &'a Value {
    value
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices.iter().find(|device| {
                device
                    .get("id")
                    .and_then(Value::as_str)
                    == Some(device_id)
            })
        })
        .expect("device entry should be present")
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

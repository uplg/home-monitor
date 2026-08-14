use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode},
};
use cat_monitor_rust_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn auth_verify_accepts_valid_token() {
    let rust = request_rust(Method::POST, "/api/auth/verify", None, Some(rust_test_token())).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(
        normalize_verify_response(rust.1),
        json!({
            "success": true,
            "user": {
                "id": "1",
                "username": "leonard",
                "role": "admin"
            }
        })
    );
}

#[tokio::test]
async fn auth_logout_returns_success_message() {
    let rust = request_rust(Method::POST, "/api/auth/logout", None, None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(
        rust.1,
        json!({
            "success": true,
            "message": "Logged out successfully"
        })
    );
}

#[tokio::test]
async fn auth_invalid_login_returns_unauthorized() {
    let body = json!({ "username": "nope", "password": "nope" });
    let rust = request_rust(Method::POST, "/api/auth/login", Some(body), None).await;

    assert_eq!(rust.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        rust.1,
        json!({
            "success": false,
            "error": "Invalid username or password"
        })
    );
}

#[tokio::test]
async fn auth_successful_login_returns_ok_and_sets_cookies() {
    let temp_dir = std::env::temp_dir()
        .join("cat-monitor-auth-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");

    // Argon2id hash of "testpass123"
    let users_json = serde_json::to_string(&json!([{
        "id": "42",
        "username": "testuser",
        "password_hash": "$argon2id$v=19$m=65536,t=3,p=4$go6DSNTX5Epa+lj9oxCogw$gGKfcn6IJsTTpj9XGQ+prTlKRGUUAMwJ6mhnzaYkvGs",
        "role": "admin"
    }]))
    .unwrap();
    let users_path = temp_dir.join("users.json");
    std::fs::write(&users_path, &users_json).expect("users file should be written");

    let source_root = workspace_root();
    let config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        jwt_secret: "super-secret-cat-key-change-me".to_string(),
        frontend_dist_dir: source_root.join("frontend").join("dist"),
        auth_cookie_name: "maison_session".to_string(),
        auth_cookie_secure: false,
        auth_rate_limit_attempts: 10,
        auth_rate_limit_window_seconds: 300,
        disable_bluetooth: true,
        users_path,
        meross_devices_path: source_root.join("meross-devices.json"),
        devices_path: source_root.join("devices.json"),
        device_cache_path: source_root.join("device-cache.json"),
        broadlink_codes_path: source_root.join("broadlink-codes.json"),
        climate_state_path: source_root.join("climate-state.json"),
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        nabaztag_config_path: source_root.join("nabaztag.json"),
        nabaztag_host: None,
        zigbee_permit_join_seconds: 120,
        source_root,
    };

    let app = build_app_from_config(Arc::new(config))
        .expect("failed to build test app")
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let body = json!({ "username": "testuser", "password": "testpass123" });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.expect("request should succeed");
    let status = response.status();

    // Collect Set-Cookie headers before consuming the body.
    let set_cookie_headers: Vec<String> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();

    let body_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json: Value = serde_json::from_slice(&body_bytes).expect("response should be valid json");

    assert_eq!(status, StatusCode::OK, "response body: {json}");
    assert_eq!(json.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(json.pointer("/user/id").and_then(Value::as_str), Some("42"));
    assert_eq!(json.pointer("/user/username").and_then(Value::as_str), Some("testuser"));

    // Both access and refresh cookies must be present.
    assert!(
        set_cookie_headers.iter().any(|c| c.starts_with("maison_session=")),
        "access token cookie missing, got: {set_cookie_headers:?}"
    );
    assert!(
        set_cookie_headers.iter().any(|c| c.starts_with("maison_refresh=")),
        "refresh token cookie missing, got: {set_cookie_headers:?}"
    );
}

async fn request_rust(
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<String>,
) -> (StatusCode, Value) {
    let config = test_config();
    let app = build_app_from_config(Arc::new(config))
        .expect("failed to build test app")
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }

    let request_body = body
        .map(|value| Body::from(serde_json::to_vec(&value).expect("body json should encode")))
        .unwrap_or_else(Body::empty);
    let request = builder.body(request_body).expect("request should build");

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
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        nabaztag_config_path: source_root.join("nabaztag.json"),
        nabaztag_host: None,
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

#[tokio::test]
async fn auth_refresh_without_cookie_returns_unauthorized() {
    let rust = request_rust(Method::POST, "/api/auth/refresh", None, None).await;

    assert_eq!(rust.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        rust.1,
        json!({
            "success": false,
            "error": "No refresh token"
        })
    );
}

#[tokio::test]
async fn auth_verify_without_token_returns_unauthorized() {
    let rust = request_rust(Method::POST, "/api/auth/verify", None, None).await;

    assert_eq!(rust.0, StatusCode::UNAUTHORIZED);
}

fn normalize_verify_response(value: Value) -> Value {
    json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "user": value.get("user").cloned().unwrap_or(Value::Null),
    })
}

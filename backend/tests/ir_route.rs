//! Regression tests for the /api/ir machine route: token auth (constant-time
//! bearer compare, fail-closed) and key-event dispatch semantics.

use std::{env, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode},
};
use maison_backend::{build_app_from_config, config::Config};
use serde_json::{Value, json};
use tower::ServiceExt;

const TEST_TOKEN: &str = "test-ir-token";

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}

fn test_config(keymap_path: std::path::PathBuf) -> Config {
    let source_root = workspace_root();
    let temp_root = std::env::temp_dir()
        .join("maison-rust-ir-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_root).expect("temp test dir should be created");

    Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        jwt_secret: env::var("JWT_SECRET")
            .unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string()),
        frontend_dist_dir: source_root.join("frontend").join("dist"),
        auth_cookie_name: "maison_session".to_string(),
        auth_cookie_secure: false,
        auth_rate_limit_attempts: 10,
        auth_rate_limit_window_seconds: 300,
        disable_bluetooth: true,
        users_path: source_root.join("users.json"),
        meross_devices_path: source_root.join("meross-devices.json"),
        devices_path: source_root.join("devices.json"),
        device_cache_path: temp_root.join("device-cache.json"),
        broadlink_codes_path: temp_root.join("broadlink-codes.json"),
        climate_state_path: temp_root.join("climate-state.json"),
        refresh_tokens_path: temp_root.join("refresh-tokens.json"),
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        nabaztag_config_path: temp_root.join("nabaztag.json"),
        nabaztag_host: None,
        zigbee_permit_join_seconds: 120,
        ir_keymap_path: keymap_path,
        ir_api_token: Some(TEST_TOKEN.to_string()),
        source_root,
    }
}

fn test_app(keymap_path: std::path::PathBuf) -> axum::Router {
    build_app_from_config(Arc::new(test_config(keymap_path)))
        .expect("app should build")
        .layer(MockConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            0,
        ))))
}

async fn post_key(
    app: &axum::Router,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/api/ir/key")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = builder
        .body(Body::from(body.to_string()))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json = serde_json::from_slice::<Value>(&bytes).expect("response should be valid json");
    (status, json)
}

/// Path to a keymap file that does not exist: empty keymap, feature loads fine.
fn missing_keymap() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("maison-rust-ir-tests")
        .join(format!("{}-missing.json", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn rejects_missing_token() {
    let app = test_app(missing_keymap());
    let (status, body) = post_key(&app, None, json!({"code": 1, "value": 1})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["success"], json!(false));
}

#[tokio::test]
async fn rejects_wrong_token() {
    let app = test_app(missing_keymap());
    let (status, _) = post_key(&app, Some("wrong-token"), json!({"code": 1, "value": 1})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_user_jwt_on_machine_route() {
    // A valid JWT is still not the machine token: the two auth paths are
    // deliberately not chained.
    let app = test_app(missing_keymap());
    let claims = serde_json::json!({
        "userId": "u1", "username": "test", "role": "admin",
        "exp": (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp()
    });
    let jwt = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(
            env::var("JWT_SECRET")
                .unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string())
                .as_bytes(),
        ),
    )
    .expect("jwt should encode");
    let (status, _) = post_key(&app, Some(&jwt), json!({"code": 1, "value": 1})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unmapped_key_is_a_200() {
    let app = test_app(missing_keymap());
    let (status, body) = post_key(&app, Some(TEST_TOKEN), json!({"code": 999, "value": 1})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], json!(true));
    assert!(
        body["message"].as_str().unwrap_or_default().contains("not mapped"),
        "unexpected message: {body}"
    );
}

#[tokio::test]
async fn release_and_repeat_are_ignored_without_repeat_flag() {
    let dir = std::env::temp_dir()
        .join("maison-rust-ir-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).expect("temp dir");
    let keymap = dir.join("ir-keymap.json");
    // Nabaztag action with no NABAZTAG_HOST configured: firing it would error,
    // proving values 0 and 2 never reach the action.
    std::fs::write(
        &keymap,
        r#"{ "207": { "actions": [{ "action": "nabaztag", "command": "dance 1" }] } }"#,
    )
    .expect("keymap written");
    let app = test_app(keymap);

    for value in [0, 2] {
        let (status, body) =
            post_key(&app, Some(TEST_TOKEN), json!({"code": 207, "value": value})).await;
        assert_eq!(status, StatusCode::OK, "value {value} should be ignored");
        assert!(
            body["message"].as_str().unwrap_or_default().contains("ignored"),
            "value {value}: unexpected message {body}"
        );
    }
}

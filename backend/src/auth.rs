use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{error::AppError, AppState};

#[derive(Clone, Default)]
pub struct AuthRateLimiter {
    inner: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

/// Server-side store for refresh tokens.
/// Maps opaque token strings to their associated user data and expiration.
/// Expired entries are evicted on each lookup to prevent unbounded growth.
/// Persisted to disk so a backend restart or redeploy does not log every
/// session out.
#[derive(Clone, Default)]
pub struct RefreshTokenStore {
    inner: Arc<Mutex<HashMap<String, RefreshEntry>>>,
    path: Option<Arc<PathBuf>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshEntry {
    pub user_id: String,
    pub username: String,
    pub role: String,
    /// Seconds since epoch when this refresh token expires.
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
struct RateLimitEntry {
    window_started_at: chrono::DateTime<Utc>,
    attempts: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct AuthRateLimitStatus {
    pub allowed: bool,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: String,
}

impl From<Claims> for AuthUser {
    fn from(value: Claims) -> Self {
        Self {
            id: value.user_id,
            username: value.username,
            role: value.role,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub AuthUser);

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = extract_auth_token(&parts.headers, &app_state.config.auth_cookie_name).map_err(|_| {
            AppError::http(
                StatusCode::UNAUTHORIZED,
                "Authentication required. Please provide a valid Bearer token.",
            )
        })?;

        let decoded = decode_token(token, app_state.config.jwt_secret.as_bytes())
            .map_err(|_| AppError::http(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(Self(decoded.claims.into()))
    }
}

/// Extractor that requires the authenticated user to have the `admin` role.
/// Use this for destructive or sensitive operations (device control, settings, etc.).
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if user.0.role != "admin" {
            return Err(AppError::http(
                StatusCode::FORBIDDEN,
                "Admin privileges required",
            ));
        }
        Ok(Self(user.0))
    }
}

/// Extractor for machine-to-machine clients (the kird IR bridge on the STB):
/// a raw bearer token compared in constant time against `IR_API_TOKEN`.
/// Fails closed when the token is not configured. Deliberately not chained
/// with the JWT path: a machine route accepts the machine token only.
#[derive(Debug, Clone)]
pub struct MachineClient;

impl<S> FromRequestParts<S> for MachineClient
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let Some(expected) = app_state.config.ir_api_token.as_deref() else {
            return Err(AppError::http(
                StatusCode::UNAUTHORIZED,
                "Machine API disabled: IR_API_TOKEN is not configured",
            ));
        };
        let token = extract_bearer_token(&parts.headers)?;
        if !constant_time_eq(token.as_bytes(), expected.as_bytes()) {
            return Err(AppError::unauthorized("Invalid machine token"));
        }
        Ok(Self)
    }
}

/// Byte-wise comparison without data-dependent early exit; the length check
/// short-circuits, which only reveals the token length, not its content.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn extract_auth_token<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Result<&'a str, AppError> {
    extract_bearer_token(headers).or_else(|_| extract_cookie_token(headers, cookie_name))
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let Some(header) = headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) else {
        return Err(AppError::unauthorized("No token provided"));
    };

    header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::unauthorized("No token provided"))
}

pub fn decode_token(
    token: &str,
    secret: &[u8],
) -> Result<jsonwebtoken::TokenData<Claims>, jsonwebtoken::errors::Error> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret), &Validation::default())
}

fn extract_cookie_token<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Result<&'a str, AppError> {
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("No token provided"))?;

    cookie_header
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then_some(value))
        .ok_or_else(|| AppError::unauthorized("No token provided"))
}

impl AuthRateLimiter {
    pub async fn check(&self, key: &str, max_attempts: u32, window: Duration) -> AuthRateLimitStatus {
        let now = Utc::now();
        let mut entries = self.inner.lock().await;

        // Evict expired entries to prevent unbounded memory growth.
        entries.retain(|_, entry| now - entry.window_started_at < window);

        let entry = entries.entry(key.to_string()).or_insert(RateLimitEntry {
            window_started_at: now,
            attempts: 0,
        });

        if now - entry.window_started_at >= window {
            entry.window_started_at = now;
            entry.attempts = 0;
        }

        entry.attempts = entry.attempts.saturating_add(1);

        AuthRateLimitStatus {
            allowed: entry.attempts <= max_attempts,
            attempts: entry.attempts,
        }
    }

    pub async fn reset(&self, key: &str) {
        self.inner.lock().await.remove(key);
    }
}

impl RefreshTokenStore {
    /// Load the store from disk (missing or corrupt file = empty store).
    /// Expired entries are dropped at load time.
    pub fn load(path: &Path) -> Self {
        let now = Utc::now().timestamp();
        let mut map = std::fs::read_to_string(path)
            .ok()
            .and_then(|content| {
                serde_json::from_str::<HashMap<String, RefreshEntry>>(content.trim()).ok()
            })
            .unwrap_or_default();
        map.retain(|_, entry| entry.expires_at > now);

        Self {
            inner: Arc::new(Mutex::new(map)),
            path: Some(Arc::new(path.to_path_buf())),
        }
    }

    /// Store a refresh token with its associated user data.
    pub async fn insert(&self, token: String, entry: RefreshEntry) {
        let mut map = self.inner.lock().await;
        map.insert(token, entry);
        self.persist(&map);
    }

    /// Look up a refresh token. Returns `None` if the token doesn't exist or is expired.
    /// Evicts all expired entries on each call.
    pub async fn validate(&self, token: &str) -> Option<RefreshEntry> {
        let now = Utc::now().timestamp();
        let mut map = self.inner.lock().await;
        // Evict expired entries to prevent unbounded growth.
        let before = map.len();
        map.retain(|_, entry| entry.expires_at > now);
        if map.len() != before {
            self.persist(&map);
        }
        map.get(token).cloned()
    }

    /// Remove a refresh token (used on logout and rotation).
    pub async fn remove(&self, token: &str) {
        let mut map = self.inner.lock().await;
        if map.remove(token).is_some() {
            self.persist(&map);
        }
    }

    /// Best-effort write-through; the tokens are bearer secrets, so the file
    /// is chmod 600. Failures are logged, never propagated: an unwritable
    /// disk must not break authentication.
    fn persist(&self, map: &HashMap<String, RefreshEntry>) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        let payload = match serde_json::to_string_pretty(map) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(%error, "failed to serialize refresh tokens");
                return;
            }
        };
        if let Err(error) = std::fs::write(path, format!("{payload}\n")) {
            tracing::warn!(%error, "failed to persist refresh tokens");
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub frontend_dist_dir: PathBuf,
    pub auth_cookie_name: String,
    pub auth_cookie_secure: bool,
    pub auth_rate_limit_attempts: u32,
    pub auth_rate_limit_window_seconds: i64,
    pub disable_bluetooth: bool,
    pub source_root: PathBuf,
    pub users_path: PathBuf,
    pub meross_devices_path: PathBuf,
    pub devices_path: PathBuf,
    pub device_cache_path: PathBuf,
    pub broadlink_codes_path: PathBuf,
    pub climate_state_path: PathBuf,
    pub refresh_tokens_path: PathBuf,
    pub hue_lamps_path: PathBuf,
    pub hue_blacklist_path: PathBuf,
    pub zigbee_lamps_path: PathBuf,
    pub zigbee_lamps_blacklist_path: PathBuf,
    pub nabaztag_config_path: PathBuf,
    pub nabaztag_host: Option<String>,
    pub zigbee_permit_join_seconds: u16,
    pub ir_keymap_path: PathBuf,
    pub tv_config_path: PathBuf,
    pub androidtv_config_path: PathBuf,
    pub adb_key_path: PathBuf,
    pub atv_identity_path: PathBuf,
    pub ir_api_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let source_root = env::var("MAISON_SOURCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_source_root());

        let users_path = env::var("USERS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("users.json"));

        let frontend_dist_dir = env::var("FRONTEND_DIST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("frontend").join("dist"));

        let meross_devices_path = env::var("MEROSS_DEVICES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("meross-devices.json"));

        let devices_path = env::var("DEVICES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("devices.json"));

        let device_cache_path = env::var("DEVICE_CACHE_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("device-cache.json"));

        let broadlink_codes_path = env::var("BROADLINK_CODES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("broadlink-codes.json"));

        let climate_state_path = env::var("CLIMATE_STATE_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("climate-state.json"));

        let refresh_tokens_path = env::var("REFRESH_TOKENS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("refresh-tokens.json"));

        let hue_lamps_path = env::var("HUE_LAMPS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("hue-lamps.json"));

        let hue_blacklist_path = env::var("HUE_BLACKLIST_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("hue-lamps-blacklist.json"));

        let zigbee_lamps_path = env::var("ZIGBEE_LAMPS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("zigbee-lamps.json"));

        let zigbee_lamps_blacklist_path = env::var("ZIGBEE_LAMPS_BLACKLIST_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("zigbee-lamps-blacklist.json"));

        let nabaztag_config_path = env::var("NABAZTAG_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("nabaztag.json"));

        let ir_keymap_path = env::var("IR_KEYMAP_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("ir-keymap.json"));

        let tv_config_path = env::var("TV_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("tv.json"));

        let androidtv_config_path = env::var("ANDROIDTV_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("androidtv.json"));

        // Private key authenticating this host to the box's adbd.
        let adb_key_path = env::var("ADB_KEY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("adb-key"));

        // TLS client identity for the Android TV Remote v2 protocol. Distinct
        // from the ADB key: the TV pairs against this certificate.
        let atv_identity_path = env::var("ATV_IDENTITY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("atv-identity"));

        // Machine token for the STB IR bridge (kird); empty/unset = IR API off.
        let ir_api_token = env::var("IR_API_TOKEN").ok().and_then(|value| {
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        });

        let nabaztag_host = env::var("NABAZTAG_HOST").ok().and_then(|value| {
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        });

        let disable_bluetooth = env::var("DISABLE_BLUETOOTH")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let auth_cookie_name =
            env::var("AUTH_COOKIE_NAME").unwrap_or_else(|_| "maison_session".to_string());

        let auth_cookie_secure = env::var("AUTH_COOKIE_SECURE")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);

        let auth_rate_limit_attempts = env::var("AUTH_RATE_LIMIT_ATTEMPTS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(10);

        let auth_rate_limit_window_seconds = env::var("AUTH_RATE_LIMIT_WINDOW_SECONDS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(300);

        let port = env::var("PORT")
            .ok()
            .or_else(|| env::var("API_PORT").ok())
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(3033);

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let jwt_secret =
            env::var("JWT_SECRET").unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string());
        let zigbee_permit_join_seconds = env::var("ZIGBEE_PERMIT_JOIN_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(120);

        Self {
            host,
            port,
            jwt_secret,
            frontend_dist_dir,
            auth_cookie_name,
            auth_cookie_secure,
            auth_rate_limit_attempts,
            auth_rate_limit_window_seconds,
            disable_bluetooth,
            source_root,
            users_path,
            meross_devices_path,
            devices_path,
            device_cache_path,
            broadlink_codes_path,
            climate_state_path,
            refresh_tokens_path,
            hue_lamps_path,
            hue_blacklist_path,
            zigbee_lamps_path,
            zigbee_lamps_blacklist_path,
            nabaztag_config_path,
            nabaztag_host,
            zigbee_permit_join_seconds,
            ir_keymap_path,
            tv_config_path,
            androidtv_config_path,
            adb_key_path,
            atv_identity_path,
            ir_api_token,
        }
    }

    #[cfg(test)]
    pub fn for_tests(source_root: PathBuf) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3033,
            jwt_secret: "test-secret".to_string(),
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
            atv_identity_path: source_root.join("atv-identity"),
            ir_api_token: Some("test-ir-token".to_string()),
            source_root,
        }
    }
}

fn default_source_root() -> PathBuf {
    if let Ok(current_dir) = env::current_dir() {
        if looks_like_source_root(&current_dir) {
            return current_dir;
        }
    }

    if let Ok(current_exe) = env::current_exe() {
        for ancestor in current_exe.ancestors() {
            if looks_like_source_root(ancestor) {
                return ancestor.to_path_buf();
            }
        }
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}

fn looks_like_source_root(path: &std::path::Path) -> bool {
    path.join("users.json").is_file() || path.join("frontend").is_dir()
}

//! Live checks against the real Android TV box, behind `live-runtime-tests`.
//!
//! Read-only: they send no keys and launch nothing, so running them cannot
//! disturb whatever is playing in the living room.
#![cfg(feature = "live-runtime-tests")]

use std::{env, path::PathBuf, sync::OnceLock};

use maison_backend::androidtv::AndroidTvManager;

static BOX: OnceLock<AndroidTvManager> = OnceLock::new();
static BOX_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn lock() -> &'static tokio::sync::Mutex<()> {
    BOX_TEST_LOCK.get_or_init(Default::default)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has a parent")
        .to_path_buf()
}

/// One shared manager so the tests reuse a single ADB connection and key.
fn manager() -> &'static AndroidTvManager {
    BOX.get_or_init(|| {
        let root = repo_root();
        let config = env::var("ANDROIDTV_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("androidtv.json"));
        let key = env::var("ADB_KEY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("adb-key"));
        AndroidTvManager::new(&config, &key).expect("manager should build")
    })
}

#[tokio::test]
async fn status_reports_a_reachable_box() {
    let _guard = lock().lock().await;
    let status = manager().status().await;
    assert!(status.configured, "androidtv.json should carry a host");
    assert!(status.reachable, "the box should answer ADB");
    assert_eq!(status.model.as_deref(), Some("LEAP-S1"));
}

#[tokio::test]
async fn lists_installed_packages() {
    let _guard = lock().lock().await;
    let packages = manager().apps().await.expect("package list");
    assert!(
        packages.iter().any(|p| p == "org.smarttube.beta"),
        "SmartTube should be installed: {packages:?}"
    );
}

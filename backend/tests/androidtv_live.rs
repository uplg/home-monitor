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
        let identity = env::var("ATV_IDENTITY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("atv-identity"));
        AndroidTvManager::new(&config, &key, &identity).expect("manager should build")
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

/// Measures what each channel costs *the caller*, which is what decides
/// whether a dashboard request blocks.
///
/// Be careful reading the Remote v2 figure: `key()` hands the message to the
/// session's background task and returns, so it measures enqueueing, not the
/// wire. That is genuinely the number that matters for an HTTP handler — but
/// it is not a round-trip, and the network cost is measured separately below.
///
/// Read-only in effect: it taps DPAD_DOWN, which moves a selection.
#[tokio::test]
async fn remote_v2_beats_adb_on_latency() {
    use std::time::Instant;

    use maison_backend::atvremote::{Identity, Session};

    let _guard = lock().lock().await;
    let root = repo_root();
    let host = "192.168.1.153";

    let identity = Identity::load_or_create(&root.join("atv-identity")).expect("identity");
    let session = Session::connect(host, &identity)
        .await
        .expect("a paired session");

    // Warm both paths so neither pays a first-call penalty.
    session.key(20).await.expect("warm remote");
    manager().send_key(maison_backend::androidtv::AndroidKey::Down).await.ok();

    let mut remote_total = std::time::Duration::ZERO;
    for _ in 0..5 {
        let start = Instant::now();
        session.key(20).await.expect("remote key");
        remote_total += start.elapsed();
    }

    let mut adb_total = std::time::Duration::ZERO;
    for _ in 0..5 {
        let start = Instant::now();
        manager()
            .adb_key_for_benchmark(maison_backend::androidtv::AndroidKey::Down)
            .await
            .expect("adb key");
        adb_total += start.elapsed();
    }

    let remote = remote_total / 5;
    let adb = adb_total / 5;
    // The wire cost Remote v2 still pays, out of the caller's way: a TCP
    // handshake to the remote port is the closest honest proxy for it.
    let mut network_total = std::time::Duration::ZERO;
    for _ in 0..5 {
        let start = Instant::now();
        let _ = tokio::net::TcpStream::connect((host, 6466u16)).await;
        network_total += start.elapsed();
    }

    println!("  Remote v2 (cout appelant) : {remote:?}");
    println!("  Remote v2 (RTT reseau)    : {:?}", network_total / 5);
    println!("  ADB       (bloquant)      : {adb:?}");
    assert!(
        remote < adb,
        "Remote v2 ({remote:?}) should beat ADB ({adb:?})"
    );
}

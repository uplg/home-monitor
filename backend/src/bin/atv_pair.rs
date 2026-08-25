//! Pairs this host with an Android TV over the Remote v2 protocol.
//!
//! Pairing is inherently interactive — the television shows a six hex-digit
//! code and the client has to prove it was read — so it lives in a small
//! binary rather than in a test.
//!
//!     cargo run --bin atv_pair -- 192.168.1.153 [path/to/atv-identity]
//!
//! The identity is generated on first run and reused afterwards; pairing only
//! has to happen once per host.

use std::{io::Write, path::PathBuf};

use maison_backend::atvremote::{Identity, Pairing, Session};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| {
        eprintln!("usage: atv_pair <host> [identity-path]");
        std::process::exit(2);
    });
    let identity_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("atv-identity"));

    println!("Loading identity from {}", identity_path.display());
    // Minting an RSA key is slow on small hardware, so keep it off the runtime.
    let path = identity_path.clone();
    let identity = tokio::task::spawn_blocking(move || Identity::load_or_create(&path)).await??;

    println!("Connecting to {host} for pairing…");
    let pairing = Pairing::start(&host, &identity).await?;

    print!("Code shown on the TV: ");
    std::io::stdout().flush()?;
    let mut code = String::new();
    std::io::stdin().read_line(&mut code)?;

    pairing.finish(&code).await?;
    println!("Paired.");

    // Prove the pairing is usable, not merely accepted.
    println!("Opening a remote session…");
    let session = Session::connect(&host, &identity).await?;
    println!("Session open. Sending KEYCODE_DPAD_DOWN…");
    session.key(20).await?;
    println!("Done — the selection should have moved on screen.");
    Ok(())
}

//! Sends a raw Broadlink IR packet, given as base64, through an RM-series
//! blaster.
//!
//! Learning a code needs the original remote in hand; synthesising one only
//! needs the protocol. This binary exists for the second case — proving a
//! generated packet before it is committed to `broadlink-codes.json`.
//!
//!     cargo run --bin send_ir -- <blaster-ip> <packet-base64> [local-ip]

use maison_backend::broadlink::BroadlinkManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or("usage: send_ir <blaster-ip> <packet-base64> [local-ip]")?;
    let packet = args.next().ok_or("missing packet-base64")?;
    let local_ip = args.next();

    // The manager only needs somewhere to look for state; sending a raw packet
    // touches neither the code store nor the climate state.
    let scratch = std::env::temp_dir();
    let manager = BroadlinkManager::new(
        &scratch.join("send-ir-codes.json"),
        &scratch.join("send-ir-climate.json"),
    )?;

    let result = manager
        .send_packet(host, local_ip, packet, None, None)
        .await?;
    println!("sent {} bytes to {}", result.packet_length, result.host);
    Ok(())
}

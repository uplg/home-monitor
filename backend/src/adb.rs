//! A minimal ADB client, spoken directly to the device over TCP.
//!
//! The Android box exposes `adbd` on port 5555, and the wire protocol is
//! simple enough that talking to it directly beats shipping Google's `adb`
//! binary to an ARMv6 Alpine box — the existing Rust crates all drive the
//! local `adb` *server* (a second daemon on :5037), which is exactly the
//! dependency we are avoiding.
//!
//! Framing is a 24-byte header followed by the payload, all little-endian:
//! command, arg0, arg1, payload length, payload checksum, and a magic word
//! that is the command with every bit flipped. The "checksum" is the plain
//! sum of the payload bytes, not a CRC, despite the field name used in
//! Google's source.
//!
//! Authentication is the interesting part. On connect the device replies with
//! `AUTH TOKEN` carrying 20 random bytes; the client signs them with RSA-2048
//! (PKCS#1 v1.5, SHA-1 DigestInfo — the token *is* the digest, so it is
//! signed pre-hashed) and answers `AUTH SIGNATURE`. A device that has never
//! seen the key rejects that and re-sends its token; the client then sends
//! `AUTH RSAPUBLICKEY`, which is what raises the "Allow USB debugging?"
//! prompt on the television. Accepting it persists the key on the device, so
//! this only happens once.

use std::time::Duration;

use rsa::{
    pkcs1v15::SigningKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    signature::{hazmat::PrehashSigner, SignatureEncoding},
    traits::PublicKeyParts,
    BigUint, RsaPrivateKey, RsaPublicKey,
};
use sha1::Sha1;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::error::AppError;

const A_CNXN: u32 = 0x4e58_4e43;
const A_AUTH: u32 = 0x4854_5541;
const A_OPEN: u32 = 0x4e45_504f;
const A_OKAY: u32 = 0x5941_4b4f;
const A_CLSE: u32 = 0x4553_4c43;
const A_WRTE: u32 = 0x4554_5257;

const A_VERSION: u32 = 0x0100_0000;
const MAX_PAYLOAD: u32 = 256 * 1024;

const AUTH_TOKEN: u32 = 1;
const AUTH_SIGNATURE: u32 = 2;
const AUTH_RSAPUBLICKEY: u32 = 3;

const RSA_BITS: usize = 2048;
/// The device's public-key blob counts 32-bit words, not bytes.
const KEY_WORDS: usize = RSA_BITS / 32;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const IO_TIMEOUT: Duration = Duration::from_secs(15);
/// After offering the public key the device shows "Allow USB debugging?" and
/// waits for a human. A session cut short here leaves the key unauthorised,
/// so the prompt gets its own, far longer budget.
pub const AUTHORISE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
struct Message {
    command: u32,
    arg0: u32,
    arg1: u32,
    payload: Vec<u8>,
}

impl Message {
    fn new(command: u32, arg0: u32, arg1: u32, payload: Vec<u8>) -> Self {
        Self {
            command,
            arg0,
            arg1,
            payload,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let checksum = self
            .payload
            .iter()
            .fold(0u32, |acc, byte| acc.wrapping_add(u32::from(*byte)));
        let mut out = Vec::with_capacity(24 + self.payload.len());
        out.extend_from_slice(&self.command.to_le_bytes());
        out.extend_from_slice(&self.arg0.to_le_bytes());
        out.extend_from_slice(&self.arg1.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&checksum.to_le_bytes());
        out.extend_from_slice(&(self.command ^ 0xffff_ffff).to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

fn protocol_error(message: impl Into<String>) -> AppError {
    AppError::service_unavailable(message.into())
}

async fn write_message(stream: &mut TcpStream, message: Message) -> Result<(), AppError> {
    timeout(IO_TIMEOUT, stream.write_all(&message.encode()))
        .await
        .map_err(|_| protocol_error("timed out writing to the ADB device"))??;
    Ok(())
}

async fn read_message(stream: &mut TcpStream) -> Result<Message, AppError> {
    read_message_within(stream, IO_TIMEOUT).await
}

async fn read_message_within(
    stream: &mut TcpStream,
    budget: Duration,
) -> Result<Message, AppError> {
    let mut header = [0u8; 24];
    timeout(budget, stream.read_exact(&mut header))
        .await
        .map_err(|_| protocol_error("timed out reading from the ADB device"))??;

    let word = |index: usize| {
        u32::from_le_bytes([
            header[index],
            header[index + 1],
            header[index + 2],
            header[index + 3],
        ])
    };
    let command = word(0);
    let length = word(12) as usize;

    if command ^ 0xffff_ffff != word(20) {
        return Err(protocol_error("ADB header failed its magic check"));
    }
    if length > MAX_PAYLOAD as usize {
        return Err(protocol_error("ADB payload larger than the negotiated max"));
    }

    let mut payload = vec![0u8; length];
    if length > 0 {
        timeout(IO_TIMEOUT, stream.read_exact(&mut payload))
            .await
            .map_err(|_| protocol_error("timed out reading an ADB payload"))??;
    }

    Ok(Message::new(command, word(4), word(8), payload))
}

/// Generates a fresh 2048-bit key. Slow on an ARMv6 Pi (tens of seconds), so
/// callers must run this off the async runtime and persist the result.
pub fn generate_key() -> Result<RsaPrivateKey, AppError> {
    RsaPrivateKey::new(&mut rand::thread_rng(), RSA_BITS)
        .map_err(|error| protocol_error(format!("could not generate an ADB key: {error}")))
}

pub fn encode_key(key: &RsaPrivateKey) -> Result<Vec<u8>, AppError> {
    key.to_pkcs8_der()
        .map(|der| der.as_bytes().to_vec())
        .map_err(|error| protocol_error(format!("could not serialize the ADB key: {error}")))
}

pub fn decode_key(der: &[u8]) -> Result<RsaPrivateKey, AppError> {
    RsaPrivateKey::from_pkcs8_der(der)
        .map_err(|error| protocol_error(format!("could not read the ADB key: {error}")))
}

/// Android's own public-key encoding, which is neither PKCS#1 nor SPKI: a
/// packed struct of little-endian 32-bit words carrying the modulus plus two
/// values the device's Montgomery reduction needs precomputed — `n0inv` and
/// `rr`. Base64 of that, then a space and a comment, is what `adbd` stores
/// once the user accepts the prompt.
pub fn android_public_key(key: &RsaPrivateKey, comment: &str) -> String {
    use base64::Engine as _;

    let public = RsaPublicKey::from(key);
    let modulus = public.n();

    // The blob stores little-endian 32-bit words, which is exactly how the
    // modulus serializes byte-wise, so go through bytes and repack.
    let to_words = |value: &BigUint| {
        let bytes = value.to_bytes_le();
        let mut words = vec![0u32; KEY_WORDS];
        for (index, chunk) in bytes.chunks(4).take(KEY_WORDS).enumerate() {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            words[index] = u32::from_le_bytes(word);
        }
        words
    };

    let modulus_words = to_words(modulus);

    // n0inv = -(n^-1) mod 2^32. Newton's iteration doubles the number of
    // correct bits each round, so five rounds cover all 32.
    let n0 = modulus_words[0];
    let mut inverse = 1u32;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inverse)));
    }
    let n0inv = inverse.wrapping_neg();

    // rr = R^2 mod n, with R = 2^2048.
    let rr = BigUint::from(1u32) << (RSA_BITS * 2);
    let rr = rr % modulus;

    let mut blob = Vec::with_capacity(4 + 4 + KEY_WORDS * 8 + 4);
    blob.extend_from_slice(&(KEY_WORDS as u32).to_le_bytes());
    blob.extend_from_slice(&n0inv.to_le_bytes());
    for word in modulus_words {
        blob.extend_from_slice(&word.to_le_bytes());
    }
    for word in to_words(&rr) {
        blob.extend_from_slice(&word.to_le_bytes());
    }
    blob.extend_from_slice(&65537u32.to_le_bytes());

    format!(
        "{} {}",
        base64::engine::general_purpose::STANDARD.encode(&blob),
        comment
    )
}

/// One authenticated connection to `adbd`. Streams are opened one at a time,
/// which is all this module needs and keeps the multiplexing trivial.
pub struct AdbDevice {
    stream: TcpStream,
    next_local_id: u32,
    /// Id of the stream currently open; streams are used one at a time.
    local_id: u32,
}

impl AdbDevice {
    pub async fn connect(address: &str, key: &RsaPrivateKey) -> Result<Self, AppError> {
        let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
            .await
            .map_err(|_| protocol_error(format!("ADB connect to {address} timed out")))??;
        let mut device = Self {
            stream,
            next_local_id: 1,
            local_id: 0,
        };
        device.handshake(key).await?;
        Ok(device)
    }

    async fn handshake(&mut self, key: &RsaPrivateKey) -> Result<(), AppError> {
        write_message(
            &mut self.stream,
            Message::new(
                A_CNXN,
                A_VERSION,
                MAX_PAYLOAD,
                b"host::features=shell_v2,cmd\0".to_vec(),
            ),
        )
        .await?;

        let mut signed = false;
        let mut offered_key = false;

        loop {
            let budget = if offered_key {
                AUTHORISE_TIMEOUT
            } else {
                IO_TIMEOUT
            };
            let message = read_message_within(&mut self.stream, budget).await?;
            if std::env::var("MAISON_ADB_DEBUG").is_ok() {
                eprintln!(
                    "  <- cmd={:08x} ({}) arg0={} arg1={} len={}",
                    message.command,
                    String::from_utf8_lossy(&message.command.to_le_bytes()),
                    message.arg0,
                    message.arg1,
                    message.payload.len()
                );
            }
            match message.command {
                A_CNXN => return Ok(()),
                A_AUTH if message.arg0 == AUTH_TOKEN => {
                    if !signed {
                        // The 20-byte token *is* the SHA-1 digest, so it must
                        // be signed pre-hashed. Hashing it again (the plain
                        // `Signer` path) produces a signature the device
                        // silently rejects, falling back to re-sending the
                        // public key on every single connection.
                        let signing_key = SigningKey::<Sha1>::new(key.clone());
                        let signature = signing_key
                            .sign_prehash(&message.payload)
                            .map_err(|error| {
                                protocol_error(format!("could not sign the ADB token: {error}"))
                            })?
                            .to_vec();
                        write_message(
                            &mut self.stream,
                            Message::new(A_AUTH, AUTH_SIGNATURE, 0, signature),
                        )
                        .await?;
                        signed = true;
                    } else if !offered_key {
                        // The device does not know this key: offer it, which
                        // is what raises the prompt on the television.
                        let mut blob = android_public_key(key, "maison@maison").into_bytes();
                        blob.push(0);
                        write_message(
                            &mut self.stream,
                            Message::new(A_AUTH, AUTH_RSAPUBLICKEY, 0, blob),
                        )
                        .await?;
                        offered_key = true;
                    } else {
                        return Err(protocol_error(
                            "ADB authorisation refused — accept the prompt on the TV screen",
                        ));
                    }
                }
                // Anything else before CNXN is noise from a previous session.
                _ => continue,
            }
        }
    }

    /// Opens a stream to `destination` (`shell:…`, `sync:`, …) and returns
    /// its remote id once the device has acknowledged it.
    async fn open(&mut self, destination: &str) -> Result<u32, AppError> {
        let local_id = self.next_local_id;
        self.next_local_id = self.next_local_id.wrapping_add(1).max(1);

        let mut payload = destination.as_bytes().to_vec();
        payload.push(0);
        write_message(&mut self.stream, Message::new(A_OPEN, local_id, 0, payload)).await?;

        loop {
            let message = read_message(&mut self.stream).await?;
            match message.command {
                A_OKAY if message.arg1 == local_id => {
                    self.local_id = local_id;
                    return Ok(message.arg0);
                }
                A_CLSE if message.arg1 == local_id => {
                    return Err(protocol_error(format!("device refused to open {destination}")))
                }
                _ => continue,
            }
        }
    }

    /// Writes one payload to an open stream and waits for its acknowledgement.
    /// ADB is strictly lock-step here: sending again before the OKAY arrives
    /// wedges the connection.
    async fn write_stream(&mut self, remote_id: u32, data: &[u8]) -> Result<(), AppError> {
        write_message(
            &mut self.stream,
            Message::new(A_WRTE, self.local_id, remote_id, data.to_vec()),
        )
        .await?;
        loop {
            let message = read_message(&mut self.stream).await?;
            match message.command {
                A_OKAY if message.arg1 == self.local_id => return Ok(()),
                A_CLSE if message.arg1 == self.local_id => {
                    return Err(protocol_error("device closed the stream mid-write"))
                }
                _ => continue,
            }
        }
    }

    /// Runs one shell command and returns everything it wrote.
    pub async fn shell(&mut self, command: &str) -> Result<String, AppError> {
        let remote_id = self.open(&format!("shell:{command}")).await?;
        let local_id = self.local_id;
        let mut output = Vec::new();

        loop {
            let message = read_message(&mut self.stream).await?;
            match message.command {
                A_WRTE if message.arg1 == local_id => {
                    output.extend_from_slice(&message.payload);
                    // Every WRTE must be acknowledged or the device stalls.
                    write_message(
                        &mut self.stream,
                        Message::new(A_OKAY, local_id, message.arg0, Vec::new()),
                    )
                    .await?;
                }
                A_CLSE if message.arg1 == local_id => {
                    write_message(
                        &mut self.stream,
                        Message::new(A_CLSE, local_id, remote_id, Vec::new()),
                    )
                    .await?;
                    break;
                }
                _ => continue,
            }
        }

        Ok(String::from_utf8_lossy(&output).into_owned())
    }

    /// Pushes a file with the `sync:` service — the transport `adb push` uses.
    ///
    /// Sync framing is its own little protocol carried inside the stream: a
    /// four-byte id, a little-endian length, then the payload. `SEND` names
    /// the target as `path,mode`, `DATA` carries at most 64 KiB per chunk,
    /// and `DONE` passes the mtime and asks for the verdict.
    pub async fn push(&mut self, remote_path: &str, data: &[u8], mode: u32) -> Result<(), AppError> {
        const SYNC_CHUNK: usize = 64 * 1024;

        let remote_id = self.open("sync:").await?;
        let local_id = self.local_id;

        let target = format!("{remote_path},{mode}");
        let mut header = Vec::from(*b"SEND");
        header.extend_from_slice(&(target.len() as u32).to_le_bytes());
        header.extend_from_slice(target.as_bytes());
        self.write_stream(remote_id, &header).await?;

        for chunk in data.chunks(SYNC_CHUNK) {
            let mut frame = Vec::with_capacity(8 + chunk.len());
            frame.extend_from_slice(b"DATA");
            frame.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            frame.extend_from_slice(chunk);
            self.write_stream(remote_id, &frame).await?;
        }

        let mut done = Vec::from(*b"DONE");
        // mtime; the device only stores it, so a fixed value is fine.
        done.extend_from_slice(&0u32.to_le_bytes());
        self.write_stream(remote_id, &done).await?;

        // The verdict arrives as OKAY or FAIL in a WRTE.
        let mut verdict = Vec::new();
        loop {
            let message = read_message(&mut self.stream).await?;
            match message.command {
                A_WRTE if message.arg1 == local_id => {
                    verdict.extend_from_slice(&message.payload);
                    write_message(
                        &mut self.stream,
                        Message::new(A_OKAY, local_id, message.arg0, Vec::new()),
                    )
                    .await?;
                    if verdict.len() >= 8 {
                        break;
                    }
                }
                A_CLSE if message.arg1 == local_id => break,
                _ => continue,
            }
        }

        write_message(
            &mut self.stream,
            Message::new(A_CLSE, local_id, remote_id, Vec::new()),
        )
        .await?;
        // The sync stream is single-use; force a fresh connection next time.
        if verdict.starts_with(b"OKAY") {
            Ok(())
        } else {
            Err(protocol_error(format!(
                "device rejected the file transfer: {}",
                String::from_utf8_lossy(&verdict)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_header_is_little_endian_with_summed_payload() {
        let encoded = Message::new(A_OPEN, 7, 0, b"abc".to_vec()).encode();
        assert_eq!(&encoded[0..4], &A_OPEN.to_le_bytes());
        assert_eq!(&encoded[4..8], &7u32.to_le_bytes());
        assert_eq!(&encoded[12..16], &3u32.to_le_bytes());
        // Checksum is the plain byte sum: 'a' + 'b' + 'c'.
        assert_eq!(&encoded[16..20], &(97u32 + 98 + 99).to_le_bytes());
        assert_eq!(&encoded[20..24], &(A_OPEN ^ 0xffff_ffff).to_le_bytes());
        assert_eq!(&encoded[24..], b"abc");
    }

    #[test]
    fn magic_word_is_the_complement_of_the_command() {
        for command in [A_CNXN, A_AUTH, A_OPEN, A_OKAY, A_CLSE, A_WRTE] {
            let encoded = Message::new(command, 0, 0, Vec::new()).encode();
            let magic = u32::from_le_bytes([encoded[20], encoded[21], encoded[22], encoded[23]]);
            assert_eq!(command ^ 0xffff_ffff, magic);
        }
    }

    /// The blob is a fixed-size struct; getting its length or the trailing
    /// exponent wrong is the classic way to have the TV silently ignore the
    /// key and never show the prompt.
    #[test]
    fn android_public_key_blob_has_the_expected_shape() {
        use base64::Engine as _;

        // A small key keeps the test fast; the encoding is size-driven, and
        // the blob is padded to 2048 bits either way.
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 512).expect("key");
        let encoded = android_public_key(&key, "maison@test");
        let (blob, comment) = encoded.split_once(' ').expect("comment is space-separated");
        assert_eq!(comment, "maison@test");

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(blob)
            .expect("valid base64");
        assert_eq!(decoded.len(), 4 + 4 + KEY_WORDS * 4 * 2 + 4);
        assert_eq!(&decoded[0..4], &(KEY_WORDS as u32).to_le_bytes());
        assert_eq!(&decoded[decoded.len() - 4..], &65537u32.to_le_bytes());
    }

    /// n0inv is what lets the device do Montgomery reduction; its defining
    /// property is n * n0inv ≡ -1 (mod 2^32).
    #[test]
    fn n0inv_satisfies_its_montgomery_identity() {
        use base64::Engine as _;

        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 512).expect("key");
        let encoded = android_public_key(&key, "c");
        let blob = base64::engine::general_purpose::STANDARD
            .decode(encoded.split_once(' ').expect("comment").0)
            .expect("valid base64");

        let n0inv = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
        let n0 = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
        // -1 (mod 2^32) is u32::MAX.
        assert_eq!(n0.wrapping_mul(n0inv), u32::MAX);
    }

    /// Real handshake against the box. Ignored by default: it needs the
    /// hardware and, on a first run with a fresh key, someone to accept the
    /// prompt on the television.
    #[tokio::test]
    #[ignore]
    async fn live_handshake_against_the_box() {
        let address = std::env::var("ADB_TEST_ADDRESS")
            .unwrap_or_else(|_| "192.168.1.153:5555".to_string());
        let key_path = std::path::PathBuf::from("/tmp/maison-adb-test.key");

        let key = match std::fs::read(&key_path) {
            Ok(der) => decode_key(&der).expect("stored key"),
            Err(_) => {
                let key = generate_key().expect("generate");
                std::fs::write(&key_path, encode_key(&key).expect("encode")).expect("persist");
                key
            }
        };

        let mut device = AdbDevice::connect(&address, &key)
            .await
            .expect("handshake should complete (accept the prompt on the TV)");
        let model = device.shell("getprop ro.product.model").await.expect("shell");
        println!("model = {}", model.trim());
        assert!(!model.trim().is_empty());
    }

    /// Exercises the sync protocol against the real box: push a file, read
    /// it back through the shell, then delete it.
    #[tokio::test]
    #[ignore]
    async fn live_push_round_trip() {
        let address = std::env::var("ADB_TEST_ADDRESS")
            .unwrap_or_else(|_| "192.168.1.153:5555".to_string());
        let key = decode_key(&std::fs::read("/tmp/maison-adb-test.key").expect("key file"))
            .expect("key");

        let mut device = AdbDevice::connect(&address, &key).await.expect("connect");
        let payload = b"maison sync check";
        device
            .push("/data/local/tmp/maison-sync-check", payload, 0o644)
            .await
            .expect("push should be accepted");

        let mut shell = AdbDevice::connect(&address, &key).await.expect("connect");
        let read_back = shell
            .shell("cat /data/local/tmp/maison-sync-check")
            .await
            .expect("cat");
        let _ = shell.shell("rm -f /data/local/tmp/maison-sync-check").await;

        assert_eq!(read_back.trim(), String::from_utf8_lossy(payload));
    }

    #[test]
    fn keys_round_trip_through_pkcs8() {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 512).expect("key");
        let der = encode_key(&key).expect("encode");
        assert_eq!(decode_key(&der).expect("decode"), key);
    }
}

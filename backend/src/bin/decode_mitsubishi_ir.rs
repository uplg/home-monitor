use std::{
    env, fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use maison_backend::broadlink::BroadlinkCodeEntry;
use serde::Deserialize;

const BROADLINK_IR_TOKEN: u8 = 0x26;
const BROADLINK_HEADER_LEN: usize = 4;
const MITSUBISHI_STATE_LEN: usize = 18;
const MITSUBISHI_FRAME_DURATIONS: usize = 2 + (MITSUBISHI_STATE_LEN * 8 * 2) + 1;
const MITSUBISHI_HDR_MARK_US: u32 = 3400;
const MITSUBISHI_HDR_SPACE_US: u32 = 1750;
const MITSUBISHI_BIT_MARK_US: u32 = 450;
const MITSUBISHI_ONE_SPACE_US: u32 = 1300;
const MITSUBISHI_ZERO_SPACE_US: u32 = 420;
const MITSUBISHI_REPEAT_GAP_US: u32 = 15500;
const BROADLINK_TICK_US: f32 = 32.84;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCodes {
    codes: Vec<BroadlinkCodeEntry>,
}

#[derive(Debug)]
struct DecodedPacket {
    durations_us: Vec<u32>,
    repeat_gaps_us: Vec<u32>,
    frames: Vec<MitsubishiFrame>,
}

#[derive(Debug)]
struct MitsubishiFrame {
    header_mark_us: u32,
    header_space_us: u32,
    footer_mark_us: u32,
    bytes: [u8; MITSUBISHI_STATE_LEN],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_codes_path);
    let payload = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let stored = serde_json::from_str::<StoredCodes>(&payload)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;

    println!("File: {}", path.display());
    println!("Codes: {}", stored.codes.len());
    println!();

    for code in &stored.codes {
        match decode_packet(&code.packet_base64) {
            Ok(packet) => print_code_report(code, &packet),
            Err(error) => {
                println!("{} ({})", code.name, code.command);
                println!("  decode error: {error}");
                println!();
            }
        }
    }

    Ok(())
}

fn default_codes_path() -> PathBuf {
    let candidates = [
        PathBuf::from("broadlink-codes.json"),
        PathBuf::from("../broadlink-codes.json"),
    ];

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from("broadlink-codes.json"))
}

fn decode_packet(packet_base64: &str) -> Result<DecodedPacket, String> {
    let bytes = STANDARD
        .decode(packet_base64)
        .map_err(|error| format!("invalid base64: {error}"))?;

    if bytes.len() <= BROADLINK_HEADER_LEN {
        return Err("packet too short".to_string());
    }

    if bytes[0] != BROADLINK_IR_TOKEN {
        return Err(format!(
            "unsupported Broadlink packet token 0x{:02X}",
            bytes[0]
        ));
    }

    let durations_us = decode_broadlink_durations(&bytes[BROADLINK_HEADER_LEN..])?;
    let (frames, repeat_gaps_us) = decode_mitsubishi_frames(&durations_us)?;

    Ok(DecodedPacket {
        durations_us,
        repeat_gaps_us,
        frames,
    })
}

fn decode_broadlink_durations(encoded: &[u8]) -> Result<Vec<u32>, String> {
    let mut durations_us = Vec::new();
    let mut index = 0;

    while index < encoded.len() {
        let value = encoded[index];
        if value == 0 {
            if encoded.len() - index == 2 && encoded[index + 1] == 0x0D {
                break;
            }

            if index + 2 >= encoded.len() {
                return Err("truncated extended Broadlink duration".to_string());
            }

            let ticks = u16::from_be_bytes([encoded[index + 1], encoded[index + 2]]) as u32;
            durations_us.push(ticks_to_micros(ticks));
            index += 3;
        } else {
            durations_us.push(ticks_to_micros(value as u32));
            index += 1;
        }
    }

    Ok(durations_us)
}

fn decode_mitsubishi_frames(
    durations_us: &[u32],
) -> Result<(Vec<MitsubishiFrame>, Vec<u32>), String> {
    let mut frames = Vec::new();
    let mut repeat_gaps_us = Vec::new();
    let mut index = 0;

    while index + MITSUBISHI_FRAME_DURATIONS <= durations_us.len() {
        let frame_slice = &durations_us[index..index + MITSUBISHI_FRAME_DURATIONS];
        frames.push(decode_frame(frame_slice)?);
        index += MITSUBISHI_FRAME_DURATIONS;

        if index < durations_us.len() {
            let gap = durations_us[index];
            if gap > 5000 {
                repeat_gaps_us.push(gap);
                index += 1;
            } else {
                break;
            }
        }
    }

    if frames.is_empty() {
        return Err("no Mitsubishi 144-bit frame found".to_string());
    }

    Ok((frames, repeat_gaps_us))
}

fn decode_frame(durations_us: &[u32]) -> Result<MitsubishiFrame, String> {
    if durations_us.len() != MITSUBISHI_FRAME_DURATIONS {
        return Err(format!(
            "unexpected Mitsubishi frame duration count {}",
            durations_us.len()
        ));
    }

    let header_mark_us = durations_us[0];
    let header_space_us = durations_us[1];
    let footer_mark_us = durations_us[MITSUBISHI_FRAME_DURATIONS - 1];
    let bit_pairs = &durations_us[2..MITSUBISHI_FRAME_DURATIONS - 1];

    let mut bytes = [0_u8; MITSUBISHI_STATE_LEN];
    for (bit_index, pair) in bit_pairs.chunks_exact(2).enumerate() {
        let space = pair[1];
        let bit = if space > ((MITSUBISHI_ONE_SPACE_US + MITSUBISHI_ZERO_SPACE_US) / 2) {
            1_u8
        } else {
            0_u8
        };
        bytes[bit_index / 8] |= bit << (bit_index % 8);
    }

    Ok(MitsubishiFrame {
        header_mark_us,
        header_space_us,
        footer_mark_us,
        bytes,
    })
}

fn print_code_report(code: &BroadlinkCodeEntry, packet: &DecodedPacket) {
    println!("{} ({})", code.name, code.command);
    println!(
        "  packet: {} bytes, {} durations, {} frame(s)",
        code.packet_length,
        packet.durations_us.len(),
        packet.frames.len()
    );

    if !packet.repeat_gaps_us.is_empty() {
        println!("  repeat gaps: {}", join_u32(&packet.repeat_gaps_us));
    }

    let first_frame = &packet.frames[0];
    let repeated = packet
        .frames
        .iter()
        .skip(1)
        .all(|frame| frame.bytes == first_frame.bytes);

    println!(
        "  timings: header={} / {} (expected {} / {}), bit mark~{}, zero~{}, one~{}, footer~{}, repeat={} ; repeated={}",
        first_frame.header_mark_us,
        first_frame.header_space_us,
        MITSUBISHI_HDR_MARK_US,
        MITSUBISHI_HDR_SPACE_US,
        MITSUBISHI_BIT_MARK_US,
        MITSUBISHI_ZERO_SPACE_US,
        MITSUBISHI_ONE_SPACE_US,
        first_frame.footer_mark_us,
        MITSUBISHI_REPEAT_GAP_US,
        if repeated { "yes" } else { "no" }
    );
    println!("  raw: {}", format_hex_bytes(&first_frame.bytes));
    println!(
        "  checksum: {:02X} ({})",
        first_frame.bytes[MITSUBISHI_STATE_LEN - 1],
        if checksum_valid(&first_frame.bytes) {
            "valid"
        } else {
            "invalid"
        }
    );

    let state = interpret_state(&first_frame.bytes);
    println!("  power: {}", on_off(state.power));
    println!("  mode: {} (0b{:03b})", mode_name(state.mode), state.mode);
    println!(
        "  temperature: {} C",
        format_temperature(state.temperature_half_degrees)
    );
    println!(
        "  fan: {} (code={}, auto={})",
        fan_name(state.fan_code, state.fan_auto),
        state.fan_code,
        on_off(state.fan_auto)
    );
    println!(
        "  vane vertical: {} (code={}, bit={})",
        vertical_vane_name(state.vertical_vane_code),
        state.vertical_vane_code,
        on_off(state.vertical_vane_enabled)
    );
    println!(
        "  vane horizontal: {} (code={})",
        horizontal_vane_name(state.horizontal_vane_code),
        state.horizontal_vane_code
    );
    println!(
        "  timers: current={}, clock={}, start={}, stop={}, weekly={}",
        timer_name(state.timer_mode),
        format_clock_value(state.clock),
        format_clock_value(state.start_clock),
        format_clock_value(state.stop_clock),
        on_off(state.weekly_timer)
    );
    println!(
        "  extras: i-see={}, econo={}, natural-flow={}, absence={}, i-save-10c={}, direct-indirect={}, left-vane={}",
        on_off(state.i_see),
        on_off(state.ecocool),
        on_off(state.natural_flow),
        on_off(state.absence_detect),
        on_off(state.i_save_10c),
        direct_indirect_name(state.direct_indirect),
        vertical_vane_name(state.left_vane_code)
    );
    println!();
}

#[derive(Debug)]
struct InterpretedState {
    power: bool,
    mode: u8,
    i_see: bool,
    temperature_half_degrees: u8,
    horizontal_vane_code: u8,
    fan_code: u8,
    fan_auto: bool,
    vertical_vane_code: u8,
    vertical_vane_enabled: bool,
    clock: u8,
    stop_clock: u8,
    start_clock: u8,
    timer_mode: u8,
    weekly_timer: bool,
    ecocool: bool,
    direct_indirect: u8,
    absence_detect: bool,
    i_save_10c: bool,
    natural_flow: bool,
    left_vane_code: u8,
}

fn interpret_state(bytes: &[u8; MITSUBISHI_STATE_LEN]) -> InterpretedState {
    InterpretedState {
        power: bytes[5] & 0x20 != 0,
        mode: (bytes[6] >> 3) & 0x07,
        i_see: bytes[6] & 0x40 != 0,
        temperature_half_degrees: ((bytes[7] & 0x0F) * 2)
            + if bytes[7] & 0x10 != 0 { 1 } else { 0 },
        horizontal_vane_code: bytes[8] >> 4,
        fan_code: bytes[9] & 0x07,
        vertical_vane_code: (bytes[9] >> 3) & 0x07,
        vertical_vane_enabled: bytes[9] & 0x40 != 0,
        fan_auto: bytes[9] & 0x80 != 0,
        clock: bytes[10],
        stop_clock: bytes[11],
        start_clock: bytes[12],
        timer_mode: bytes[13] & 0x07,
        weekly_timer: bytes[13] & 0x08 != 0,
        ecocool: bytes[14] & 0x20 != 0,
        direct_indirect: bytes[15] & 0x03,
        absence_detect: bytes[15] & 0x04 != 0,
        i_save_10c: bytes[15] & 0x20 != 0,
        natural_flow: bytes[16] & 0x02 != 0,
        left_vane_code: (bytes[16] >> 3) & 0x07,
    }
}

fn checksum_valid(bytes: &[u8; MITSUBISHI_STATE_LEN]) -> bool {
    bytes[..MITSUBISHI_STATE_LEN - 1]
        .iter()
        .copied()
        .fold(0_u8, |sum, byte| sum.wrapping_add(byte))
        == bytes[MITSUBISHI_STATE_LEN - 1]
}

fn ticks_to_micros(ticks: u32) -> u32 {
    ((ticks as f32) * BROADLINK_TICK_US).round() as u32
}

fn format_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn join_u32(values: &[u32]) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

fn mode_name(mode: u8) -> &'static str {
    match mode {
        0b100 => "auto",
        0b011 => "cool",
        0b010 => "dry",
        0b001 => "heat",
        0b111 => "fan",
        _ => "unknown",
    }
}

fn fan_name(fan_code: u8, fan_auto: bool) -> &'static str {
    if fan_auto {
        return "auto";
    }

    match fan_code {
        1 => "level-1",
        2 => "level-2",
        3 => "level-3",
        4 => "max",
        5 => "silent",
        _ => "unknown",
    }
}

fn vertical_vane_name(code: u8) -> &'static str {
    match code {
        0b000 => "auto",
        0b001 => "highest",
        0b010 => "high",
        0b011 => "middle",
        0b100 => "low",
        0b101 => "lowest",
        0b111 => "swing",
        _ => "unknown",
    }
}

fn horizontal_vane_name(code: u8) -> &'static str {
    match code {
        0b0001 => "far-left",
        0b0010 => "left",
        0b0011 => "center",
        0b0100 => "right",
        0b0101 => "far-right",
        0b0110 => "wide",
        0b1000 => "auto",
        _ => "unknown",
    }
}

fn timer_name(code: u8) -> &'static str {
    match code {
        0 => "none",
        3 => "stop",
        5 => "start",
        7 => "start+stop",
        _ => "unknown",
    }
}

fn direct_indirect_name(code: u8) -> &'static str {
    match code {
        0b00 => "off",
        0b01 => "indirect",
        0b11 => "direct",
        _ => "unknown",
    }
}

fn format_clock_value(raw_value: u8) -> String {
    if raw_value == 0 {
        return "--:--".to_string();
    }

    let total_minutes = (raw_value as u16) * 10;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    format!("{hours:02}:{minutes:02}")
}

fn format_temperature(half_degrees: u8) -> String {
    let whole = 16 + (half_degrees / 2);
    if half_degrees % 2 == 0 {
        whole.to_string()
    } else {
        format!("{whole}.5")
    }
}

#[allow(dead_code)]
fn _path_exists(path: &Path) -> bool {
    path.exists()
}

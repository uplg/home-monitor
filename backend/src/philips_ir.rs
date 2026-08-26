//! Philips RC5 infrared frames, encoded as Broadlink packets.
//!
//! Infrared is the only channel that reaches this set in every state. In deep
//! standby it drops its network interface — magic packets go unanswered even
//! when sent raw at layer 2 — and it leaves the CEC bus, so One Touch Play
//! times out with nothing on the wire. Its infrared receiver stays powered
//! throughout, whatever the standby depth, and keeps working when the
//! JointSPACE server has crashed.
//!
//! The codes are synthesised rather than learnt. Learning needs the original
//! remote pointed at the blaster, which means waking the set first — and that
//! destroys the very state the code exists to escape.

const BROADLINK_TICK_US: f32 = 32.84;
const BROADLINK_IR_TOKEN: u8 = 0x26;

/// RC5 spends 1778µs on a bit, split into two equal halves.
const RC5_HALF_BIT_US: u32 = 889;
const RC5_ADDRESS_BITS: u8 = 5;
const RC5_COMMAND_BITS: u8 = 6;

/// Philips addresses the television at 0.
pub const TV_ADDRESS: u8 = 0x00;
/// Power is *discrete* on this set rather than a toggle, so an "on" code can
/// never switch a running television off — which is what makes it safe to fire
/// blind, with no way to read the current state back.
pub const TV_POWER_ON: u8 = 0x3F;
pub const TV_POWER_OFF: u8 = 0x3D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Mark,
    Space,
}

/// Builds the Broadlink packet for one RC5 frame.
///
/// `toggle` has to flip between consecutive presses: a receiver reading the
/// same toggle bit twice takes the second frame for a held key and ignores it.
pub fn encode_rc5(address: u8, command: u8, toggle: bool) -> Vec<u8> {
    let mut bits = Vec::with_capacity(14);
    bits.push(true);
    // Commands above 63 borrow this second start bit as a seventh command bit
    // (RC5X); the Philips power codes stay inside six, so it stays set.
    bits.push(true);
    bits.push(toggle);
    for shift in (0..RC5_ADDRESS_BITS).rev() {
        bits.push((address >> shift) & 1 == 1);
    }
    for shift in (0..RC5_COMMAND_BITS).rev() {
        bits.push((command >> shift) & 1 == 1);
    }

    // Bi-phase coding: a one is a space then a mark, a zero the reverse.
    let mut halves = Vec::with_capacity(bits.len() * 2);
    for bit in bits {
        halves.extend_from_slice(if bit {
            &[Level::Space, Level::Mark]
        } else {
            &[Level::Mark, Level::Space]
        });
    }

    // Neighbouring halves at the same level are one longer pulse on the wire.
    let mut durations: Vec<(Level, u32)> = Vec::with_capacity(halves.len());
    for level in halves {
        match durations.last_mut() {
            Some((last, micros)) if *last == level => *micros += RC5_HALF_BIT_US,
            _ => durations.push((level, RC5_HALF_BIT_US)),
        }
    }

    // A Broadlink frame alternates starting from a mark, so the space that
    // RC5's first start bit opens with cannot be expressed — and needs no
    // expressing, an idle receiver being unable to tell it from the silence
    // before the frame. Leaving it in would invert every pulse that follows.
    if matches!(durations.first(), Some((Level::Space, _))) {
        durations.remove(0);
    }

    build_packet(&durations)
}

fn build_packet(durations: &[(Level, u32)]) -> Vec<u8> {
    let mut packet = vec![BROADLINK_IR_TOKEN, 0x00, 0x00, 0x00];

    for (_, micros) in durations {
        let ticks = ((*micros as f32) / BROADLINK_TICK_US).round() as u16;
        if ticks >= 256 {
            packet.push(0x00);
            packet.push((ticks >> 8) as u8);
            packet.push((ticks & 0xFF) as u8);
        } else {
            packet.push(ticks as u8);
        }
    }

    packet.extend_from_slice(&[0x00, 0x0D]);
    let encoded_len = (packet.len() - 4 + 1) as u16;
    packet[2] = (encoded_len & 0xFF) as u8;
    packet[3] = (encoded_len >> 8) as u8;
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    /// The exact bytes that were fired at the set and brought it out of deep
    /// standby: the network answered within five seconds and the box reported
    /// `power_status: 0` for the television on the CEC bus. Pinned so a change
    /// to the encoder cannot silently stop reaching the hardware.
    #[test]
    fn power_on_matches_the_packet_proven_against_the_set() {
        let packet = encode_rc5(TV_ADDRESS, TV_POWER_ON, false);
        assert_eq!(
            STANDARD.encode(&packet),
            "JgAcABsbNhsbGxsbGxsbGxs2GxsbGxsbGxsbGxsADQ=="
        );
    }

    #[test]
    fn power_off_matches_the_generated_packet() {
        let packet = encode_rc5(TV_ADDRESS, TV_POWER_OFF, false);
        assert_eq!(
            STANDARD.encode(&packet),
            "JgAaABsbNhsbGxsbGxsbGxs2GxsbGxsbNjYbAA0="
        );
    }

    /// Half-bits are 27 ticks and merged pairs 54; a frame that opened on a
    /// space would have every pulse inverted, so the first duration matters.
    #[test]
    fn frame_opens_on_a_mark_of_one_half_bit() {
        let packet = encode_rc5(TV_ADDRESS, TV_POWER_ON, false);
        assert_eq!(packet[4], 27);
        assert_eq!(packet[0], BROADLINK_IR_TOKEN);
        assert_eq!(&packet[packet.len() - 2..], &[0x00, 0x0D]);
    }

    #[test]
    fn toggle_changes_the_frame() {
        assert_ne!(
            encode_rc5(TV_ADDRESS, TV_POWER_ON, false),
            encode_rc5(TV_ADDRESS, TV_POWER_ON, true)
        );
    }

    /// The declared length has to cover the durations and the terminator, or
    /// the blaster truncates the frame.
    #[test]
    fn declared_length_covers_the_payload() {
        let packet = encode_rc5(TV_ADDRESS, TV_POWER_ON, false);
        let declared = u16::from(packet[2]) | (u16::from(packet[3]) << 8);
        assert_eq!(usize::from(declared), packet.len() - 4 + 1);
    }
}

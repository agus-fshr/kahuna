//! Asynchronous serial (UART) decoder.
//!
//! Decodes one line. A full-duplex link is two decoders, which is also how
//! logic analysers model it: TX and RX have independent framing and can even
//! run at different rates.

use serde::{Deserialize, Serialize};

use super::{Bit, BitLane, BitOrder, DecodedLane, DecodedWord, Role, WordFormat, WordKind};

/// Role indices. Positional in saved state, so only ever append.
pub const LINE: usize = 0;

pub const ROLES: &[Role] = &[Role {
    name: "Line",
    required: true,
    aliases: &["uart", "tx", "rx", "txd", "rxd", "serial", "sout", "sin"],
}];

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display, Default,
)]
pub enum Parity {
    #[default]
    #[display("None")]
    None,
    #[display("Even")]
    Even,
    #[display("Odd")]
    Odd,
}

impl Parity {
    pub const ALL: [Parity; 3] = [Parity::None, Parity::Even, Parity::Odd];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UartSettings {
    /// Length of one bit in timescale ticks. `None` measures it from the
    /// narrowest pulse on the line.
    pub bit_period: Option<u64>,
    /// Bits per frame, excluding start, parity and stop. 5..=9.
    pub data_bits: u32,
    pub parity: Parity,
    /// Stop bits to require before the next frame. 1 or 2.
    pub stop_bits: u32,
    pub bit_order: BitOrder,
    /// Level the line rests at between frames. Idle high is conventional;
    /// idle low covers an inverted line without needing a separate setting.
    pub idle_high: bool,
    pub format: WordFormat,
}

impl Default for UartSettings {
    fn default() -> Self {
        // 8N1, LSB first, idle high, rate measured from the trace.
        Self {
            bit_period: None,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            bit_order: BitOrder::LsbFirst,
            idle_high: true,
            format: WordFormat::Ascii,
        }
    }
}

impl UartSettings {
    #[must_use]
    pub fn lane_names(&self) -> Vec<String> {
        vec!["Data".to_string()]
    }

    const fn idle(&self) -> Bit {
        if self.idle_high { Bit::One } else { Bit::Zero }
    }

    const fn start(&self) -> Bit {
        if self.idle_high { Bit::Zero } else { Bit::One }
    }

    /// Bit period to decode with: the configured one, else the narrowest pulse
    /// on the line, which is one bit whenever the data contains a bit that
    /// differs from both its neighbours.
    #[must_use]
    pub fn resolve_bit_period(&self, line: &BitLane) -> Option<u64> {
        if let Some(p) = self.bit_period {
            return (p > 0).then_some(p);
        }
        line.changes
            .windows(2)
            .map(|w| w[1].0.saturating_sub(w[0].0))
            .filter(|d| *d > 0)
            .min()
    }

    #[must_use]
    pub fn decode(&self, lanes: &[Option<BitLane>]) -> Vec<DecodedLane> {
        let name = "Data".to_string();
        let Some(Some(line)) = lanes.get(LINE) else {
            return vec![DecodedLane {
                name,
                words: vec![],
            }];
        };
        let Some(period) = self.resolve_bit_period(line) else {
            return vec![DecodedLane {
                name,
                words: vec![],
            }];
        };

        let data_bits = self.data_bits.clamp(1, 9);
        let parity_bits = u32::from(self.parity != Parity::None);
        let stop_bits = self.stop_bits.clamp(1, 2);
        let frame_bits = 1 + data_bits + parity_bits + stop_bits;

        let mut words = vec![];
        let mut next_free = 0u64;

        for start in line.edges_to(self.start()) {
            // Skip edges that fall inside a frame already decoded: they are
            // data transitions, not the start of a new frame.
            if start < next_free {
                continue;
            }

            // Sample at the centre of each bit.
            let centre = |index: u32| start + u64::from(index) * period + period / 2;

            if line.value_at(centre(0)) != Some(self.start()) {
                // A glitch narrower than half a bit, not a start bit.
                continue;
            }

            let mut value = 0u64;
            let mut ones = 0u32;
            let mut invalid = false;
            for i in 0..data_bits {
                match line.value_at(centre(i + 1)) {
                    Some(b @ (Bit::Zero | Bit::One)) => {
                        // The idle level is a logical 1 (mark), so on an
                        // inverted line the data bits invert with it.
                        let is_one = b == self.idle();
                        let one = u64::from(is_one);
                        ones += u32::from(is_one);
                        match self.bit_order {
                            BitOrder::LsbFirst => value |= one << i,
                            BitOrder::MsbFirst => value = (value << 1) | one,
                        }
                    }
                    _ => invalid = true,
                }
            }

            let mut error = invalid.then(|| "x".to_string());

            if self.parity != Parity::None {
                let bit = line.value_at(centre(data_bits + 1));
                let got = bit == Some(Bit::One);
                let want_even = self.parity == Parity::Even;
                // Even parity makes the count of ones including the parity bit
                // even, so the bit is set when the data has an odd count.
                let expected = (ones % 2 == 1) == want_even;
                if got != expected {
                    error.get_or_insert_with(|| "parity".to_string());
                }
            }

            // Every stop bit must be at the idle level. A frame that fails this
            // is usually a wrong bit period or bit count, so it is worth
            // reporting rather than silently emitting a plausible byte.
            for s in 0..stop_bits {
                if line.value_at(centre(data_bits + parity_bits + 1 + s)) != Some(self.idle()) {
                    error.get_or_insert_with(|| "framing".to_string());
                }
            }

            let end = start + u64::from(frame_bits) * period;
            words.push(DecodedWord {
                start,
                end,
                text: error
                    .clone()
                    .unwrap_or_else(|| self.format.format(value, data_bits)),
                kind: if error.is_some() {
                    WordKind::Error
                } else {
                    WordKind::Data
                },
            });
            next_free = end;
        }

        vec![DecodedLane { name, words }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: u64 = 100;

    /// Build an idle-high 8N1 line carrying `bytes`, LSB first, with `gap`
    /// bit-times of idle between frames.
    fn frames(bytes: &[u8], gap: u64) -> BitLane {
        let mut changes = vec![(0u64, Bit::One)];
        let mut t = PERIOD;
        let push = |changes: &mut Vec<(u64, Bit)>, time: u64, bit: Bit| {
            if changes.last().map(|c| c.1) != Some(bit) {
                changes.push((time, bit));
            }
        };
        for &byte in bytes {
            push(&mut changes, t, Bit::Zero); // start
            t += PERIOD;
            for i in 0..8 {
                let bit = if (byte >> i) & 1 == 1 {
                    Bit::One
                } else {
                    Bit::Zero
                };
                push(&mut changes, t, bit);
                t += PERIOD;
            }
            push(&mut changes, t, Bit::One); // stop
            t += PERIOD * (1 + gap);
        }
        BitLane::new(changes)
    }

    fn settings() -> UartSettings {
        UartSettings {
            bit_period: Some(PERIOD),
            format: WordFormat::Hex,
            ..Default::default()
        }
    }

    fn texts(lanes: &[DecodedLane]) -> Vec<String> {
        lanes[0].words.iter().map(|w| w.text.clone()).collect()
    }

    #[test]
    fn decodes_8n1() {
        let lanes = vec![Some(frames(&[0x4B, 0x61, 0x68], 2))];
        assert_eq!(texts(&settings().decode(&lanes)), vec!["4B", "61", "68"]);
    }

    #[test]
    fn ascii_format_renders_characters() {
        let lanes = vec![Some(frames(b"Kahuna", 2))];
        let s = UartSettings {
            bit_period: Some(PERIOD),
            ..Default::default()
        };
        assert_eq!(
            texts(&s.decode(&lanes)),
            vec!["'K'", "'a'", "'h'", "'u'", "'n'", "'a'"]
        );
    }

    #[test]
    fn bit_period_is_measured_when_unset() {
        // 0x55 alternates every bit, so the narrowest pulse is exactly one bit.
        let lanes = vec![Some(frames(&[0x55], 2))];
        let s = UartSettings {
            bit_period: None,
            format: WordFormat::Hex,
            ..Default::default()
        };
        assert_eq!(
            s.resolve_bit_period(lanes[0].as_ref().unwrap()),
            Some(PERIOD)
        );
        assert_eq!(texts(&s.decode(&lanes)), vec!["55"]);
    }

    #[test]
    fn data_bit_transitions_are_not_mistaken_for_start_bits() {
        // 0x00 drives the line low for eight bits straight after the start bit;
        // 0xFE has a single low data bit. Both tempt a naive scan into
        // resynchronising mid-frame.
        let lanes = vec![Some(frames(&[0x00, 0xFE], 2))];
        assert_eq!(texts(&settings().decode(&lanes)), vec!["00", "FE"]);
    }

    #[test]
    fn back_to_back_frames_decode() {
        let lanes = vec![Some(frames(&[0x41, 0x42, 0x43], 0))];
        assert_eq!(texts(&settings().decode(&lanes)), vec!["41", "42", "43"]);
    }

    #[test]
    fn a_bad_stop_bit_is_reported_as_framing() {
        // Hold the line low where the stop bit belongs. Emitting a plausible
        // byte here would hide a wrong bit rate or bit count.
        // Drop the transition that raises the line for the stop bit, leaving
        // it at bit 7's level (low for 0x4B) where the stop bit is sampled.
        let mut line = frames(&[0x4B], 4);
        let stop_at = PERIOD * 10;
        line.changes.retain(|(t, _)| *t != stop_at);
        let out = settings().decode(&[Some(line)]);
        assert_eq!(out[0].words[0].kind, WordKind::Error);
        assert_eq!(out[0].words[0].text, "framing");
    }

    #[test]
    fn parity_is_checked() {
        // frames() emits no parity bit, so the stop bit lands where parity is
        // expected and at least one frame must fail.
        let lanes = vec![Some(frames(&[0x00], 2))];
        let s = UartSettings {
            parity: Parity::Even,
            ..settings()
        };
        assert!(
            s.decode(&lanes)[0]
                .words
                .iter()
                .any(|w| w.kind == WordKind::Error)
        );
    }

    #[test]
    fn seven_bit_words_decode() {
        // The generator always emits eight data bits, so at data_bits = 7 the
        // eighth is read as the stop bit. 0xC1 has it set, which is what a stop
        // bit looks like, and its low seven bits are 0x41.
        let lanes = vec![Some(frames(&[0xC1], 2))];
        let s = UartSettings {
            data_bits: 7,
            ..settings()
        };
        assert_eq!(texts(&s.decode(&lanes)), vec!["41"]);
    }

    #[test]
    fn idle_low_inverts_the_line() {
        let normal = frames(&[0x4B], 2);
        let inverted = BitLane::new(
            normal
                .changes
                .iter()
                .map(|&(t, b)| {
                    (
                        t,
                        match b {
                            Bit::One => Bit::Zero,
                            Bit::Zero => Bit::One,
                            Bit::Invalid => Bit::Invalid,
                        },
                    )
                })
                .collect(),
        );
        let s = UartSettings {
            idle_high: false,
            ..settings()
        };
        assert_eq!(texts(&s.decode(&[Some(inverted)])), vec!["4B"]);
    }

    #[test]
    fn missing_line_yields_an_empty_row() {
        let out = settings().decode(&[None]);
        assert_eq!(out.len(), 1);
        assert!(out[0].words.is_empty());
    }
}

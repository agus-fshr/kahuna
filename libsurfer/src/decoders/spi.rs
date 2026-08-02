//! SPI protocol decoder.
//!
//! Samples MOSI and MISO on the clock edge selected by CPOL/CPHA, gated by chip
//! select, and groups the sampled bits into words.

use serde::{Deserialize, Serialize};

use super::{Bit, BitLane, BitOrder, DecodedLane, DecodedWord, Role, WordFormat, WordKind};

/// Role indices. These are positional in the saved state, so only ever append.
pub const SCLK: usize = 0;
pub const MOSI: usize = 1;
pub const MISO: usize = 2;
pub const CS: usize = 3;

pub const ROLES: &[Role] = &[
    Role {
        name: "SCLK",
        required: true,
        aliases: &["sclk", "sck", "scl", "clk"],
    },
    Role {
        name: "MOSI",
        required: false,
        aliases: &["mosi", "copi", "sdo", "din", "tx"],
    },
    Role {
        name: "MISO",
        required: false,
        aliases: &["miso", "cipo", "sdi", "dout", "rx"],
    },
    Role {
        name: "CS",
        required: false,
        aliases: &["cs_n", "csn", "ss_n", "ssn", "nss", "cs", "ss"],
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpiSettings {
    /// Clock polarity: the idle level of SCLK.
    pub cpol: bool,
    /// Clock phase: `false` samples on the leading edge, `true` on the trailing edge.
    pub cpha: bool,
    pub bit_order: BitOrder,
    /// Bits per word. 1..=64.
    pub word_size: u32,
    pub format: WordFormat,
    /// Whether chip select is active low.
    pub cs_active_low: bool,
}

impl Default for SpiSettings {
    fn default() -> Self {
        // Mode 0, 8-bit, MSB first, active-low CS: by far the most common.
        Self {
            cpol: false,
            cpha: false,
            bit_order: BitOrder::MsbFirst,
            word_size: 8,
            format: WordFormat::Hex,
            cs_active_low: true,
        }
    }
}

impl SpiSettings {
    /// SPI mode number (0-3), the conventional way to name a CPOL/CPHA pair.
    #[must_use]
    pub const fn mode(&self) -> u8 {
        (self.cpol as u8) << 1 | (self.cpha as u8)
    }

    pub const fn set_mode(&mut self, mode: u8) {
        self.cpol = mode & 0b10 != 0;
        self.cpha = mode & 0b01 != 0;
    }

    #[must_use]
    pub fn lane_names(&self) -> Vec<String> {
        vec!["MOSI".to_string(), "MISO".to_string()]
    }

    /// The SCLK level whose arrival latches a bit.
    ///
    /// The leading edge of a clock pulse goes away from the idle level, so with
    /// CPOL=0 it rises and with CPOL=1 it falls. CPHA=0 samples that leading
    /// edge; CPHA=1 samples the trailing one.
    const fn sampling_level(&self) -> Bit {
        let leading_is_rise = !self.cpol;
        let sample_on_leading = !self.cpha;
        if leading_is_rise == sample_on_leading {
            Bit::One
        } else {
            Bit::Zero
        }
    }

    /// Decode `lanes`, indexed by the `SCLK`/`MOSI`/`MISO`/`CS` constants.
    #[must_use]
    pub fn decode(&self, lanes: &[Option<BitLane>]) -> Vec<DecodedLane> {
        let names = self.lane_names();
        let Some(Some(sclk)) = lanes.get(SCLK) else {
            // Without a clock there is nothing to sample on.
            return names
                .into_iter()
                .map(|name| DecodedLane {
                    name,
                    words: vec![],
                })
                .collect();
        };

        let cs = lanes.get(CS).and_then(Option::as_ref);
        let word_size = self.word_size.clamp(1, 64);
        let sample_times = sclk.edges_to(self.sampling_level());

        let mut out = vec![];
        for (data_idx, name) in [MOSI, MISO].into_iter().zip(names) {
            let words = match lanes.get(data_idx).and_then(Option::as_ref) {
                Some(data) => self.decode_one(data, cs, &sample_times, word_size),
                None => vec![],
            };
            out.push(DecodedLane { name, words });
        }
        out
    }

    fn decode_one(
        &self,
        data: &BitLane,
        cs: Option<&BitLane>,
        sample_times: &[u64],
        word_size: u32,
    ) -> Vec<DecodedWord> {
        let mut words = vec![];

        // Bits accumulated so far for the word in progress.
        let mut acc: u64 = 0;
        let mut count: u32 = 0;
        let mut word_start: u64 = 0;
        let mut invalid = false;
        // Tracks CS so that a deassert mid-word can be reported as an error
        // rather than silently merged into the next transfer.
        let mut was_selected = false;

        for &t in sample_times {
            let selected = cs.is_none_or(|cs| self.is_selected(cs, t));

            if was_selected && !selected && count > 0 {
                words.push(DecodedWord {
                    start: word_start,
                    end: t,
                    text: format!("{count} of {word_size} bits"),
                    kind: WordKind::Error,
                });
                acc = 0;
                count = 0;
                invalid = false;
            }
            was_selected = selected;

            if !selected {
                continue;
            }

            let bit = data.value_at(t);
            if count == 0 {
                word_start = t;
            }
            match bit {
                Some(Bit::Zero) | Some(Bit::One) => {
                    let b = u64::from(bit == Some(Bit::One));
                    match self.bit_order {
                        BitOrder::MsbFirst => acc = (acc << 1) | b,
                        BitOrder::LsbFirst => acc |= b << count,
                    }
                }
                // An x or z on the data line makes the whole word untrustworthy,
                // but the bit still occupies its slot so the framing survives.
                _ => invalid = true,
            }
            count += 1;

            if count == word_size {
                words.push(DecodedWord {
                    start: word_start,
                    end: t,
                    text: if invalid {
                        "x".to_string()
                    } else {
                        self.format.format(acc, word_size)
                    },
                    kind: if invalid {
                        WordKind::Error
                    } else {
                        WordKind::Data
                    },
                });
                acc = 0;
                count = 0;
                invalid = false;
            }
        }

        // A word left in progress when the trace ends is incomplete, and saying
        // so is more useful than dropping it or showing a half-shifted value.
        if count > 0 {
            words.push(DecodedWord {
                start: word_start,
                end: sample_times.last().copied().unwrap_or(word_start),
                text: format!("{count} of {word_size} bits"),
                kind: WordKind::Error,
            });
        }

        // Each word is drawn from its first sampling edge to its last, which
        // leaves a gap before the next word. Stretch each word to meet the next
        // so the row reads as a contiguous sequence of blocks.
        for i in 0..words.len() {
            let next_start = words.get(i + 1).map(|w| w.start);
            if let Some(next_start) = next_start {
                words[i].end = next_start;
            }
        }
        words
    }

    fn is_selected(&self, cs: &BitLane, t: u64) -> bool {
        match cs.value_at(t) {
            Some(Bit::One) => !self.cs_active_low,
            Some(Bit::Zero) => self.cs_active_low,
            // Before CS has any value, or while it is x/z, assume not selected
            // rather than decoding noise into confident-looking words.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the SCLK, MOSI, MISO and CS lanes for a mode-0 transfer of `bytes`,
    /// as `(mosi, miso)` pairs, with a half period of 10 time units.
    fn mode0_transfer(bytes: &[(u8, u8)]) -> Vec<Option<BitLane>> {
        let half = 10u64;
        let mut sclk = vec![(0u64, Bit::Zero)];
        let mut mosi = vec![(0u64, Bit::Zero)];
        let mut miso = vec![(0u64, Bit::Zero)];
        let cs = vec![(0u64, Bit::One), (half, Bit::Zero)];

        let mut t = half;
        for &(tx, rx) in bytes {
            for bit in (0..8).rev() {
                let b = |v: u8| {
                    if (v >> bit) & 1 == 1 {
                        Bit::One
                    } else {
                        Bit::Zero
                    }
                };
                // Drive while low, then rise (sample) and fall again.
                mosi.push((t, b(tx)));
                miso.push((t, b(rx)));
                sclk.push((t + half, Bit::One));
                sclk.push((t + 2 * half, Bit::Zero));
                t += 2 * half;
            }
        }

        vec![
            Some(BitLane::new(sclk)),
            Some(BitLane::new(mosi)),
            Some(BitLane::new(miso)),
            Some(BitLane::new(cs)),
        ]
    }

    fn texts(lane: &DecodedLane) -> Vec<String> {
        lane.words.iter().map(|w| w.text.clone()).collect()
    }

    #[test]
    fn decodes_a_mode0_transfer_on_both_directions() {
        let lanes = mode0_transfer(&[(0x9F, 0x00), (0x00, 0xEF), (0x00, 0x40)]);
        let out = SpiSettings::default().decode(&lanes);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "MOSI");
        assert_eq!(texts(&out[0]), vec!["9F", "00", "00"]);
        assert_eq!(texts(&out[1]), vec!["00", "EF", "40"]);
        assert!(
            out.iter()
                .all(|l| l.words.iter().all(|w| w.kind == WordKind::Data))
        );
    }

    #[test]
    fn words_are_contiguous_and_ordered() {
        let lanes = mode0_transfer(&[(0x12, 0), (0x34, 0)]);
        let out = SpiSettings::default().decode(&lanes);
        let w = &out[0].words;
        assert_eq!(w.len(), 2);
        assert!(w[0].start < w[0].end);
        assert_eq!(w[0].end, w[1].start, "no gap between consecutive words");
    }

    #[test]
    fn lsb_first_reverses_each_word() {
        let lanes = mode0_transfer(&[(0b1000_0000, 0)]);
        let settings = SpiSettings {
            bit_order: BitOrder::LsbFirst,
            ..Default::default()
        };
        assert_eq!(texts(&settings.decode(&lanes)[0]), vec!["01"]);
    }

    #[test]
    fn word_size_regroups_the_same_bits() {
        let lanes = mode0_transfer(&[(0xAB, 0), (0xCD, 0)]);
        let settings = SpiSettings {
            word_size: 4,
            ..Default::default()
        };
        assert_eq!(texts(&settings.decode(&lanes)[0]), vec!["A", "B", "C", "D"]);
    }

    /// Mode-1 timing: data is driven on the leading (rising) edge and sampled on
    /// the trailing (falling) one.
    fn mode1_transfer(bytes: &[u8]) -> Vec<Option<BitLane>> {
        let half = 10u64;
        let mut sclk = vec![(0u64, Bit::Zero)];
        let mut mosi = vec![(0u64, Bit::Zero)];
        let cs = vec![(0u64, Bit::One), (half, Bit::Zero)];

        let mut t = half;
        for &tx in bytes {
            for bit in (0..8).rev() {
                let b = if (tx >> bit) & 1 == 1 {
                    Bit::One
                } else {
                    Bit::Zero
                };
                sclk.push((t, Bit::One));
                // Driven just after the leading edge, as clock-to-output delay
                // does in hardware. Driving it exactly on the edge would be
                // ambiguous: a decoder sampling that timestamp cannot tell
                // whether it should see the old bit or the new one.
                mosi.push((t + 1, b));
                sclk.push((t + half, Bit::Zero));
                t += 2 * half;
            }
        }

        vec![
            Some(BitLane::new(sclk)),
            Some(BitLane::new(mosi)),
            None,
            Some(BitLane::new(cs)),
        ]
    }

    #[test]
    fn cpha_selects_the_other_edge() {
        let mode1 = SpiSettings {
            cpha: true,
            ..Default::default()
        };
        assert_eq!(mode1.mode(), 1);

        // Mode-1 timing decodes correctly only when sampling the trailing edge.
        let lanes = mode1_transfer(&[0x9F]);
        assert_eq!(texts(&mode1.decode(&lanes)[0]), vec!["9F"]);

        // And reading the same trace with mode 0 samples the leading edge, where
        // the bit has only just been driven, so it must not agree.
        let mode0 = SpiSettings::default();
        assert_ne!(
            texts(&mode0.decode(&lanes)[0]),
            vec!["9F"],
            "mode 0 must not decode mode-1 timing correctly"
        );
    }

    #[test]
    fn mode_round_trips() {
        for m in 0..4u8 {
            let mut s = SpiSettings::default();
            s.set_mode(m);
            assert_eq!(s.mode(), m);
        }
    }

    #[test]
    fn chip_select_gates_decoding() {
        // Same clocking, but CS never asserts: nothing should decode.
        let mut lanes = mode0_transfer(&[(0x9F, 0)]);
        lanes[CS] = Some(BitLane::new(vec![(0, Bit::One)]));
        assert!(SpiSettings::default().decode(&lanes)[0].words.is_empty());
    }

    #[test]
    fn a_word_cut_short_by_chip_select_is_an_error() {
        let mut lanes = mode0_transfer(&[(0x9F, 0)]);
        // Deassert CS partway through the byte.
        lanes[CS] = Some(BitLane::new(vec![
            (0, Bit::One),
            (10, Bit::Zero),
            (55, Bit::One),
        ]));
        let out = SpiSettings::default().decode(&lanes);
        assert_eq!(out[0].words.len(), 1);
        assert_eq!(out[0].words[0].kind, WordKind::Error);
        assert!(out[0].words[0].text.contains("of 8 bits"));
    }

    #[test]
    fn invalid_data_bits_poison_only_their_word() {
        let mut lanes = mode0_transfer(&[(0xFF, 0), (0xAB, 0)]);
        // Corrupt the bit that the first sampling edge (t=20) will read. The
        // insert has to keep `changes` sorted, since lookups binary search it.
        if let Some(mosi) = &mut lanes[MOSI] {
            let at = mosi.changes.partition_point(|(t, _)| *t <= 15);
            mosi.changes.insert(at, (15, Bit::Invalid));
        }
        let out = SpiSettings::default().decode(&lanes);
        assert_eq!(out[0].words[0].kind, WordKind::Error);
        assert_eq!(
            out[0].words[1].kind,
            WordKind::Data,
            "later words still decode"
        );
        assert_eq!(out[0].words[1].text, "AB");
    }

    #[test]
    fn missing_data_lane_yields_an_empty_row() {
        let mut lanes = mode0_transfer(&[(0x9F, 0)]);
        lanes[MISO] = None;
        let out = SpiSettings::default().decode(&lanes);
        assert_eq!(texts(&out[0]), vec!["9F"]);
        assert!(out[1].words.is_empty(), "MISO row exists but is empty");
    }

    #[test]
    fn missing_clock_yields_no_words() {
        let mut lanes = mode0_transfer(&[(0x9F, 0)]);
        lanes[SCLK] = None;
        let out = SpiSettings::default().decode(&lanes);
        assert_eq!(out.len(), 2, "rows still exist so the item can be drawn");
        assert!(out.iter().all(|l| l.words.is_empty()));
    }

    #[test]
    fn decoding_without_chip_select_uses_every_clock_edge() {
        let mut lanes = mode0_transfer(&[(0x9F, 0)]);
        lanes[CS] = None;
        assert_eq!(texts(&SpiSettings::default().decode(&lanes)[0]), vec!["9F"]);
    }
}

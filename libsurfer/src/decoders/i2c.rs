//! I2C protocol decoder.
//!
//! Unlike SPI and UART, the output is a token stream rather than a flat run of
//! words: a transaction reads `S`, `50 W`, `A`, `00`, `A`, `2A`, `A`, `P`. The
//! framing is the useful part of I2C, so it is shown rather than hidden.

use serde::{Deserialize, Serialize};

use super::{Bit, BitLane, DecodedLane, DecodedWord, Role, WordFormat, WordKind};

/// Role indices. Positional in saved state, so only ever append.
pub const SCL: usize = 0;
pub const SDA: usize = 1;

pub const ROLES: &[Role] = &[
    Role {
        name: "SCL",
        required: true,
        aliases: &["scl", "i2c_clk", "sclk", "clk"],
    },
    Role {
        name: "SDA",
        required: true,
        aliases: &["sda", "i2c_data", "data"],
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct I2cSettings {
    /// Show the 7-bit address with its read/write bit split out, rather than
    /// showing the raw 8-bit frame that went over the wire.
    pub split_address: bool,
    pub format: WordFormat,
}

impl Default for I2cSettings {
    fn default() -> Self {
        Self {
            split_address: true,
            format: WordFormat::Hex,
        }
    }
}

/// What happened at a given time on the bus.
enum Event {
    Start,
    Stop,
    /// A rising SCL edge, which latches whatever SDA holds.
    Sample(Bit),
}

impl I2cSettings {
    #[must_use]
    pub fn lane_names(&self) -> Vec<String> {
        vec!["Bus".to_string()]
    }

    #[must_use]
    pub fn decode(&self, lanes: &[Option<BitLane>]) -> Vec<DecodedLane> {
        let name = "Bus".to_string();
        let (Some(Some(scl)), Some(Some(sda))) = (lanes.get(SCL), lanes.get(SDA)) else {
            return vec![DecodedLane {
                name,
                words: vec![],
            }];
        };

        let mut events = self.collect_events(scl, sda);
        events.sort_by_key(|(t, _)| *t);

        // Used to bound how wide a token is drawn, so a STOP does not stretch
        // across the idle gap until the next transaction.
        let clock_period = scl
            .edges_to(Bit::One)
            .windows(2)
            .map(|w| w[1] - w[0])
            .min()
            .unwrap_or(1);

        let mut words: Vec<DecodedWord> = vec![];
        let mut in_transaction = false;
        let mut bits: u32 = 0;
        let mut acc: u64 = 0;
        let mut byte_start = 0u64;
        // The first byte after a START is the address; the rest are data.
        let mut expect_address = false;

        let push = |words: &mut Vec<DecodedWord>, start: u64, text: String, kind: WordKind| {
            words.push(DecodedWord {
                start,
                end: start + clock_period,
                text,
                kind,
            });
        };

        for (time, event) in events {
            match event {
                Event::Start => {
                    // A START inside a transaction is a repeated start, which
                    // is legal and keeps the bus held.
                    push(
                        &mut words,
                        time,
                        if in_transaction { "Sr" } else { "S" }.to_string(),
                        WordKind::Data,
                    );
                    in_transaction = true;
                    expect_address = true;
                    bits = 0;
                    acc = 0;
                }
                Event::Stop => {
                    if bits > 0 {
                        push(
                            &mut words,
                            byte_start,
                            format!("{bits} of 8 bits"),
                            WordKind::Error,
                        );
                    }
                    push(&mut words, time, "P".to_string(), WordKind::Data);
                    in_transaction = false;
                    bits = 0;
                    acc = 0;
                }
                Event::Sample(bit) => {
                    if !in_transaction {
                        continue;
                    }
                    if bits == 8 {
                        // The ninth clock is the acknowledge from the receiver.
                        let acked = bit == Bit::Zero;
                        push(
                            &mut words,
                            time,
                            if acked { "A" } else { "N" }.to_string(),
                            if acked {
                                WordKind::Data
                            } else {
                                WordKind::Error
                            },
                        );
                        bits = 0;
                        acc = 0;
                        expect_address = false;
                        continue;
                    }

                    if bits == 0 {
                        byte_start = time;
                    }
                    acc = (acc << 1) | u64::from(bit == Bit::One);
                    if bit == Bit::Invalid {
                        // Keep the framing but remember the byte is unreliable.
                        acc |= 1 << 63;
                    }
                    bits += 1;

                    if bits == 8 {
                        let unreliable = acc & (1 << 63) != 0;
                        let byte = acc & 0xFF;
                        let text = if unreliable {
                            "x".to_string()
                        } else if expect_address && self.split_address {
                            format!(
                                "{} {}",
                                self.format.format(byte >> 1, 7),
                                if byte & 1 == 0 { "W" } else { "R" }
                            )
                        } else {
                            self.format.format(byte, 8)
                        };
                        words.push(DecodedWord {
                            start: byte_start,
                            end: time,
                            text,
                            kind: if unreliable {
                                WordKind::Error
                            } else {
                                WordKind::Data
                            },
                        });
                    }
                }
            }
        }

        if bits > 0 {
            push(
                &mut words,
                byte_start,
                format!("{bits} of 8 bits"),
                WordKind::Error,
            );
        }

        // Close each token at the next one when they are adjacent, so a byte
        // runs up to its acknowledge rather than leaving a slot-shaped hole,
        // but leave real idle gaps between transactions alone.
        for i in 0..words.len() {
            if let Some(next_start) = words.get(i + 1).map(|w| w.start)
                && next_start > words[i].start
                && next_start - words[i].start <= clock_period * 2
            {
                words[i].end = next_start;
            }
        }

        vec![DecodedLane { name, words }]
    }

    /// Find the START/STOP conditions and the sampling edges.
    fn collect_events(&self, scl: &BitLane, sda: &BitLane) -> Vec<(u64, Event)> {
        let mut events: Vec<(u64, Event)> = vec![];

        // START and STOP are SDA moving while SCL is high, which is the one
        // time SDA is otherwise required to be stable.
        let mut prev: Option<Bit> = None;
        for &(t, v) in &sda.changes {
            if let Some(p) = prev
                && p != v
                && scl.value_at(t) == Some(Bit::One)
            {
                match v {
                    Bit::Zero => events.push((t, Event::Start)),
                    Bit::One => events.push((t, Event::Stop)),
                    Bit::Invalid => {}
                }
            }
            prev = Some(v);
        }

        // A rising SCL edge latches a data bit only if SDA stays put for the
        // whole high phase. If SDA moves while SCL is high, that pulse carries
        // a START or STOP, and counting it as a bit leaves a spurious partial
        // byte in front of every STOP.
        let falls = scl.edges_to(Bit::Zero);
        for t in scl.edges_to(Bit::One) {
            let next_fall = falls
                .get(falls.partition_point(|f| *f <= t))
                .copied()
                .unwrap_or(u64::MAX);
            let first_after = sda.changes.partition_point(|(st, _)| *st <= t);
            let sda_moves = sda
                .changes
                .get(first_after)
                .is_some_and(|(st, _)| *st < next_fall);
            if sda_moves {
                continue;
            }
            events.push((t, Event::Sample(sda.value_at(t).unwrap_or(Bit::Invalid))));
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF: u64 = 10;

    /// Build SCL and SDA for a write transaction: START, address+W, ACK, then
    /// each data byte followed by an ACK, then STOP.
    fn transaction(addr: u8, read: bool, data: &[u8]) -> Vec<Option<BitLane>> {
        transaction_acked(addr, read, data, true)
    }

    /// `acked` drives the level the receiver puts on SDA during every
    /// acknowledge slot: low acknowledges, high is a NACK.
    fn transaction_acked(addr: u8, read: bool, data: &[u8], acked: bool) -> Vec<Option<BitLane>> {
        let ack_bit = if acked { Bit::Zero } else { Bit::One };
        let mut scl = vec![(0u64, Bit::One)];
        let mut sda = vec![(0u64, Bit::One)];
        let mut t = HALF * 2;

        let set = |v: &mut Vec<(u64, Bit)>, time: u64, bit: Bit| {
            if v.last().map(|c| c.1) != Some(bit) {
                v.push((time, bit));
            }
        };

        // START: SDA falls while SCL is high, then SCL goes low.
        set(&mut sda, t, Bit::Zero);
        set(&mut scl, t + HALF / 2, Bit::Zero);
        t += HALF;

        let clock_bit =
            |scl: &mut Vec<(u64, Bit)>, sda: &mut Vec<(u64, Bit)>, t: &mut u64, bit: Bit| {
                set(sda, *t, bit);
                set(scl, *t + HALF / 2, Bit::One);
                set(scl, *t + HALF / 2 + HALF, Bit::Zero);
                *t += HALF * 2;
            };

        let frame = (addr << 1) | u8::from(read);
        for i in (0..8).rev() {
            let bit = if (frame >> i) & 1 == 1 {
                Bit::One
            } else {
                Bit::Zero
            };
            clock_bit(&mut scl, &mut sda, &mut t, bit);
        }
        clock_bit(&mut scl, &mut sda, &mut t, ack_bit);

        for &byte in data {
            for i in (0..8).rev() {
                let bit = if (byte >> i) & 1 == 1 {
                    Bit::One
                } else {
                    Bit::Zero
                };
                clock_bit(&mut scl, &mut sda, &mut t, bit);
            }
            clock_bit(&mut scl, &mut sda, &mut t, ack_bit);
        }

        // STOP: SCL rises with SDA low, then SDA rises.
        set(&mut sda, t, Bit::Zero);
        set(&mut scl, t + HALF / 2, Bit::One);
        set(&mut sda, t + HALF, Bit::One);

        vec![Some(BitLane::new(scl)), Some(BitLane::new(sda))]
    }

    fn texts(lanes: &[DecodedLane]) -> Vec<String> {
        lanes[0].words.iter().map(|w| w.text.clone()).collect()
    }

    #[test]
    fn decodes_a_write_transaction() {
        let lanes = transaction(0x50, false, &[0x00, 0x2A]);
        assert_eq!(
            texts(&I2cSettings::default().decode(&lanes)),
            vec!["S", "50 W", "A", "00", "A", "2A", "A", "P"]
        );
    }

    #[test]
    fn read_transactions_show_the_direction() {
        let lanes = transaction(0x50, true, &[0xFF]);
        let out = texts(&I2cSettings::default().decode(&lanes));
        assert_eq!(out[1], "50 R");
    }

    #[test]
    fn raw_address_frame_can_be_shown_instead() {
        let lanes = transaction(0x50, false, &[]);
        let s = I2cSettings {
            split_address: false,
            ..Default::default()
        };
        // 0x50 << 1 with the write bit clear is 0xA0 on the wire.
        assert_eq!(texts(&s.decode(&lanes))[1], "A0");
    }

    #[test]
    fn a_nack_is_flagged() {
        let lanes = transaction_acked(0x50, false, &[0x00], false);
        let out = I2cSettings::default().decode(&lanes);
        assert!(
            texts(&out).contains(&"N".to_string()),
            "an unacknowledged byte must show as N: {:?}",
            texts(&out)
        );
        assert!(out[0].words.iter().any(|w| w.kind == WordKind::Error));
    }

    #[test]
    fn traffic_outside_a_transaction_is_ignored() {
        // Clocking with no START must not produce bytes.
        let mut lanes = transaction(0x50, false, &[0x00]);
        let sda = lanes[SDA].as_ref().unwrap().clone();
        // Drop the START by holding SDA high until the first clock.
        lanes[SDA] = Some(BitLane::new(
            sda.changes
                .into_iter()
                .skip_while(|(t, _)| *t <= HALF * 2)
                .collect(),
        ));
        let out = I2cSettings::default().decode(&lanes);
        assert!(
            !out[0].words.iter().any(|w| w.text.contains('W')),
            "no address should decode without a START: {:?}",
            texts(&out)
        );
    }

    #[test]
    fn missing_a_line_yields_an_empty_row() {
        let mut lanes = transaction(0x50, false, &[0x00]);
        lanes[SDA] = None;
        let out = I2cSettings::default().decode(&lanes);
        assert_eq!(out.len(), 1);
        assert!(out[0].words.is_empty());
    }
}

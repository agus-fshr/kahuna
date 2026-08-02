//! Protocol decoders.
//!
//! A protocol decoder turns the transitions of several signals into a sequence
//! of words spanning time ranges, which are then drawn as their own rows in the
//! waveform view. This is distinct from an *instruction* decoder, which
//! translates a single n-bit value in isolation; see
//! `docs/plugins/instruction-decoders.md`.
//!
//! Decoding is deliberately split from the wave container: a decoder consumes
//! [`BitLane`]s of pre-extracted transitions and returns [`DecodedLane`]s, so it
//! can be unit tested without loading a waveform.

use derive_more::Display;
use serde::{Deserialize, Serialize};

use crate::wave_container::VariableRef;

pub mod dialog;
pub mod i2c;
pub mod spi;
pub mod uart;

/// A single signal's transitions, as `(time, bit)` pairs sorted by time.
///
/// Values that are not a clean 0 or 1 (x, z, or a multi-bit vector) are
/// recorded as [`Bit::Invalid`] so a decoder can refuse to decode across them
/// rather than silently treating them as 0.
#[derive(Debug, Clone, Default)]
pub struct BitLane {
    pub changes: Vec<(u64, Bit)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit {
    Zero,
    One,
    Invalid,
}

impl Bit {
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, Bit::One)
    }
}

impl BitLane {
    #[must_use]
    pub fn new(changes: Vec<(u64, Bit)>) -> Self {
        Self { changes }
    }

    /// Value of the lane at `time`, i.e. the value set by the most recent
    /// transition at or before `time`. `None` before the first transition.
    #[must_use]
    pub fn value_at(&self, time: u64) -> Option<Bit> {
        let idx = self.changes.partition_point(|(t, _)| *t <= time);
        (idx > 0).then(|| self.changes[idx - 1].1)
    }

    /// Times at which the lane transitions to `to`, in order.
    #[must_use]
    pub fn edges_to(&self, to: Bit) -> Vec<u64> {
        let mut out = vec![];
        let mut prev: Option<Bit> = None;
        for &(t, v) in &self.changes {
            if prev.is_some_and(|p| p != v) && v == to {
                out.push(t);
            }
            prev = Some(v);
        }
        out
    }
}

/// What a decoded word represents. Drives how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordKind {
    /// Successfully decoded payload.
    Data,
    /// The decoder could not make sense of this span, e.g. a word truncated by
    /// chip select going inactive, or an `x`/`z` sampled on a data line.
    Error,
}

/// One decoded word occupying the half-open time range `[start, end)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedWord {
    pub start: u64,
    pub end: u64,
    pub text: String,
    pub kind: WordKind,
}

/// A row of decoded output. A decoder may produce several: SPI produces one for
/// MOSI and one for MISO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLane {
    pub name: String,
    pub words: Vec<DecodedWord>,
}

/// Bit order within a decoded word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, Default)]
pub enum BitOrder {
    #[default]
    #[display("MSB first")]
    MsbFirst,
    #[display("LSB first")]
    LsbFirst,
}

impl BitOrder {
    pub const ALL: [BitOrder; 2] = [BitOrder::MsbFirst, BitOrder::LsbFirst];
}

/// How decoded words are rendered as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, Default)]
pub enum WordFormat {
    #[default]
    #[display("Hexadecimal")]
    Hex,
    #[display("Decimal")]
    Decimal,
    #[display("Binary")]
    Binary,
    #[display("ASCII")]
    Ascii,
}

impl WordFormat {
    pub const ALL: [WordFormat; 4] = [
        WordFormat::Hex,
        WordFormat::Decimal,
        WordFormat::Binary,
        WordFormat::Ascii,
    ];

    /// Render `value` of `bits` width.
    #[must_use]
    pub fn format(self, value: u64, bits: u32) -> String {
        match self {
            WordFormat::Hex => format!("{value:0width$X}", width = (bits as usize).div_ceil(4)),
            WordFormat::Decimal => format!("{value}"),
            WordFormat::Binary => format!("{value:0width$b}", width = bits as usize),
            WordFormat::Ascii => {
                let c = u8::try_from(value).map(char::from);
                match c {
                    // Only printable ASCII is shown as a character; anything
                    // else would render as a box or reorder the line.
                    Ok(c) if c.is_ascii_graphic() || c == ' ' => format!("'{c}'"),
                    _ => format!("{value:02X}"),
                }
            }
        }
    }
}

/// Which protocol a decoder speaks. New protocols get a variant here plus a
/// module alongside [`spi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum Protocol {
    #[display("SPI")]
    Spi,
    #[display("I2C")]
    I2c,
    #[display("UART")]
    Uart,
}

impl Protocol {
    pub const ALL: [Protocol; 3] = [Protocol::Spi, Protocol::I2c, Protocol::Uart];

    /// Signal roles this protocol binds, in the order they should be presented.
    #[must_use]
    pub const fn roles(self) -> &'static [Role] {
        match self {
            Protocol::Spi => spi::ROLES,
            Protocol::I2c => i2c::ROLES,
            Protocol::Uart => uart::ROLES,
        }
    }
}

/// A named signal input of a decoder, e.g. SPI's `SCLK`.
///
/// This is static metadata describing a protocol, never part of saved state,
/// so it is deliberately not serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    pub name: &'static str,
    /// Whether decoding is impossible without this signal.
    pub required: bool,
    /// Lowercase substrings that suggest a signal fills this role, used to
    /// pre-assign roles when a decoder is created from a group.
    pub aliases: &'static [&'static str],
}

/// Signals bound to a decoder's roles. Kept parallel to
/// [`Protocol::roles`] by index rather than by name so that renaming a role
/// does not silently orphan a binding in a saved state file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RoleBindings {
    pub signals: Vec<Option<VariableRef>>,
}

impl RoleBindings {
    /// Bind roles by matching each role's aliases against the signal names in
    /// `candidates`, longest alias first so that `sclk` wins over `clk`.
    #[must_use]
    pub fn guess(protocol: Protocol, candidates: &[VariableRef]) -> Self {
        let roles = protocol.roles();
        let mut signals: Vec<Option<VariableRef>> = vec![None; roles.len()];
        let mut taken = vec![false; candidates.len()];

        // Longer aliases are more specific, so let them claim signals first.
        let mut by_alias: Vec<(usize, &str)> = roles
            .iter()
            .enumerate()
            .flat_map(|(i, r)| r.aliases.iter().map(move |a| (i, *a)))
            .collect();
        by_alias.sort_by_key(|(_, a)| std::cmp::Reverse(a.len()));

        for (role_idx, alias) in by_alias {
            if signals[role_idx].is_some() {
                continue;
            }
            for (cand_idx, cand) in candidates.iter().enumerate() {
                if taken[cand_idx] {
                    continue;
                }
                if cand.name.to_ascii_lowercase().contains(alias) {
                    signals[role_idx] = Some(cand.clone());
                    taken[cand_idx] = true;
                    break;
                }
            }
        }

        Self { signals }
    }

    #[must_use]
    pub fn get(&self, idx: usize) -> Option<&VariableRef> {
        self.signals.get(idx).and_then(Option::as_ref)
    }

    /// Roles that are required but unbound, by name.
    #[must_use]
    pub fn missing(&self, protocol: Protocol) -> Vec<&'static str> {
        protocol
            .roles()
            .iter()
            .enumerate()
            .filter(|(i, r)| r.required && self.get(*i).is_none())
            .map(|(_, r)| r.name)
            .collect()
    }

    /// Grow or shrink to match the role count of `protocol`, keeping bindings
    /// that still have a role.
    pub fn fit_to(&mut self, protocol: Protocol) {
        self.signals.resize(protocol.roles().len(), None);
    }
}

/// Everything a decode result depends on.
///
/// A cached result stays valid exactly while this compares equal, so anything
/// that can change the output has to be represented here.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeKey {
    /// Bumped when a waveform is reloaded, so results from the previous file
    /// are never reused for the new one.
    pub generation: u64,
    pub settings: DecoderSettings,
    pub bindings: RoleBindings,
    /// Whether each bound signal had data available when the decode ran.
    ///
    /// Signals load asynchronously, so a decode can legitimately run before its
    /// inputs exist and produce nothing. Without this the empty result would be
    /// cached and the row would stay blank forever.
    pub loaded: Vec<bool>,
}

/// A decode result together with the inputs it was produced from.
pub struct CachedDecode {
    pub key: DecodeKey,
    pub lanes: Vec<DecodedLane>,
}

impl CachedDecode {
    #[must_use]
    pub fn is_valid_for(&self, key: &DecodeKey) -> bool {
        self.key == *key
    }
}

/// Per-protocol decoder settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecoderSettings {
    Spi(spi::SpiSettings),
    I2c(i2c::I2cSettings),
    Uart(uart::UartSettings),
}

impl DecoderSettings {
    #[must_use]
    pub fn for_protocol(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Spi => DecoderSettings::Spi(spi::SpiSettings::default()),
            Protocol::I2c => DecoderSettings::I2c(i2c::I2cSettings::default()),
            Protocol::Uart => DecoderSettings::Uart(uart::UartSettings::default()),
        }
    }

    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        match self {
            DecoderSettings::Spi(_) => Protocol::Spi,
            DecoderSettings::I2c(_) => Protocol::I2c,
            DecoderSettings::Uart(_) => Protocol::Uart,
        }
    }

    /// Decode `lanes`, which are indexed to match [`Protocol::roles`].
    /// Entries are `None` for unbound roles.
    #[must_use]
    pub fn decode(&self, lanes: &[Option<BitLane>]) -> Vec<DecodedLane> {
        match self {
            DecoderSettings::Spi(s) => s.decode(lanes),
            DecoderSettings::I2c(s) => s.decode(lanes),
            DecoderSettings::Uart(s) => s.decode(lanes),
        }
    }

    /// Names of the rows this decoder produces, for laying out the item before
    /// any waveform data is available.
    #[must_use]
    pub fn lane_names(&self) -> Vec<String> {
        match self {
            DecoderSettings::Spi(s) => s.lane_names(),
            DecoderSettings::I2c(s) => s.lane_names(),
            DecoderSettings::Uart(s) => s.lane_names(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave_container::VariableRefExt;

    fn lane(changes: &[(u64, u8)]) -> BitLane {
        BitLane::new(
            changes
                .iter()
                .map(|&(t, v)| (t, if v == 0 { Bit::Zero } else { Bit::One }))
                .collect(),
        )
    }

    fn cache_key() -> DecodeKey {
        DecodeKey {
            generation: 1,
            settings: DecoderSettings::for_protocol(Protocol::Spi),
            bindings: RoleBindings {
                signals: vec![None; Protocol::Spi.roles().len()],
            },
            loaded: vec![true; Protocol::Spi.roles().len()],
        }
    }

    fn cached() -> CachedDecode {
        CachedDecode {
            key: cache_key(),
            lanes: vec![],
        }
    }

    #[test]
    fn a_cached_decode_is_reused_for_an_identical_key() {
        assert!(cached().is_valid_for(&cache_key()));
    }

    #[test]
    fn reloading_the_waveform_invalidates_the_cache() {
        let key = DecodeKey {
            generation: 2,
            ..cache_key()
        };
        assert!(!cached().is_valid_for(&key));
    }

    #[test]
    fn changing_settings_invalidates_the_cache() {
        let mut settings = spi::SpiSettings::default();
        settings.word_size = 4;
        let key = DecodeKey {
            settings: DecoderSettings::Spi(settings),
            ..cache_key()
        };
        assert!(!cached().is_valid_for(&key));
    }

    #[test]
    fn rebinding_a_role_invalidates_the_cache() {
        let mut bindings = cache_key().bindings;
        bindings.signals[spi::SCLK] = Some(VariableRef::from_hierarchy_string("tb.spi.sclk"));
        let key = DecodeKey {
            bindings,
            ..cache_key()
        };
        assert!(!cached().is_valid_for(&key));
    }

    #[test]
    fn a_signal_finishing_loading_invalidates_the_cache() {
        // Signals load asynchronously, so a decode can run before its inputs
        // exist. Caching that empty result forever would leave the row blank.
        let mut loaded = cache_key().loaded;
        loaded[spi::SCLK] = false;
        let stale = CachedDecode {
            key: DecodeKey {
                loaded,
                ..cache_key()
            },
            lanes: vec![],
        };
        assert!(
            !stale.is_valid_for(&cache_key()),
            "a decode made without data must not survive the data arriving"
        );
    }

    #[test]
    fn value_at_holds_previous_value() {
        let l = lane(&[(10, 1), (20, 0)]);
        assert_eq!(l.value_at(0), None, "before the first transition");
        assert_eq!(l.value_at(10), Some(Bit::One), "at the transition");
        assert_eq!(l.value_at(15), Some(Bit::One), "held between transitions");
        assert_eq!(l.value_at(20), Some(Bit::Zero));
        assert_eq!(l.value_at(9999), Some(Bit::Zero), "held after the last");
    }

    #[test]
    fn edges_to_ignores_repeated_values() {
        let l = lane(&[(0, 0), (10, 1), (20, 1), (30, 0), (40, 1)]);
        assert_eq!(l.edges_to(Bit::One), vec![10, 40]);
        assert_eq!(l.edges_to(Bit::Zero), vec![30]);
    }

    #[test]
    fn word_format_pads_to_width() {
        assert_eq!(WordFormat::Hex.format(0x0A, 8), "0A");
        assert_eq!(WordFormat::Binary.format(0b101, 8), "00000101");
        assert_eq!(WordFormat::Decimal.format(42, 8), "42");
        assert_eq!(WordFormat::Ascii.format(u64::from(b'K'), 8), "'K'");
        assert_eq!(
            WordFormat::Ascii.format(0x00, 8),
            "00",
            "unprintable falls back to hex"
        );
    }

    #[test]
    fn guess_prefers_the_more_specific_alias() {
        let candidates = ["clk", "sclk", "mosi", "miso", "cs_n"]
            .iter()
            .map(|n| VariableRef::from_hierarchy_string(&format!("tb.spi.{n}")))
            .collect::<Vec<_>>();

        let b = RoleBindings::guess(Protocol::Spi, &candidates);
        let roles = Protocol::Spi.roles();
        let named = |name: &str| {
            let i = roles.iter().position(|r| r.name == name).unwrap();
            b.get(i).map(|v| v.name.clone())
        };

        // "sclk" contains "clk", so both signals match the SCLK role; the
        // longer alias must win rather than whichever comes first.
        assert_eq!(named("SCLK"), Some("sclk".to_string()));
        assert_eq!(named("MOSI"), Some("mosi".to_string()));
        assert_eq!(named("MISO"), Some("miso".to_string()));
        assert_eq!(named("CS"), Some("cs_n".to_string()));
        assert!(b.missing(Protocol::Spi).is_empty());
    }

    #[test]
    fn missing_reports_only_required_roles() {
        let b = RoleBindings {
            signals: vec![None; Protocol::Spi.roles().len()],
        };
        let missing = b.missing(Protocol::Spi);
        assert!(missing.contains(&"SCLK"));
        assert!(
            !missing.contains(&"MISO"),
            "MISO is optional; a write-only bus still decodes"
        );
    }
}

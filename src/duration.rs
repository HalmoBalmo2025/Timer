//! The countdown's duration: the typed value, how it is read from the
//! field and the data file and written back, and which whole second a round
//! shows when.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64, ParseIntError};
use std::str::FromStr;

/// How long one round of the countdown takes, in tenths of a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CountdownDuration(NonZeroU32);

impl CountdownDuration {
    /// 18 s, used until another duration has been started.
    pub(crate) const DEFAULT: Self = match NonZeroU32::new(180) {
        Some(tenths) => Self(tenths),
        // 180 is not zero; this arm only satisfies the type.
        None => Self(NonZeroU32::MIN),
    };

    fn tenths(self) -> NonZeroU64 {
        NonZeroU64::from(self.0)
    }

    /// The whole seconds and the tenth after them: `17.5` is 17 and 5.
    const fn parts(self) -> Parts {
        Parts {
            whole: self.0.get() / 10,
            tenth: self.0.get() % 10,
        }
    }
}

/// A duration split for writing it out.
struct Parts {
    whole: u32,
    /// 0 to 9.
    tenth: u32,
}

impl Default for CountdownDuration {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a text is not a countdown duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DurationError {
    /// The text holds no digits.
    Empty,
    /// Anything but `18`, `17.5`, `17.` or `.5`.
    Malformed,
    /// The number does not fit.
    TooLarge(ParseIntError),
    /// The duration is zero.
    Zero,
}

impl fmt::Display for DurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the duration is empty"),
            Self::Malformed => f.write_str("the duration is not a number with at most one decimal"),
            Self::TooLarge(source) => write!(f, "the duration is too large: {source}"),
            Self::Zero => f.write_str("the duration is zero"),
        }
    }
}

/// Parses seconds with at most one decimal: `18`, `17.5`, `17.` or `.5`.
impl FromStr for CountdownDuration {
    type Err = DurationError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let (whole, tenth) = input.split_once('.').unwrap_or((input, ""));
        if whole.is_empty() && tenth.is_empty() {
            return Err(DurationError::Empty);
        }
        if tenth.len() > 1 {
            return Err(DurationError::Malformed);
        }
        // The tenths are the digits side by side: "17.5" is 175, "17" is
        // 170 and ".5" is 5.
        let digits = format!("{whole}{}", if tenth.is_empty() { "0" } else { tenth });
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DurationError::Malformed);
        }
        let tenths = digits.parse().map_err(DurationError::TooLarge)?;
        NonZeroU32::new(tenths).map(Self).ok_or(DurationError::Zero)
    }
}

/// The duration as typed back into the input: `18` or `17.5`.
impl fmt::Display for CountdownDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.parts() {
            Parts { whole, tenth: 0 } => write!(f, "{whole}"),
            Parts { whole, tenth } => write!(f, "{whole}.{tenth}"),
        }
    }
}

/// Stored as seconds. Whole durations are written as integers, so the file
/// keeps its old format unless a duration with tenths is actually used.
impl Serialize for CountdownDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.parts() {
            Parts { whole, tenth: 0 } => serializer.serialize_u32(whole),
            Parts { whole: _, tenth: _ } => {
                serializer.serialize_f64(f64::from(self.0.get()) / 10.0)
            }
        }
    }
}

impl<'de> Deserialize<'de> for CountdownDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Rounding to one decimal in text keeps float-to-integer casts out;
        // the parser then rejects negative, zero and huge values.
        let seconds = f64::deserialize(deserializer)?;
        format!("{seconds:.1}").parse().map_err(D::Error::custom)
    }
}

/// Whole seconds shown `elapsed` tenths after the start of a round of
/// `total`.
///
/// The countdown runs down in whole seconds: `18` shows `17, …, 1, 0`, each
/// for one second, then jumps back to `17`. The fraction of a duration with
/// tenths is simply the first, shorter step: `17.5` shows `17` for half a
/// second, then `16, …, 0`. The full duration itself never appears (its first
/// step is already ticking away), so one round is still exactly `total` long
/// and ends with a full second of `0`.
pub(crate) fn countdown_remaining(total: CountdownDuration, elapsed: u64) -> u64 {
    tenths_left(total, elapsed).div_ceil(10).saturating_sub(1)
}

/// Elapsed tenths at which the shown value next changes after `elapsed`.
pub(crate) fn next_countdown_change(total: CountdownDuration, elapsed: u64) -> u64 {
    // Steps end whenever a multiple of ten tenths is left, the last one
    // exactly at the end of the round.
    let step = match tenths_left(total, elapsed) % 10 {
        0 => 10,
        partial => partial,
    };
    elapsed.saturating_add(step)
}

/// Tenths left in the current round, from `total` down to 1.
fn tenths_left(total: CountdownDuration, elapsed: u64) -> u64 {
    let total = total.tenths();
    total.get().saturating_sub(elapsed % total)
}

pub(crate) fn format_countdown(seconds: u64) -> String {
    format!("{seconds:02}")
}

/// Digits the duration field takes before the decimal point: 99999999.9 s
/// is the longest duration it can hold. Its tenths fit a `u32`, so the
/// field holds only valid durations, empty ones and zero.
pub(crate) const MAX_WHOLE_DIGITS: usize = 8;

/// Keeps only what a duration may consist of: up to [`MAX_WHOLE_DIGITS`]
/// digits, one decimal separator (a comma becomes a dot) and at most one
/// digit after it.
pub(crate) fn sanitize_duration_input(raw: &str) -> String {
    let mut out = String::new();
    let mut tenths_digits = None;
    for c in raw.chars() {
        match (c, tenths_digits) {
            // Before the point, `out` holds only digits.
            ('0'..='9', None) => {
                if out.len() < MAX_WHOLE_DIGITS {
                    out.push(c);
                }
            }
            ('0'..='9', Some(0)) => {
                out.push(c);
                tenths_digits = Some(1);
            }
            ('.' | ',', None) => {
                out.push('.');
                tenths_digits = Some(0);
            }
            _ => {}
        }
    }
    out
}

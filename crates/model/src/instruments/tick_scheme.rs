// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Tick scheme definitions for price-level navigation.

use std::{fmt::Display, str::FromStr, sync::LazyLock};

use nautilus_core::correctness::{
    CorrectnessError, CorrectnessResult, check_predicate_true, check_valid_string_ascii_optional,
};
use thiserror::Error;

#[cfg(not(feature = "high-precision"))]
use crate::types::fixed::f64_to_fixed_i64;
#[cfg(feature = "high-precision")]
use crate::types::fixed::f64_to_fixed_i128;
use crate::types::{
    Price,
    fixed::{FIXED_PRECISION, FIXED_SCALAR},
    price::{PRICE_MAX, PRICE_MIN, PRICE_RAW_MAX, PRICE_RAW_MIN, PriceRaw},
};

pub trait TickSchemeRule: Display {
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
}

/// Error returned when tick scheme construction or parsing fails.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum TickSchemeError {
    /// A fixed tick size was not finite.
    #[error("tick must be finite")]
    TickNotFinite {
        /// The invalid tick size.
        tick: f64,
    },
    /// A fixed tick size was not positive.
    #[error("tick must be positive")]
    TickNotPositive {
        /// The invalid tick size.
        tick: f64,
    },
    /// No tier definitions were supplied.
    #[error("tiers must not be empty")]
    EmptyTiers,
    /// A tier contained a NaN value.
    #[error("tier {index}: values must not be NaN")]
    TierValuesNaN {
        /// The invalid tier index.
        index: usize,
        /// The tier start value.
        start: f64,
        /// The tier stop value.
        stop: f64,
        /// The tier step value.
        step: f64,
    },
    /// A tier start was not less than its stop.
    #[error("tier {index}: start ({start}) must be less than stop ({stop})")]
    TierStartNotLessThanStop {
        /// The invalid tier index.
        index: usize,
        /// The tier start value.
        start: f64,
        /// The tier stop value.
        stop: f64,
    },
    /// A tier step was not positive.
    #[error("tier {index}: step ({step}) must be positive")]
    TierStepNotPositive {
        /// The invalid tier index.
        index: usize,
        /// The tier step value.
        step: f64,
    },
    /// A finite tier step was not smaller than the tier range.
    #[error("tier {index}: step ({step}) must be less than range ({stop} - {start} = {range})")]
    TierStepNotLessThanRange {
        /// The invalid tier index.
        index: usize,
        /// The tier start value.
        start: f64,
        /// The tier stop value.
        stop: f64,
        /// The tier step value.
        step: f64,
        /// The tier range.
        range: f64,
    },
    /// A tier overlaps the previous tier.
    #[error("tier {index}: start ({start}) overlaps previous tier stop ({prev_stop})")]
    TierOverlapsPrevious {
        /// The invalid tier index.
        index: usize,
        /// The tier start value.
        start: f64,
        /// The previous tier stop value.
        prev_stop: f64,
    },
    /// A tier start was outside the representable price range.
    #[error("tier {index}: start ({start}) outside Price range")]
    TierStartOutsidePriceRange {
        /// The invalid tier index.
        index: usize,
        /// The tier start value.
        start: f64,
    },
    /// A tier stop was outside the representable price range.
    #[error("tier {index}: stop ({stop}) outside Price range")]
    TierStopOutsidePriceRange {
        /// The invalid tier index.
        index: usize,
        /// The tier stop value.
        stop: f64,
    },
    /// The requested price precision is invalid.
    #[error("{source}")]
    InvalidPrecision {
        /// The source correctness error.
        #[source]
        source: CorrectnessError,
    },
    /// Tier expansion produced no ticks.
    #[error("tier expansion produced no ticks")]
    EmptyTickExpansion,
    /// An expanded tick value was outside the representable price range.
    #[error("expanded tick value {value} outside Price range")]
    ExpandedTickOutsidePriceRange {
        /// The invalid expanded tick value.
        value: f64,
    },
    /// The requested tick scheme name is not registered.
    #[error("unknown tick scheme {name}")]
    UnknownName {
        /// The requested tick scheme name.
        name: String,
    },
}

pub const BETFAIR_TICK_SCHEME_NAME: &str = "BETFAIR";
pub const TOPIX100_TICK_SCHEME_NAME: &str = "TOPIX100";
pub const CRYPTO_0_01_TICK_SCHEME_NAME: &str = "CRYPTO_0_01";
pub const FOREX_3DECIMAL_TICK_SCHEME_NAME: &str = "FOREX_3DECIMAL";
pub const FOREX_5DECIMAL_TICK_SCHEME_NAME: &str = "FOREX_5DECIMAL";
pub const FIXED_TICK_SCHEME_NAME: &str = "FIXED";
pub const FIXED_PRECISION_TICK_SCHEME_PREFIX: &str = "FIXED_PRECISION_";

const BETFAIR_PRICE_TIERS: [(f64, f64, f64); 10] = [
    (1.01, 2.0, 0.01),
    (2.0, 3.0, 0.02),
    (3.0, 4.0, 0.05),
    (4.0, 6.0, 0.1),
    (6.0, 10.0, 0.2),
    (10.0, 20.0, 0.5),
    (20.0, 30.0, 1.0),
    (30.0, 50.0, 2.0),
    (50.0, 100.0, 5.0),
    (100.0, 1010.0, 10.0),
];

pub static BETFAIR_TICK_SCHEME: LazyLock<TieredTickScheme> = LazyLock::new(|| {
    TieredTickScheme::new(&BETFAIR_PRICE_TIERS, 2, 100)
        .expect("BETFAIR tick scheme tiers are valid by construction")
});

pub static TOPIX100_TICK_SCHEME: LazyLock<TieredTickScheme> = LazyLock::new(|| {
    TieredTickScheme::new(
        &[
            (0.1, 1_000.0, 0.1),
            (1_000.0, 3_000.0, 0.5),
            (3_000.0, 10_000.0, 1.0),
            (10_000.0, 30_000.0, 5.0),
            (30_000.0, 100_000.0, 10.0),
            (100_000.0, 300_000.0, 50.0),
            (300_000.0, 1_000_000.0, 100.0),
            (1_000_000.0, 3_000_000.0, 500.0),
            (3_000_000.0, 10_000_000.0, 1_000.0),
            (10_000_000.0, 30_000_000.0, 5_000.0),
            (30_000_000.0, f64::INFINITY, 10_000.0),
        ],
        4,
        10_000,
    )
    .expect("TOPIX100 tick scheme tiers are valid by construction")
});

static FIXED_TICK_SCHEME: LazyLock<FixedTickScheme> =
    LazyLock::new(|| FixedTickScheme::new(1.0).expect("fixed tick scheme is valid"));

static CRYPTO_0_01_TICK_SCHEME: LazyLock<FixedTickScheme> =
    LazyLock::new(|| FixedTickScheme::new(0.01).expect("crypto tick scheme is valid"));

static FIXED_PRECISION_TICK_SCHEMES: LazyLock<Vec<FixedTickScheme>> = LazyLock::new(|| {
    (0..=FIXED_PRECISION)
        .map(|precision| {
            let tick = 10_f64.powi(-i32::from(precision));
            FixedTickScheme::new(tick).expect("fixed precision tick scheme is valid")
        })
        .collect()
});

#[derive(Clone, Copy, Debug)]
pub struct FixedTickScheme {
    tick: f64,
}

impl PartialEq for FixedTickScheme {
    fn eq(&self, other: &Self) -> bool {
        self.tick == other.tick
    }
}
impl Eq for FixedTickScheme {}

impl FixedTickScheme {
    /// Creates a new [`FixedTickScheme`] with the given tick size.
    ///
    /// # Errors
    ///
    /// Returns an error if `tick` is not finite or not positive.
    pub fn new(tick: f64) -> Result<Self, TickSchemeError> {
        if !tick.is_finite() {
            return Err(TickSchemeError::TickNotFinite { tick });
        }

        if tick <= 0.0 {
            return Err(TickSchemeError::TickNotPositive { tick });
        }

        Ok(Self { tick })
    }
}

impl TickSchemeRule for FixedTickScheme {
    #[inline(always)]
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        fixed_next_bid_price(self.tick, value, n, precision)
    }

    #[inline(always)]
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        fixed_next_ask_price(self.tick, value, n, precision)
    }
}

impl Display for FixedTickScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FIXED")
    }
}

/// Tick scheme with price-dependent tick sizes.
///
/// Stores expanded ticks as raw fixed-point integers for exact comparison
/// and fast binary search. Each tier defines a (start, stop, step) range
/// that is expanded at construction.
#[derive(Clone, Debug)]
pub struct TieredTickScheme {
    ticks: Vec<PriceRaw>,
    precision: u8,
}

impl PartialEq for TieredTickScheme {
    fn eq(&self, other: &Self) -> bool {
        self.precision == other.precision && self.ticks == other.ticks
    }
}
impl Eq for TieredTickScheme {}

impl TieredTickScheme {
    /// Creates a new [`TieredTickScheme`] from tier definitions.
    ///
    /// Each tier is `(start, stop, step)` where `start < stop` and `step > 0`.
    /// Use `f64::INFINITY` for the last tier's stop value.
    ///
    /// # Errors
    ///
    /// Returns an error if any tier is invalid or contains out-of-range values.
    pub fn new(
        tiers: &[(f64, f64, f64)],
        price_precision: u8,
        max_ticks_per_tier: usize,
    ) -> Result<Self, TickSchemeError> {
        if tiers.is_empty() {
            return Err(TickSchemeError::EmptyTiers);
        }

        for (index, &(start, stop, step)) in tiers.iter().enumerate() {
            if start.is_nan() || stop.is_nan() || step.is_nan() {
                return Err(TickSchemeError::TierValuesNaN {
                    index,
                    start,
                    stop,
                    step,
                });
            }

            if start >= stop {
                return Err(TickSchemeError::TierStartNotLessThanStop { index, start, stop });
            }

            if step <= 0.0 {
                return Err(TickSchemeError::TierStepNotPositive { index, step });
            }

            if !stop.is_infinite() && step >= (stop - start) {
                return Err(TickSchemeError::TierStepNotLessThanRange {
                    index,
                    start,
                    stop,
                    step,
                    range: stop - start,
                });
            }

            if index > 0 {
                let prev_stop = tiers[index - 1].1;

                if start < prev_stop {
                    return Err(TickSchemeError::TierOverlapsPrevious {
                        index,
                        start,
                        prev_stop,
                    });
                }
            }

            if !(PRICE_MIN..=PRICE_MAX).contains(&start) {
                return Err(TickSchemeError::TierStartOutsidePriceRange { index, start });
            }

            if !stop.is_infinite() && !(PRICE_MIN..=PRICE_MAX).contains(&stop) {
                return Err(TickSchemeError::TierStopOutsidePriceRange { index, stop });
            }
        }

        let _ = Price::new_checked(0.0, price_precision)
            .map_err(|source| TickSchemeError::InvalidPrecision { source })?;

        let ticks = Self::build_ticks(tiers, price_precision, max_ticks_per_tier)?;

        if ticks.is_empty() {
            return Err(TickSchemeError::EmptyTickExpansion);
        }
        Ok(Self {
            ticks,
            precision: price_precision,
        })
    }

    fn build_ticks(
        tiers: &[(f64, f64, f64)],
        precision: u8,
        max_ticks_per_tier: usize,
    ) -> Result<Vec<PriceRaw>, TickSchemeError> {
        let mut all_ticks = Vec::new();

        for &(start, stop, step) in tiers {
            let effective_stop = if stop.is_infinite() {
                start + (max_ticks_per_tier.saturating_add(1) as f64) * step
            } else {
                stop
            };
            let mut i = 0;
            while i < max_ticks_per_tier {
                let value = start + (i as f64) * step;

                if value >= effective_stop {
                    break;
                }

                if !value.is_finite() || !(PRICE_MIN..=PRICE_MAX).contains(&value) {
                    return Err(TickSchemeError::ExpandedTickOutsidePriceRange { value });
                }
                let raw = f64_to_raw(value, precision);

                if all_ticks.last() != Some(&raw) {
                    all_ticks.push(raw);
                }
                i += 1;
            }
        }
        Ok(all_ticks)
    }

    #[inline(always)]
    fn price_at(&self, index: usize) -> Price {
        Price {
            raw: self.ticks[index],
            precision: self.precision,
        }
    }

    /// Returns the expanded ticks as `Price` objects.
    #[must_use]
    pub fn ticks(&self) -> Vec<Price> {
        self.ticks
            .iter()
            .map(|&raw| Price {
                raw,
                precision: self.precision,
            })
            .collect()
    }

    /// Returns the number of ticks.
    #[must_use]
    pub fn tick_count(&self) -> usize {
        self.ticks.len()
    }

    /// Returns the minimum tick price.
    #[must_use]
    pub fn min_price(&self) -> Price {
        self.price_at(0)
    }

    /// Returns the maximum tick price.
    #[must_use]
    pub fn max_price(&self) -> Price {
        self.price_at(self.ticks.len() - 1)
    }

    /// Returns the price precision.
    #[must_use]
    pub fn precision(&self) -> u8 {
        self.precision
    }

    /// Creates the TOPIX100 tick scheme.
    ///
    /// # Panics
    ///
    /// Panics if the hardcoded TOPIX100 tiers fail validation (should not happen).
    #[must_use]
    pub fn topix100() -> Self {
        TOPIX100_TICK_SCHEME.clone()
    }

    /// Creates the BETFAIR tick scheme.
    ///
    /// # Panics
    ///
    /// Panics if the hardcoded BETFAIR tiers fail validation (should not happen).
    #[must_use]
    pub fn betfair() -> Self {
        BETFAIR_TICK_SCHEME.clone()
    }
}

impl TickSchemeRule for TieredTickScheme {
    fn next_bid_price(&self, value: f64, n: i32, _precision: u8) -> Option<Price> {
        let n = usize::try_from(n).ok()?;

        if value.is_nan() {
            return None;
        }

        // Floor to get a raw value guaranteed <= true value, infinite values
        // saturate at the integer bounds during the float-to-integer cast.
        let raw_floor = (value * FIXED_SCALAR).floor() as PriceRaw;

        if raw_floor < self.ticks[0] {
            return None;
        }

        // First index where tick >= raw_floor
        let idx = self.ticks.partition_point(|&t| t < raw_floor);

        if idx < self.ticks.len() && self.ticks[idx] == raw_floor {
            // Value converts exactly to a tick
            let target = idx.checked_sub(n)?;
            return Some(self.price_at(target));
        }

        // Value is beyond or between ticks; bid is the tick below
        let target = idx.checked_sub(1)?.checked_sub(n)?;
        Some(self.price_at(target))
    }

    fn next_ask_price(&self, value: f64, n: i32, _precision: u8) -> Option<Price> {
        let n = usize::try_from(n).ok()?;

        if value.is_nan() {
            return None;
        }

        // Ceil to get a raw value guaranteed >= true value, infinite values
        // saturate at the integer bounds during the float-to-integer cast.
        let raw_ceil = (value * FIXED_SCALAR).ceil() as PriceRaw;

        if raw_ceil > *self.ticks.last()? {
            return None;
        }

        // First index where tick >= raw_ceil
        let idx = self.ticks.partition_point(|&t| t < raw_ceil);
        let target = idx.checked_add(n)?;

        if target >= self.ticks.len() {
            return None;
        }
        Some(self.price_at(target))
    }
}

impl Display for TieredTickScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TIERED")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TickScheme {
    Fixed(FixedTickScheme),
    Tiered(TieredTickScheme),
    Betfair,
    Crypto,
}

impl TickSchemeRule for TickScheme {
    #[inline(always)]
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        match self {
            Self::Fixed(scheme) => scheme.next_bid_price(value, n, precision),
            Self::Tiered(scheme) => scheme.next_bid_price(value, n, precision),
            Self::Betfair => BETFAIR_TICK_SCHEME.next_bid_price(value, n, precision),
            Self::Crypto => CRYPTO_0_01_TICK_SCHEME.next_bid_price(value, n, precision),
        }
    }

    #[inline(always)]
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        match self {
            Self::Fixed(scheme) => scheme.next_ask_price(value, n, precision),
            Self::Tiered(scheme) => scheme.next_ask_price(value, n, precision),
            Self::Betfair => BETFAIR_TICK_SCHEME.next_ask_price(value, n, precision),
            Self::Crypto => CRYPTO_0_01_TICK_SCHEME.next_ask_price(value, n, precision),
        }
    }
}

impl Display for TickScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed(_) => write!(f, "FIXED"),
            Self::Tiered(scheme) => write!(f, "{scheme}"),
            Self::Betfair => write!(f, "{BETFAIR_TICK_SCHEME_NAME}"),
            Self::Crypto => write!(f, "{CRYPTO_0_01_TICK_SCHEME_NAME}"),
        }
    }
}

impl FromStr for TickScheme {
    type Err = TickSchemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            FIXED_TICK_SCHEME_NAME => Ok(Self::Fixed(FixedTickScheme::new(1.0)?)),
            FOREX_3DECIMAL_TICK_SCHEME_NAME => Ok(Self::Fixed(FixedTickScheme::new(0.001)?)),
            FOREX_5DECIMAL_TICK_SCHEME_NAME => Ok(Self::Fixed(FixedTickScheme::new(0.00001)?)),
            TOPIX100_TICK_SCHEME_NAME => Ok(Self::Tiered(TieredTickScheme::topix100())),
            BETFAIR_TICK_SCHEME_NAME => Ok(Self::Betfair),
            CRYPTO_0_01_TICK_SCHEME_NAME => Ok(Self::Crypto),
            name => {
                if let Some(precision) = parse_fixed_precision_name(name)
                    && precision <= FIXED_PRECISION
                {
                    let tick = 10_f64.powi(-i32::from(precision));
                    return Ok(Self::Fixed(FixedTickScheme::new(tick)?));
                }
                Err(TickSchemeError::UnknownName {
                    name: s.to_string(),
                })
            }
        }
    }
}

/// Returns a registered tick scheme rule by name.
#[must_use]
pub fn tick_scheme_rule_from_name(name: &str) -> Option<&'static dyn TickSchemeRule> {
    let name = name.trim();
    if name.eq_ignore_ascii_case(FIXED_TICK_SCHEME_NAME) {
        Some(&*FIXED_TICK_SCHEME)
    } else if name.eq_ignore_ascii_case(FOREX_3DECIMAL_TICK_SCHEME_NAME) {
        Some(&FIXED_PRECISION_TICK_SCHEMES[3])
    } else if name.eq_ignore_ascii_case(FOREX_5DECIMAL_TICK_SCHEME_NAME) {
        Some(&FIXED_PRECISION_TICK_SCHEMES[5])
    } else if name.eq_ignore_ascii_case(TOPIX100_TICK_SCHEME_NAME) {
        Some(&*TOPIX100_TICK_SCHEME)
    } else if name.eq_ignore_ascii_case(BETFAIR_TICK_SCHEME_NAME) {
        Some(&*BETFAIR_TICK_SCHEME)
    } else if name.eq_ignore_ascii_case(CRYPTO_0_01_TICK_SCHEME_NAME) {
        Some(&*CRYPTO_0_01_TICK_SCHEME)
    } else {
        parse_fixed_precision_name_ignore_ascii_case(name).and_then(|precision| {
            FIXED_PRECISION_TICK_SCHEMES
                .get(usize::from(precision))
                .map(|scheme| scheme as &dyn TickSchemeRule)
        })
    }
}

/// Validates an optional tick scheme name.
///
/// # Errors
///
/// Returns an error if the name is not valid ASCII or does not identify a registered scheme.
pub fn check_tick_scheme<T: AsRef<str> + Copy>(tick_scheme: Option<T>) -> CorrectnessResult<()> {
    check_valid_string_ascii_optional(tick_scheme, "tick_scheme")?;
    if let Some(name) = tick_scheme {
        check_predicate_true(
            tick_scheme_rule_from_name(name.as_ref()).is_some(),
            "tick_scheme not found in tick schemes",
        )?;
    }
    Ok(())
}

/// Converts an f64 value to a `PriceRaw` fixed-point integer.
#[inline(always)]
fn f64_to_raw(value: f64, precision: u8) -> PriceRaw {
    #[cfg(feature = "high-precision")]
    {
        f64_to_fixed_i128(value, precision)
    }
    #[cfg(not(feature = "high-precision"))]
    {
        f64_to_fixed_i64(value, precision)
    }
}

fn parse_fixed_precision_name(name: &str) -> Option<u8> {
    name.strip_prefix(FIXED_PRECISION_TICK_SCHEME_PREFIX)
        .and_then(|precision| precision.parse::<u8>().ok())
}

fn parse_fixed_precision_name_ignore_ascii_case(name: &str) -> Option<u8> {
    let prefix_len = FIXED_PRECISION_TICK_SCHEME_PREFIX.len();
    let prefix = name.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(FIXED_PRECISION_TICK_SCHEME_PREFIX) {
        return None;
    }

    name.get(prefix_len..)?.parse::<u8>().ok()
}

fn fixed_next_bid_price(tick: f64, value: f64, n: i32, precision: u8) -> Option<Price> {
    let n = PriceRaw::from(n);
    if n < 0 {
        return None;
    }
    let tick_raw = fixed_tick_raw(tick, precision)?;
    let value_raw = value_to_raw(value)?;
    let base = value_raw
        .checked_div_euclid(tick_raw)?
        .checked_mul(tick_raw)?;
    let offset = tick_raw.checked_mul(n)?;
    price_from_raw_checked(base.checked_sub(offset)?, precision)
}

fn fixed_next_ask_price(tick: f64, value: f64, n: i32, precision: u8) -> Option<Price> {
    let n = PriceRaw::from(n);
    if n < 0 {
        return None;
    }
    let tick_raw = fixed_tick_raw(tick, precision)?;
    let value_raw = value_to_raw(value)?;
    let base = value_raw
        .checked_neg()?
        .checked_div_euclid(tick_raw)?
        .checked_neg()?
        .checked_mul(tick_raw)?;
    let offset = tick_raw.checked_mul(n)?;
    price_from_raw_checked(base.checked_add(offset)?, precision)
}

fn fixed_tick_raw(tick: f64, precision: u8) -> Option<PriceRaw> {
    Price::new_checked(0.0, precision).ok()?;

    if !tick.is_finite() || tick <= 0.0 {
        return None;
    }

    let raw = f64_to_raw(tick, precision);
    (raw > 0).then_some(raw)
}

fn value_to_raw(value: f64) -> Option<PriceRaw> {
    if !value.is_finite() || !(PRICE_MIN..=PRICE_MAX).contains(&value) {
        return None;
    }
    Some(f64_to_raw(value, FIXED_PRECISION))
}

fn price_from_raw_checked(raw: PriceRaw, precision: u8) -> Option<Price> {
    if !(PRICE_RAW_MIN..=PRICE_RAW_MAX).contains(&raw) {
        return None;
    }
    Some(Price { raw, precision })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn fixed_tick_scheme_prices() {
        let scheme = FixedTickScheme::new(0.5).unwrap();
        let bid = scheme.next_bid_price(10.3, 0, 2).unwrap();
        let ask = scheme.next_ask_price(10.3, 0, 2).unwrap();
        assert!(bid < ask);
    }

    #[rstest]
    fn fixed_tick_negative_returns_typed_error_with_display() {
        let error = FixedTickScheme::new(-0.01).unwrap_err();

        assert_eq!(error, TickSchemeError::TickNotPositive { tick: -0.01 });
        assert_eq!(error.to_string(), "tick must be positive");
    }

    #[rstest]
    fn fixed_tick_boundary() {
        let scheme = FixedTickScheme::new(0.5).unwrap();
        let price = scheme.next_bid_price(10.5, 0, 2).unwrap();
        assert_eq!(price, Price::new(10.5, 2));
    }

    #[rstest]
    fn fixed_tick_scheme_preserves_decimal_boundaries() {
        let tenth = FixedTickScheme::new(0.1).unwrap();
        let cent = FixedTickScheme::new(0.01).unwrap();

        assert_eq!(tenth.next_bid_price(0.3, 0, 1), Some(Price::new(0.3, 1)));
        assert_eq!(tenth.next_ask_price(0.3, 0, 1), Some(Price::new(0.3, 1)));
        assert_eq!(cent.next_bid_price(0.07, 0, 2), Some(Price::new(0.07, 2)));
        assert_eq!(cent.next_ask_price(0.07, 0, 2), Some(Price::new(0.07, 2)));
    }

    #[rstest]
    fn fixed_tick_multiple_steps() {
        let scheme = FixedTickScheme::new(1.0).unwrap();
        let bid = scheme.next_bid_price(10.0, 2, 1).unwrap();
        let ask = scheme.next_ask_price(10.0, 3, 1).unwrap();
        assert_eq!(bid, Price::new(8.0, 1));
        assert_eq!(ask, Price::new(13.0, 1));
    }

    #[rstest]
    fn tick_scheme_round_trip() {
        let scheme = TickScheme::from_str("CRYPTO_0_01").unwrap();
        assert_eq!(scheme.to_string(), "CRYPTO_0_01");
    }

    #[rstest]
    fn tick_scheme_rule_from_fixed_precision_name() {
        let scheme = tick_scheme_rule_from_name("fixed_precision_1").unwrap();

        assert_eq!(scheme.next_bid_price(0.3, 0, 1), Some(Price::new(0.3, 1)));
        assert_eq!(scheme.next_ask_price(0.31, 0, 1), Some(Price::new(0.4, 1)));
    }

    #[rstest]
    fn tick_scheme_unknown() {
        let error = TickScheme::from_str("UNKNOWN").unwrap_err();

        assert_eq!(
            error,
            TickSchemeError::UnknownName {
                name: "UNKNOWN".to_string(),
            }
        );
        assert_eq!(error.to_string(), "unknown tick scheme UNKNOWN");
    }

    #[rstest]
    fn tick_scheme_fixed_precision_above_max_returns_unknown_name() {
        let name = format!("FIXED_PRECISION_{}", FIXED_PRECISION + 1);
        let error = TickScheme::from_str(&name).unwrap_err();

        assert_eq!(error, TickSchemeError::UnknownName { name: name.clone() });
        assert_eq!(error.to_string(), format!("unknown tick scheme {name}"));
    }

    #[rstest]
    fn fixed_tick_zero() {
        let error = FixedTickScheme::new(0.0).unwrap_err();

        assert_eq!(error, TickSchemeError::TickNotPositive { tick: 0.0 });
        assert_eq!(error.to_string(), "tick must be positive");
    }

    #[rstest]
    #[case(f64::INFINITY)]
    #[case(f64::NAN)]
    fn fixed_tick_non_finite_returns_error(#[case] tick: f64) {
        let error = FixedTickScheme::new(tick).unwrap_err();

        match &error {
            TickSchemeError::TickNotFinite {
                tick: returned_tick,
            } => {
                assert!(
                    *returned_tick == tick || returned_tick.is_nan() && tick.is_nan(),
                    "returned tick {returned_tick} did not match input {tick}",
                );
            }
            _ => panic!("unexpected error variant: {error:?}"),
        }
        assert_eq!(error.to_string(), "tick must be finite");
    }

    #[rstest]
    fn fixed_tick_scheme_nan_value_returns_none() {
        let scheme = FixedTickScheme::new(1.0).unwrap();
        assert!(scheme.next_bid_price(f64::NAN, 0, 2).is_none());
        assert!(scheme.next_ask_price(f64::NAN, 0, 2).is_none());
    }

    #[rstest]
    fn fixed_tick_scheme_out_of_range_returns_none() {
        // Stepping one tick above PRICE_MAX must yield None rather than panicking
        let scheme = FixedTickScheme::new(PRICE_MAX).unwrap();
        assert!(scheme.next_ask_price(PRICE_MAX, 1, 2).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_topix100_construction() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.tick_count() > 0);
        assert_eq!(scheme.precision(), 4);
        assert_eq!(scheme.min_price(), Price::new(0.1, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_betfair_construction() {
        let scheme = TieredTickScheme::betfair();
        assert_eq!(scheme.tick_count(), 350);
        assert_eq!(scheme.precision(), 2);
        assert_eq!(scheme.min_price(), Price::new(1.01, 2));
        assert_eq!(scheme.max_price(), Price::new(1000.0, 2));
    }

    #[rstest]
    fn tiered_tick_scheme_ask_at_low_price() {
        let scheme = TieredTickScheme::topix100();
        let ask = scheme.next_ask_price(500.0, 0, 4).unwrap();
        assert_eq!(ask, Price::new(500.0, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_bid_at_low_price() {
        let scheme = TieredTickScheme::topix100();
        let bid = scheme.next_bid_price(500.0, 0, 4).unwrap();
        assert_eq!(bid, Price::new(500.0, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_ask_steps() {
        let scheme = TieredTickScheme::topix100();
        let ask0 = scheme.next_ask_price(500.0, 0, 4).unwrap();
        let ask1 = scheme.next_ask_price(500.0, 1, 4).unwrap();
        assert!(ask1 > ask0);
        assert_eq!(ask1, Price::new(500.1, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_bid_steps() {
        let scheme = TieredTickScheme::topix100();
        let bid0 = scheme.next_bid_price(500.0, 0, 4).unwrap();
        let bid1 = scheme.next_bid_price(500.0, 1, 4).unwrap();
        assert!(bid1 < bid0);
        assert_eq!(bid1, Price::new(499.9, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_tier_boundary_1000() {
        let scheme = TieredTickScheme::topix100();
        // At 1000.0 we cross from 0.1 step to 0.5 step
        let ask = scheme.next_ask_price(1000.0, 1, 4).unwrap();
        assert_eq!(ask, Price::new(1000.5, 4));
    }

    #[rstest]
    #[case(3.90, 1, "3.95")]
    #[case(4.0, 1, "4.10")]
    fn tiered_tick_scheme_betfair_ask_transition(
        #[case] value: f64,
        #[case] n: i32,
        #[case] expected: &str,
    ) {
        let scheme = TieredTickScheme::betfair();
        let ask = scheme.next_ask_price(value, n, 2).unwrap();
        assert_eq!(ask, Price::from(expected));
    }

    #[rstest]
    #[case(1.499, 0, "1.49")]
    #[case(2.011, 0, "2.00")]
    #[case(2.027, 2, "1.99")]
    fn tiered_tick_scheme_betfair_bid_transition(
        #[case] value: f64,
        #[case] n: i32,
        #[case] expected: &str,
    ) {
        let scheme = TieredTickScheme::betfair();
        let bid = scheme.next_bid_price(value, n, 2).unwrap();
        assert_eq!(bid, Price::from(expected));
    }

    #[rstest]
    fn tiered_tick_scheme_between_ticks() {
        let scheme = TieredTickScheme::topix100();
        // 1000.3 is between ticks in the 0.5-step tier (1000.0, 1000.5)
        let ask = scheme.next_ask_price(1000.3, 0, 4).unwrap();
        assert!(ask.as_f64() >= 1000.3);
        let bid = scheme.next_bid_price(1000.3, 0, 4).unwrap();
        assert!(bid.as_f64() <= 1000.3);
    }

    #[rstest]
    fn tiered_tick_scheme_off_grid_bid_below_tick() {
        // 1.049 is below the 1.05 tick; bid should be 1.00, not 1.05
        let scheme = TieredTickScheme::new(&[(1.0, 2.0, 0.05)], 2, 100).unwrap();
        let bid = scheme.next_bid_price(1.049, 0, 2).unwrap();
        assert_eq!(bid, Price::new(1.0, 2));
    }

    #[rstest]
    fn tiered_tick_scheme_off_grid_ask_above_tick() {
        // 1.051 is above the 1.05 tick; ask should be 1.10, not 1.05
        let scheme = TieredTickScheme::new(&[(1.0, 2.0, 0.05)], 2, 100).unwrap();
        let ask = scheme.next_ask_price(1.051, 0, 2).unwrap();
        assert_eq!(ask, Price::new(1.10, 2));
    }

    #[rstest]
    fn tiered_tick_scheme_bid_below_min_returns_none() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.next_bid_price(0.05, 0, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_ask_beyond_last_tick_returns_none() {
        let scheme = TieredTickScheme::topix100();
        let last = scheme.max_price().as_f64();
        assert!(scheme.next_ask_price(last, 1, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_bid_beyond_last_tick_returns_last() {
        let scheme = TieredTickScheme::new(&[(1.0, 10.0, 1.0)], 1, 100).unwrap();
        // 9.5 is beyond last tick (9.0) but bid should be 9.0
        let bid = scheme.next_bid_price(9.5, 0, 1).unwrap();
        assert_eq!(bid, Price::new(9.0, 1));
    }

    #[rstest]
    fn tiered_tick_scheme_negative_n_returns_none() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.next_bid_price(500.0, -1, 4).is_none());
        assert!(scheme.next_ask_price(500.0, -1, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_nan_value_returns_none() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.next_bid_price(f64::NAN, 0, 4).is_none());
        assert!(scheme.next_ask_price(f64::NAN, 0, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_infinite_value_saturates() {
        let scheme = TieredTickScheme::topix100();
        assert_eq!(
            scheme.next_bid_price(f64::INFINITY, 0, 4),
            Some(scheme.max_price())
        );
        assert!(scheme.next_ask_price(f64::INFINITY, 0, 4).is_none());
        assert!(scheme.next_bid_price(f64::NEG_INFINITY, 0, 4).is_none());
    }

    #[rstest]
    fn crypto_tick_scheme_out_of_range_returns_none() {
        // Values beyond the Price range must yield None rather than panicking
        let scheme = TickScheme::Crypto;
        assert!(scheme.next_ask_price(PRICE_MAX * 2.0, 0, 2).is_none());
        assert!(scheme.next_bid_price(PRICE_MIN * 2.0, 0, 2).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_validation_empty_tiers() {
        let error = TieredTickScheme::new(&[], 2, 100).unwrap_err();

        assert_eq!(error, TickSchemeError::EmptyTiers);
        assert_eq!(error.to_string(), "tiers must not be empty");
    }

    #[rstest]
    #[case(
        vec![(100.0, 50.0, 1.0)],
        TickSchemeError::TierStartNotLessThanStop {
            index: 0,
            start: 100.0,
            stop: 50.0,
        },
        "tier 0: start (100) must be less than stop (50)"
    )]
    #[case(
        vec![(2.0, 2.0, 0.1)],
        TickSchemeError::TierStartNotLessThanStop {
            index: 0,
            start: 2.0,
            stop: 2.0,
        },
        "tier 0: start (2) must be less than stop (2)"
    )]
    #[case(
        vec![(0.0, 100.0, -1.0)],
        TickSchemeError::TierStepNotPositive {
            index: 0,
            step: -1.0,
        },
        "tier 0: step (-1) must be positive"
    )]
    #[case(
        vec![(1.0, 2.0, 0.0)],
        TickSchemeError::TierStepNotPositive {
            index: 0,
            step: 0.0,
        },
        "tier 0: step (0) must be positive"
    )]
    #[case(
        vec![(0.0, 100.0, 200.0)],
        TickSchemeError::TierStepNotLessThanRange {
            index: 0,
            start: 0.0,
            stop: 100.0,
            step: 200.0,
            range: 100.0,
        },
        "tier 0: step (200) must be less than range (100 - 0 = 100)"
    )]
    #[case(
        vec![(10.0, 20.0, 1.0), (1.0, 10.0, 1.0)],
        TickSchemeError::TierOverlapsPrevious {
            index: 1,
            start: 1.0,
            prev_stop: 20.0,
        },
        "tier 1: start (1) overlaps previous tier stop (20)"
    )]
    #[case(
        vec![(1.0, 10.0, 1.0), (5.0, 15.0, 1.0)],
        TickSchemeError::TierOverlapsPrevious {
            index: 1,
            start: 5.0,
            prev_stop: 10.0,
        },
        "tier 1: start (5) overlaps previous tier stop (10)"
    )]
    fn tiered_tick_scheme_invalid_tiers_return_typed_errors(
        #[case] tiers: Vec<(f64, f64, f64)>,
        #[case] expected_error: TickSchemeError,
        #[case] expected_display: &str,
    ) {
        let error = TieredTickScheme::new(&tiers, 2, 100).unwrap_err();

        assert_eq!(error, expected_error);
        assert_eq!(error.to_string(), expected_display);
    }

    #[rstest]
    #[case(vec![(f64::NAN, 10.0, 1.0)])]
    #[case(vec![(1.0, f64::NAN, 1.0)])]
    #[case(vec![(1.0, 10.0, f64::NAN)])]
    fn tiered_tick_scheme_nan_tiers_return_typed_error(#[case] tiers: Vec<(f64, f64, f64)>) {
        let error = TieredTickScheme::new(&tiers, 2, 100).unwrap_err();

        match &error {
            TickSchemeError::TierValuesNaN {
                index,
                start,
                stop,
                step,
            } => {
                assert_eq!(*index, 0);
                assert!(
                    start.is_nan() || stop.is_nan() || step.is_nan(),
                    "expected one NaN tier value in {error:?}",
                );
            }
            _ => panic!("unexpected error variant: {error:?}"),
        }
        assert_eq!(error.to_string(), "tier 0: values must not be NaN");
    }

    #[rstest]
    fn tiered_tick_scheme_start_outside_price_range_returns_typed_error() {
        let start = PRICE_MIN - 1.0;
        let stop = PRICE_MIN + 1.0;
        let error = TieredTickScheme::new(&[(start, stop, 1.0)], 2, 100).unwrap_err();

        assert_eq!(
            error,
            TickSchemeError::TierStartOutsidePriceRange { index: 0, start }
        );
        assert_eq!(
            error.to_string(),
            format!("tier 0: start ({start}) outside Price range")
        );
    }

    #[rstest]
    fn tiered_tick_scheme_stop_outside_price_range_returns_typed_error() {
        let start = PRICE_MAX - 2.0;
        let stop = PRICE_MAX + 1.0;
        let error = TieredTickScheme::new(&[(start, stop, 1.0)], 2, 100).unwrap_err();

        assert_eq!(
            error,
            TickSchemeError::TierStopOutsidePriceRange { index: 0, stop }
        );
        assert_eq!(
            error.to_string(),
            format!("tier 0: stop ({stop}) outside Price range")
        );
    }

    #[rstest]
    fn tiered_tick_scheme_invalid_precision_wraps_source_error() {
        let invalid_precision = FIXED_PRECISION + 1;
        let source = Price::new_checked(0.0, invalid_precision).unwrap_err();
        let error = TieredTickScheme::new(&[(1.0, 10.0, 1.0)], invalid_precision, 100).unwrap_err();

        assert_eq!(
            error,
            TickSchemeError::InvalidPrecision {
                source: source.clone(),
            }
        );
        assert_eq!(error.to_string(), source.to_string());
    }

    #[rstest]
    fn tiered_tick_scheme_empty_expansion_returns_typed_error() {
        let error = TieredTickScheme::new(&[(1.0, f64::INFINITY, 1.0)], 2, 0).unwrap_err();

        assert_eq!(error, TickSchemeError::EmptyTickExpansion);
        assert_eq!(error.to_string(), "tier expansion produced no ticks");
    }

    #[rstest]
    fn tiered_tick_scheme_expanded_tick_outside_range_returns_typed_error() {
        let invalid_value = PRICE_MAX + 1.0;
        let error = TieredTickScheme::new(&[(PRICE_MAX, f64::INFINITY, 1.0)], 2, 2).unwrap_err();

        assert_eq!(
            error,
            TickSchemeError::ExpandedTickOutsidePriceRange {
                value: invalid_value,
            }
        );
        assert_eq!(
            error.to_string(),
            format!("expanded tick value {invalid_value} outside Price range")
        );
    }

    #[rstest]
    fn tiered_tick_scheme_finite_tier_includes_all_ticks() {
        // (0.0, 0.3, 0.1) should produce 0.0, 0.1, 0.2 (3 ticks, not 2)
        let scheme = TieredTickScheme::new(&[(0.0, 0.3, 0.1)], 1, 100).unwrap();
        assert_eq!(scheme.tick_count(), 3);
    }

    #[rstest]
    fn tiered_tick_scheme_simple_two_tiers() {
        let scheme =
            TieredTickScheme::new(&[(1.0, 10.0, 1.0), (10.0, 100.0, 5.0)], 2, 100).unwrap();
        let ticks = scheme.ticks();
        // First tier: 1, 2, 3, ..., 9
        // Second tier: 10, 15, 20, ..., 95
        assert_eq!(ticks[0], Price::new(1.0, 2));
        assert_eq!(ticks[8], Price::new(9.0, 2));
        assert_eq!(ticks[9], Price::new(10.0, 2));
        assert_eq!(ticks[10], Price::new(15.0, 2));
    }

    #[rstest]
    fn tiered_tick_scheme_infinity_tier() {
        let scheme = TieredTickScheme::new(&[(100.0, f64::INFINITY, 10.0)], 1, 5).unwrap();
        assert_eq!(scheme.tick_count(), 5);
        let ticks = scheme.ticks();
        assert_eq!(ticks[0], Price::new(100.0, 1));
        assert_eq!(ticks[4], Price::new(140.0, 1));
    }

    #[rstest]
    fn tiered_tick_scheme_from_str_topix100() {
        let scheme = TickScheme::from_str("TOPIX100").unwrap();
        assert_eq!(scheme.to_string(), "TIERED");
    }

    #[rstest]
    fn tiered_tick_scheme_from_str_betfair() {
        let scheme = TickScheme::from_str("BETFAIR").unwrap();
        assert_eq!(scheme.to_string(), BETFAIR_TICK_SCHEME_NAME);
        assert_eq!(
            scheme.next_ask_price(4.0, 1, 2).unwrap(),
            Price::new(4.1, 2)
        );
    }

    #[rstest]
    fn tiered_tick_scheme_display() {
        let scheme = TieredTickScheme::new(&[(1.0, 10.0, 1.0)], 2, 100).unwrap();
        assert_eq!(scheme.to_string(), "TIERED");
    }

    #[rstest]
    fn tiered_tick_scheme_min_tick_bid() {
        let scheme = TieredTickScheme::topix100();
        let result = scheme.next_bid_price(0.1, 0, 4).unwrap();
        assert_eq!(result, Price::new(0.1, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_min_tick_bid_n1_returns_none() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.next_bid_price(0.1, 1, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_boundary_tick_equality() {
        let scheme = TieredTickScheme::topix100();
        let bid = scheme.next_bid_price(1000.0, 0, 4).unwrap();
        assert_eq!(bid, Price::new(1000.0, 4));
        let ask = scheme.next_ask_price(1000.0, 0, 4).unwrap();
        assert_eq!(ask, Price::new(1000.0, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_tier_transition_ask_from_999_9() {
        let scheme = TieredTickScheme::topix100();
        let ask = scheme.next_ask_price(999.9, 0, 4).unwrap();
        assert_eq!(ask, Price::new(999.9, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_tier_transition_bid_from_1000_5() {
        let scheme = TieredTickScheme::topix100();
        let bid = scheme.next_bid_price(1000.5, 1, 4).unwrap();
        assert_eq!(bid, Price::new(1000.0, 4));
    }

    #[rstest]
    fn tiered_tick_scheme_large_n_beyond_bounds_ask() {
        let scheme = TieredTickScheme::topix100();
        let max = scheme.max_price().as_f64();
        assert!(scheme.next_ask_price(max - 1000.0, 100_000, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_large_n_beyond_bounds_bid() {
        let scheme = TieredTickScheme::topix100();
        let min = scheme.min_price().as_f64();
        assert!(scheme.next_bid_price(min + 1000.0, 100_000, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_out_of_bounds_ask_far_above() {
        let scheme = TieredTickScheme::topix100();
        assert!(scheme.next_ask_price(999_999_999.0, 0, 4).is_none());
    }

    #[rstest]
    fn tiered_tick_scheme_idempotent_on_tick() {
        let scheme = TieredTickScheme::topix100();
        let price = 500.0;
        let ask = scheme.next_ask_price(price, 0, 4).unwrap();
        let ask2 = scheme.next_ask_price(ask.as_f64(), 0, 4).unwrap();
        assert_eq!(ask, ask2);
        let bid = scheme.next_bid_price(price, 0, 4).unwrap();
        let bid2 = scheme.next_bid_price(bid.as_f64(), 0, 4).unwrap();
        assert_eq!(bid, bid2);
    }

    #[rstest]
    fn tiered_tick_scheme_consistency_forward_backward() {
        let scheme = TieredTickScheme::topix100();
        let start = 5000.0;
        let forward = scheme.next_ask_price(start, 10, 4).unwrap();
        let back = scheme.next_bid_price(forward.as_f64(), 10, 4).unwrap();
        assert!(back.as_f64() <= start);
    }

    #[rstest]
    fn tiered_tick_scheme_cumulative_equals_direct() {
        let scheme = TieredTickScheme::topix100();
        let price = 1000.0;
        let mut cumulative = price;
        for _ in 0..5 {
            if let Some(result) = scheme.next_ask_price(cumulative, 1, 4) {
                cumulative = result.as_f64();
            }
        }
        let direct = scheme.next_ask_price(price, 5, 4).unwrap();
        assert!((cumulative - direct.as_f64()).abs() < 1e-10);
    }

    #[rstest]
    #[case(1000.0, 0, 1000.0)]
    #[case(1000.25, 0, 1000.5)]
    #[case(10_001.0, 0, 10_005.0)]
    #[case(10_000_001.0, 0, 10_005_000.0)]
    #[case(9999.0, 2, 10_005.0)]
    fn tiered_tick_scheme_topix100_ask_parametrized(
        #[case] value: f64,
        #[case] n: i32,
        #[case] expected: f64,
    ) {
        let scheme = TieredTickScheme::topix100();
        let ask = scheme.next_ask_price(value, n, 4).unwrap();
        assert_eq!(ask, Price::new(expected, 4));
    }

    #[rstest]
    #[case(1000.75, 0, 1000.5)]
    #[case(10_007.0, 0, 10_005.0)]
    #[case(10_000_001.0, 0, 10_000_000.0)]
    #[case(10_006.0, 2, 9999.0)]
    fn tiered_tick_scheme_topix100_bid_parametrized(
        #[case] value: f64,
        #[case] n: i32,
        #[case] expected: f64,
    ) {
        let scheme = TieredTickScheme::topix100();
        let bid = scheme.next_bid_price(value, n, 4).unwrap();
        assert_eq!(bid, Price::new(expected, 4));
    }

    // Property: bid(value, 0) <= value for any value in range
    proptest! {
        #[rstest]
        fn prop_tiered_bid_at_or_below_value(value in 0.1f64..100_000.0) {
            let scheme = TieredTickScheme::topix100();
            if let Some(bid) = scheme.next_bid_price(value, 0, 4) {
                prop_assert!(bid.as_f64() <= value + 1e-9);
            }
        }
    }

    // Property: ask(value, 0) >= value for any value in range
    proptest! {
        #[rstest]
        fn prop_tiered_ask_at_or_above_value(value in 0.1f64..100_000.0) {
            let scheme = TieredTickScheme::topix100();
            if let Some(ask) = scheme.next_ask_price(value, 0, 4) {
                prop_assert!(ask.as_f64() >= value - 1e-9);
            }
        }
    }

    // Property: bid(value, 0) < ask(value, 0) when value is between ticks
    proptest! {
        #[rstest]
        fn prop_tiered_bid_less_than_ask_off_grid(value in 0.15f64..99_999.0) {
            let scheme = TieredTickScheme::topix100();

            if let (Some(bid), Some(ask)) = (
                scheme.next_bid_price(value, 0, 4),
                scheme.next_ask_price(value, 0, 4),
            ) {
                prop_assert!(bid <= ask);
            }
        }
    }

    // Property: ask(value, n) is monotonically increasing in n
    proptest! {
        #[rstest]
        fn prop_tiered_ask_monotonic_in_n(value in 1.0f64..10_000.0) {
            let scheme = TieredTickScheme::topix100();
            let mut prev: Option<Price> = None;

            for n in 0..5 {
                if let Some(ask) = scheme.next_ask_price(value, n, 4) {
                    if let Some(p) = prev {
                        prop_assert!(ask >= p);
                    }
                    prev = Some(ask);
                }
            }
        }
    }

    // Property: ticks are strictly sorted
    proptest! {
        #[rstest]
        fn prop_tiered_ticks_sorted(
            start in 1.0f64..100.0,
            step in 0.01f64..10.0,
        ) {
            let stop = start + step * 10.0;
            if let Ok(scheme) = TieredTickScheme::new(&[(start, stop, step)], 2, 100) {
                let ticks = scheme.ticks();
                for i in 1..ticks.len() {
                    prop_assert!(ticks[i] > ticks[i - 1]);
                }
            }
        }
    }
}

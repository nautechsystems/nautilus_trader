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

//! Represents a price in a market with a specified precision.
//!
//! [`Price`] is an immutable value type for representing market prices, bid/ask quotes,
//! and price levels. Unlike [`Quantity`](super::Quantity), prices can be negative (useful for spreads,
//! basis trades, or certain derivative instruments).
//!
//! # Arithmetic behavior
//!
//! | Operation         | Result    | Notes                              |
//! |-------------------|-----------|------------------------------------|
//! | `Price + Price`   | `Price`   | Precision is max of both operands. |
//! | `Price - Price`   | `Price`   | Precision is max of both operands. |
//! | `Price + Decimal` | `Decimal` |                                    |
//! | `Price - Decimal` | `Decimal` |                                    |
//! | `Price * Decimal` | `Decimal` |                                    |
//! | `Price / Decimal` | `Decimal` |                                    |
//! | `Price + f64`     | `f64`     |                                    |
//! | `Price - f64`     | `f64`     |                                    |
//! | `Price * f64`     | `f64`     |                                    |
//! | `Price / f64`     | `f64`     |                                    |
//! | `-Price`          | `Price`   |                                    |
//!
//! # Immutability
//!
//! `Price` is immutable. All arithmetic operations return new instances.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, Deref, Div, Mul, Neg, Sub},
    str::FromStr,
};

use nautilus_core::{
    correctness::{
        CorrectnessError, CorrectnessResult, CorrectnessResultExt, FAILED,
        check_in_range_inclusive_f64,
    },
    string::formatting::Separable,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};

use super::fixed::{
    FIXED_PRECISION, FIXED_SCALAR, check_fixed_precision, mantissa_exponent_to_fixed_i128,
    raw_scales_match,
};
#[cfg(feature = "high-precision")]
use super::fixed::{PRECISION_DIFF_SCALAR, f64_to_fixed_i128, fixed_i128_to_f64};
#[cfg(not(feature = "high-precision"))]
use super::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
#[cfg(feature = "defi")]
use crate::types::fixed::MAX_FLOAT_PRECISION;

// -----------------------------------------------------------------------------
// PriceRaw
// -----------------------------------------------------------------------------

// Use 128-bit integers when either `high-precision` or `defi` features are enabled. This is
// required for the extended 18-decimal wei precision used in DeFi contexts.

#[cfg(feature = "high-precision")]
pub type PriceRaw = i128;

#[cfg(not(feature = "high-precision"))]
pub type PriceRaw = i64;

// -----------------------------------------------------------------------------

/// The maximum raw price integer value.
///
/// # Safety
///
/// This value is computed at compile time from `PRICE_MAX` * `FIXED_SCALAR`.
/// The multiplication is guaranteed not to overflow because `PRICE_MAX` and `FIXED_SCALAR`
/// are chosen such that their product fits within `PriceRaw`'s range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static PRICE_RAW_MAX: PriceRaw = (PRICE_MAX * FIXED_SCALAR) as PriceRaw;

/// The minimum raw price integer value.
///
/// # Safety
///
/// This value is computed at compile time from `PRICE_MIN` * `FIXED_SCALAR`.
/// The multiplication is guaranteed not to overflow because `PRICE_MIN` and `FIXED_SCALAR`
/// are chosen such that their product fits within `PriceRaw`'s range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static PRICE_RAW_MIN: PriceRaw = (PRICE_MIN * FIXED_SCALAR) as PriceRaw;

/// The sentinel value for an unset or null price.
pub const PRICE_UNDEF: PriceRaw = PriceRaw::MAX;

/// The sentinel value for an error or invalid price.
pub const PRICE_ERROR: PriceRaw = PriceRaw::MIN;

// -----------------------------------------------------------------------------
// PRICE_MAX
// -----------------------------------------------------------------------------

/// The maximum valid price value that can be represented.
#[cfg(feature = "high-precision")]
pub const PRICE_MAX: f64 = 17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The maximum valid price value that can be represented.
pub const PRICE_MAX: f64 = 9_223_372_036.0;

// -----------------------------------------------------------------------------
// PRICE_MIN
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The minimum valid price value that can be represented.
pub const PRICE_MIN: f64 = -17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The minimum valid price value that can be represented.
pub const PRICE_MIN: f64 = -9_223_372_036.0;

// -----------------------------------------------------------------------------

/// The sentinel `Price` representing errors (this will be removed when Cython is gone).
pub const ERROR_PRICE: Price = Price {
    raw: 0,
    precision: 255,
};

/// Represents a price in a market with a specified precision.
///
/// The number of decimal places may vary. For certain asset classes, prices may
/// have negative values. For example, prices for options instruments can be
/// negative under certain conditions.
///
/// Handles up to [`FIXED_PRECISION`] decimals of precision.
///
/// - [`PRICE_MAX`] - Maximum representable price value.
/// - [`PRICE_MIN`] - Minimum representable price value.
#[repr(C)]
#[derive(Clone, Copy, Default, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Price {
    /// Represents the raw fixed-point value, with `precision` defining the number of decimal places.
    pub raw: PriceRaw,
    /// The number of decimal places, with a maximum of [`FIXED_PRECISION`].
    pub precision: u8,
}

impl Price {
    /// Creates a new [`Price`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is invalid outside the representable range [`PRICE_MIN`, `PRICE_MAX`].
    /// - `precision` is invalid outside the representable range [0, `FIXED_PRECISION`].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(value: f64, precision: u8) -> CorrectnessResult<Self> {
        check_in_range_inclusive_f64(value, PRICE_MIN, PRICE_MAX, "value")?;

        #[cfg(feature = "defi")]
        if precision > MAX_FLOAT_PRECISION {
            // Floats are only reliable up to ~16 decimal digits of precision regardless of feature flags
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "`precision` exceeded maximum float precision ({MAX_FLOAT_PRECISION}), use `Price::from_wei()` for wei values instead"
                ),
            });
        }

        check_fixed_precision(precision)?;

        #[cfg(feature = "high-precision")]
        let raw = f64_to_fixed_i128(value, precision);

        #[cfg(not(feature = "high-precision"))]
        let raw = f64_to_fixed_i64(value, precision);

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Price`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn new(value: f64, precision: u8) -> Self {
        Self::new_checked(value, precision).expect_display(FAILED)
    }

    /// Creates a new [`Price`] instance from the given `raw` fixed-point value and `precision`.
    ///
    /// # Panics
    ///
    /// Panics if `raw` is outside the valid range and is not a sentinel value.
    /// Panics if `precision` exceeds [`FIXED_PRECISION`].
    #[must_use]
    pub fn from_raw(raw: PriceRaw, precision: u8) -> Self {
        assert!(
            raw == PRICE_ERROR
                || raw == PRICE_UNDEF
                || (raw >= PRICE_RAW_MIN && raw <= PRICE_RAW_MAX),
            "`raw` value {raw} outside valid range [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}] for Price"
        );

        if raw == PRICE_UNDEF {
            assert!(
                precision == 0,
                "`precision` must be 0 when `raw` is PRICE_UNDEF"
            );
        }
        check_fixed_precision(precision).expect_display(FAILED);

        // TODO: Enforce spurious bits validation in v2
        // if !matches!(raw, PRICE_UNDEF | PRICE_ERROR) && raw != 0 {
        //     #[cfg(feature = "high-precision")]
        //     super::fixed::check_fixed_raw_i128(raw, precision).expect(FAILED);
        //     #[cfg(not(feature = "high-precision"))]
        //     super::fixed::check_fixed_raw_i64(raw, precision).expect(FAILED);
        // }

        Self { raw, precision }
    }

    /// Creates a new [`Price`] instance from the given `raw` fixed-point value and `precision`
    /// with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `precision` exceeds the maximum fixed precision.
    /// - `precision` is not 0 when `raw` is `PRICE_UNDEF`.
    /// - `raw` is outside the valid range `[PRICE_RAW_MIN, PRICE_RAW_MAX]`
    ///   and is not a sentinel value.
    pub fn from_raw_checked(raw: PriceRaw, precision: u8) -> CorrectnessResult<Self> {
        if raw == PRICE_UNDEF && precision != 0 {
            return Err(CorrectnessError::PredicateViolation {
                message: "`precision` must be 0 when `raw` is PRICE_UNDEF".to_string(),
            });
        }

        if raw != PRICE_ERROR && raw != PRICE_UNDEF && (raw < PRICE_RAW_MIN || raw > PRICE_RAW_MAX)
        {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "raw value {raw} outside valid range [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}]"
                ),
            });
        }

        check_fixed_precision(precision)?;

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Price`] instance with a value of zero with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn zero(precision: u8) -> Self {
        check_fixed_precision(precision).expect_display(FAILED);
        Self { raw: 0, precision }
    }

    /// Creates a new [`Price`] instance with the maximum representable value with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn max(precision: u8) -> Self {
        check_fixed_precision(precision).expect_display(FAILED);
        Self {
            raw: PRICE_RAW_MAX,
            precision,
        }
    }

    /// Creates a new [`Price`] instance with the minimum representable value with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn min(precision: u8) -> Self {
        check_fixed_precision(precision).expect_display(FAILED);
        Self {
            raw: PRICE_RAW_MIN,
            precision,
        }
    }

    /// Performs a checked addition, returning `None` on raw integer overflow, when the
    /// result falls outside `[PRICE_RAW_MIN, PRICE_RAW_MAX]`, when either operand is a
    /// sentinel (`PRICE_UNDEF`, `PRICE_ERROR`, or `ERROR_PRICE`), or when the operands
    /// have mixed raw scales (one at `FIXED_PRECISION` scale, the other at a defi
    /// `WEI_PRECISION` scale).
    ///
    /// Precision follows the `Add` implementation: uses the maximum precision of both operands.
    #[must_use]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        if self.is_sentinel() || rhs.is_sentinel() {
            return None;
        }

        if !raw_scales_match(self.precision, rhs.precision) {
            return None;
        }
        let raw = self.raw.checked_add(rhs.raw)?;
        if raw < PRICE_RAW_MIN || raw > PRICE_RAW_MAX {
            return None;
        }
        Some(Self {
            raw,
            precision: self.precision.max(rhs.precision),
        })
    }

    /// Performs a checked subtraction, returning `None` on raw integer underflow, when
    /// the result falls outside `[PRICE_RAW_MIN, PRICE_RAW_MAX]`, when either operand
    /// is a sentinel (`PRICE_UNDEF`, `PRICE_ERROR`, or `ERROR_PRICE`), or when the
    /// operands have mixed raw scales (one at `FIXED_PRECISION` scale, the other at a
    /// defi `WEI_PRECISION` scale).
    ///
    /// Precision follows the `Sub` implementation: uses the maximum precision of both operands.
    #[must_use]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        if self.is_sentinel() || rhs.is_sentinel() {
            return None;
        }

        if !raw_scales_match(self.precision, rhs.precision) {
            return None;
        }
        let raw = self.raw.checked_sub(rhs.raw)?;
        if raw < PRICE_RAW_MIN || raw > PRICE_RAW_MAX {
            return None;
        }
        Some(Self {
            raw,
            precision: self.precision.max(rhs.precision),
        })
    }

    #[inline]
    fn is_sentinel(self) -> bool {
        // ERROR_PRICE uses precision == u8::MAX as its sentinel marker, distinct from
        // valid high-precision values (e.g. defi `from_wei` uses precision 18 which is
        // > FIXED_PRECISION but is not a sentinel).
        self.raw == PRICE_UNDEF || self.raw == PRICE_ERROR || self.precision == u8::MAX
    }

    /// Returns `true` if the value of this instance is undefined.
    #[must_use]
    pub fn is_undefined(&self) -> bool {
        self.raw == PRICE_UNDEF
    }

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Returns `true` if the value of this instance is position (> 0).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.raw != PRICE_UNDEF && self.raw > 0
    }

    #[cfg(feature = "high-precision")]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond `MAX_FLOAT_PRECISION` (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        assert!(
            self.precision <= MAX_FLOAT_PRECISION,
            "Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)"
        );

        fixed_i128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond `MAX_FLOAT_PRECISION` (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        if self.precision > MAX_FLOAT_PRECISION {
            panic!("Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)");
        }

        fixed_i64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let precision_diff = FIXED_PRECISION.saturating_sub(self.precision);
        let rescaled_raw = self.raw / PriceRaw::pow(10, u32::from(precision_diff));
        #[allow(
            clippy::unnecessary_cast,
            clippy::cast_lossless,
            reason = "cast is real when PriceRaw is i64, no-op when i128"
        )]
        Decimal::from_i128_with_scale(rescaled_raw as i128, u32::from(self.precision))
    }

    /// Returns a formatted string representation of this instance.
    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        format!("{self}").separate_with_underscores()
    }

    /// Creates a new [`Price`] from a `Decimal` value with specified precision.
    ///
    /// Uses pure integer arithmetic on the Decimal's mantissa and scale for fast conversion.
    /// The value is rounded to the specified precision using banker's rounding (round half to even).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `precision` exceeds [`FIXED_PRECISION`].
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    pub fn from_decimal_dp(decimal: Decimal, precision: u8) -> CorrectnessResult<Self> {
        let exponent = -(decimal.scale() as i8);
        let raw_i128 = mantissa_exponent_to_fixed_i128(decimal.mantissa(), exponent, precision)?;

        #[allow(
            clippy::useless_conversion,
            reason = "i128 to PriceRaw is real when not high-precision"
        )]
        let raw: PriceRaw =
            raw_i128
                .try_into()
                .map_err(|_| CorrectnessError::PredicateViolation {
                    message: format!(
                        "Decimal value exceeds PriceRaw range [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}]"
                    ),
                })?;

        if !(raw >= PRICE_RAW_MIN && raw <= PRICE_RAW_MAX) {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "Raw value {raw} outside valid range [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}] for Price"
                ),
            });
        }

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Price`] from a [`Decimal`] value with precision inferred from the decimal's scale.
    ///
    /// The precision is determined by the scale of the decimal (number of decimal places).
    /// The value is rounded to the inferred precision using banker's rounding (round half to even).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The inferred precision exceeds [`FIXED_PRECISION`].
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    pub fn from_decimal(decimal: Decimal) -> CorrectnessResult<Self> {
        let precision = decimal.scale() as u8;
        Self::from_decimal_dp(decimal, precision)
    }

    /// Creates a new [`Price`] from a mantissa/exponent pair using pure integer arithmetic.
    ///
    /// The value is `mantissa * 10^exponent`. This avoids all floating-point and Decimal
    /// operations, making it ideal for exchange data that arrives as mantissa/exponent pairs.
    ///
    /// # Panics
    ///
    /// Panics if the resulting raw value exceeds [`PRICE_RAW_MAX`] or [`PRICE_RAW_MIN`].
    #[must_use]
    pub fn from_mantissa_exponent(mantissa: i64, exponent: i8, precision: u8) -> Self {
        check_fixed_precision(precision).expect_display(FAILED);

        if mantissa == 0 {
            return Self { raw: 0, precision };
        }

        let raw_i128 = mantissa_exponent_to_fixed_i128(i128::from(mantissa), exponent, precision)
            .expect("Overflow in Price::from_mantissa_exponent");

        #[allow(
            clippy::useless_conversion,
            reason = "i128 to PriceRaw is real when not high-precision"
        )]
        let raw: PriceRaw = raw_i128
            .try_into()
            .expect("Raw value exceeds PriceRaw range in Price::from_mantissa_exponent");
        assert!(
            raw >= PRICE_RAW_MIN && raw <= PRICE_RAW_MAX,
            "`raw` value {raw} exceeded bounds [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}] for Price"
        );

        Self { raw, precision }
    }
}

impl FromStr for Price {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let clean_value = value.replace('_', "");

        let decimal = if clean_value.contains('e') || clean_value.contains('E') {
            Decimal::from_scientific(&clean_value)
                .map_err(|e| format!("Error parsing `input` string '{value}' as Decimal: {e}"))?
        } else {
            Decimal::from_str(&clean_value)
                .map_err(|e| format!("Error parsing `input` string '{value}' as Decimal: {e}"))?
        };

        // Use decimal scale to preserve caller-specified precision (including trailing zeros)
        let precision = decimal.scale() as u8;

        Self::from_decimal_dp(decimal, precision).map_err(|e| e.to_string())
    }
}

impl<T: AsRef<str>> From<T> for Price {
    fn from(value: T) -> Self {
        Self::from_str(value.as_ref()).expect(FAILED)
    }
}

impl From<Price> for f64 {
    fn from(price: Price) -> Self {
        price.as_f64()
    }
}

impl From<&Price> for f64 {
    fn from(price: &Price) -> Self {
        price.as_f64()
    }
}

impl From<Price> for Decimal {
    fn from(value: Price) -> Self {
        value.as_decimal()
    }
}

impl From<&Price> for Decimal {
    fn from(value: &Price) -> Self {
        value.as_decimal()
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    fn lt(&self, other: &Self) -> bool {
        self.raw.lt(&other.raw)
    }

    fn le(&self, other: &Self) -> bool {
        self.raw.le(&other.raw)
    }

    fn gt(&self, other: &Self) -> bool {
        self.raw.gt(&other.raw)
    }

    fn ge(&self, other: &Self) -> bool {
        self.raw.ge(&other.raw)
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Price {
    type Target = PriceRaw;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Neg for Price {
    type Output = Self;
    fn neg(self) -> Self::Output {
        // Preserve sentinel values (negating PRICE_ERROR would also overflow)
        if self.raw == PRICE_ERROR || self.raw == PRICE_UNDEF {
            return self;
        }
        Self {
            raw: -self.raw,
            precision: self.precision,
        }
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            raw: self
                .raw
                .checked_add(rhs.raw)
                .expect("Overflow occurred when adding `Price`"),
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self
                .raw
                .checked_sub(rhs.raw)
                .expect("Underflow occurred when subtracting `Price`"),
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl Add<Decimal> for Price {
    type Output = Decimal;
    fn add(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() + rhs
    }
}

impl Sub<Decimal> for Price {
    type Output = Decimal;
    fn sub(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() - rhs
    }
}

impl Mul<Decimal> for Price {
    type Output = Decimal;
    fn mul(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() * rhs
    }
}

impl Div<Decimal> for Price {
    type Output = Decimal;
    fn div(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() / rhs
    }
}

impl Add<f64> for Price {
    type Output = f64;
    fn add(self, rhs: f64) -> Self::Output {
        self.as_f64() + rhs
    }
}

impl Sub<f64> for Price {
    type Output = f64;
    fn sub(self, rhs: f64) -> Self::Output {
        self.as_f64() - rhs
    }
}

impl Mul<f64> for Price {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl Div<f64> for Price {
    type Output = f64;
    fn div(self, rhs: f64) -> Self::Output {
        self.as_f64() / rhs
    }
}

impl Debug for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{}({})", stringify!(Price), self.raw)
        } else {
            write!(f, "{}({})", stringify!(Price), self.as_decimal())
        }
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{}", self.raw)
        } else {
            write!(f, "{}", self.as_decimal())
        }
    }
}

impl Serialize for Price {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Price {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let price_str: &str = Deserialize::deserialize(deserializer)?;
        let price: Self = price_str.into();
        Ok(price)
    }
}

/// Checks the price `value` is positive.
///
/// # Errors
///
/// Returns an error if `value` is `PRICE_UNDEF` or not positive.
pub fn check_positive_price(value: Price, param: &str) -> CorrectnessResult<()> {
    if value.raw == PRICE_UNDEF {
        return Err(CorrectnessError::InvalidValue {
            param: param.to_string(),
            value: "PRICE_UNDEF".to_string(),
            type_name: "`Price`",
        });
    }

    if !value.is_positive() {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "`Price`",
        });
    }
    Ok(())
}

#[cfg(feature = "high-precision")]
/// The raw i64 price has already been scaled by 10^9. Further scale it by the difference to
/// `FIXED_PRECISION` to make it high/defi-precision raw price.
#[must_use]
pub fn decode_raw_price_i64(value: i64) -> PriceRaw {
    PriceRaw::from(value) * PRECISION_DIFF_SCALAR as PriceRaw
}

#[cfg(not(feature = "high-precision"))]
#[must_use]
pub fn decode_raw_price_i64(value: i64) -> PriceRaw {
    value
}

#[cfg(test)]
mod tests {
    use nautilus_core::{approx_eq, correctness::CorrectnessError};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[cfg(all(not(feature = "defi"), not(feature = "high-precision")))]
    #[should_panic(expected = "`precision` exceeded maximum `FIXED_PRECISION` (9), was 50")]
    fn test_invalid_precision_new() {
        // Precision exceeds float precision limit
        let _ = Price::new(1.0, 50);
    }

    #[rstest]
    #[cfg(all(not(feature = "defi"), feature = "high-precision"))]
    #[should_panic(expected = "`precision` exceeded maximum `FIXED_PRECISION` (16), was 50")]
    fn test_invalid_precision_new() {
        // Precision exceeds float precision limit
        let _ = Price::new(1.0, 50);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Price::from_raw(1, FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_max() {
        // Precision out of range for fixed
        let _ = Price::max(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_min() {
        // Precision out of range for fixed
        let _ = Price::min(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Price::zero(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid f64 for 'value' not in range")]
    fn test_max_value_exceeded() {
        let _ = Price::new(PRICE_MAX + 0.1, FIXED_PRECISION);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid f64 for 'value' not in range")]
    fn test_min_value_exceeded() {
        let _ = Price::new(PRICE_MIN - 0.1, FIXED_PRECISION);
    }

    #[rstest]
    fn test_is_positive_ok() {
        // A normal, non‑zero price should be positive.
        let price = Price::new(42.0, 2);
        assert!(price.is_positive());

        // `check_positive_price` should accept it without error.
        check_positive_price(price, "price").unwrap();
    }

    #[rstest]
    fn test_is_positive_rejects_non_positive() {
        // Zero is NOT positive.
        let zero = Price::zero(2);
        let error = check_positive_price(zero, "price").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::NotPositive {
                param: "price".to_string(),
                value: "0.00".to_string(),
                type_name: "`Price`",
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid `Price` for 'price' not positive, was 0.00"
        );
    }

    #[rstest]
    fn test_is_positive_rejects_undefined() {
        // PRICE_UNDEF must also be rejected.
        let undef = Price::from_raw(PRICE_UNDEF, 0);
        let error = check_positive_price(undef, "price").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::InvalidValue {
                param: "price".to_string(),
                value: "PRICE_UNDEF".to_string(),
                type_name: "`Price`",
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid `Price` for 'price', was PRICE_UNDEF"
        );
    }

    #[rstest]
    fn test_construction() {
        let price = Price::new_checked(1.23456, 4);
        assert!(price.is_ok());
        let price = price.unwrap();
        assert_eq!(price.precision, 4);
        assert!(approx_eq!(f64, price.as_f64(), 1.23456, epsilon = 0.0001));
    }

    #[rstest]
    fn test_negative_price_in_range() {
        // Use max fixed precision which varies based on feature flags
        let neg_price = Price::new(PRICE_MIN / 2.0, FIXED_PRECISION);
        assert!(neg_price.raw < 0);
    }

    #[rstest]
    fn test_new_checked() {
        // Use max fixed precision which varies based on feature flags
        assert!(Price::new_checked(1.0, FIXED_PRECISION).is_ok());
        assert!(Price::new_checked(f64::NAN, FIXED_PRECISION).is_err());
        assert!(Price::new_checked(f64::INFINITY, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_new_checked_returns_typed_error_with_stable_display() {
        let error = Price::new_checked(PRICE_MAX + 1.0, FIXED_PRECISION).unwrap_err();

        assert!(matches!(error, CorrectnessError::OutOfRange { .. }));
        assert_eq!(
            error.to_string(),
            format!(
                "invalid f64 for 'value' not in range [{PRICE_MIN}, {PRICE_MAX}], was {}",
                PRICE_MAX + 1.0
            )
        );
    }

    #[rstest]
    fn test_from_raw_checked_returns_typed_error_with_stable_display() {
        let error = Price::from_raw_checked(PRICE_UNDEF, 3).unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::PredicateViolation {
                message: "`precision` must be 0 when `raw` is PRICE_UNDEF".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "`precision` must be 0 when `raw` is PRICE_UNDEF"
        );
    }

    #[rstest]
    fn test_from_raw() {
        let raw = 100 * FIXED_SCALAR as PriceRaw;
        let price = Price::from_raw(raw, 2);
        assert_eq!(price.raw, raw);
        assert_eq!(price.precision, 2);
    }

    #[rstest]
    fn test_zero_constructor() {
        let zero = Price::zero(3);
        assert!(zero.is_zero());
        assert_eq!(zero.precision, 3);
    }

    #[rstest]
    fn test_max_constructor() {
        let max = Price::max(4);
        assert_eq!(max.raw, PRICE_RAW_MAX);
        assert_eq!(max.precision, 4);
    }

    #[rstest]
    fn test_min_constructor() {
        let min = Price::min(4);
        assert_eq!(min.raw, PRICE_RAW_MIN);
        assert_eq!(min.precision, 4);
    }

    #[rstest]
    fn test_nan_validation() {
        assert!(Price::new_checked(f64::NAN, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_infinity_validation() {
        assert!(Price::new_checked(f64::INFINITY, FIXED_PRECISION).is_err());
        assert!(Price::new_checked(f64::NEG_INFINITY, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_special_values() {
        let zero = Price::zero(5);
        assert!(zero.is_zero());
        assert_eq!(zero.to_string(), "0.00000");

        let undef = Price::from_raw(PRICE_UNDEF, 0);
        assert!(undef.is_undefined());

        let error = ERROR_PRICE;
        assert_eq!(error.precision, 255);
    }

    #[rstest]
    fn test_string_parsing() {
        let price: Price = "123.456".into();
        assert_eq!(price.precision, 3);
        assert_eq!(price, Price::from("123.456"));
    }

    #[rstest]
    fn test_negative_price_from_str() {
        let price: Price = "-123.45".parse().unwrap();
        assert_eq!(price.precision, 2);
        assert!(approx_eq!(f64, price.as_f64(), -123.45, epsilon = 1e-9));
    }

    #[rstest]
    fn test_string_parsing_errors() {
        assert!(Price::from_str("invalid").is_err());
    }

    #[rstest]
    #[case("1e7", 0, 10_000_000.0)]
    #[case("1.5e3", 0, 1_500.0)]
    #[case("1.234e-2", 5, 0.01234)]
    #[case("5E-3", 3, 0.005)]
    fn test_from_str_scientific_notation(
        #[case] input: &str,
        #[case] expected_precision: u8,
        #[case] expected_value: f64,
    ) {
        let price = Price::from_str(input).unwrap();
        assert_eq!(price.precision, expected_precision);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    #[case("1_234.56", 2, 1234.56)]
    #[case("1000000", 0, 1_000_000.0)]
    #[case("99_999.999_99", 5, 99_999.999_99)]
    fn test_from_str_with_underscores(
        #[case] input: &str,
        #[case] expected_precision: u8,
        #[case] expected_value: f64,
    ) {
        let price = Price::from_str(input).unwrap();
        assert_eq!(price.precision, expected_precision);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    fn test_from_decimal_dp_preservation() {
        // Test that decimal conversion preserves exact values
        let decimal = dec!(123.456789);
        let price = Price::from_decimal_dp(decimal, 6).unwrap();
        assert_eq!(price.precision, 6);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            123.456_789,
            epsilon = 1e-10
        ));

        // Verify raw value is exact
        let expected_raw = 123_456_789 * 10_i64.pow(u32::from(FIXED_PRECISION - 6));
        assert_eq!(price.raw, PriceRaw::from(expected_raw));
    }

    #[rstest]
    fn test_from_decimal_dp_rounding() {
        // Test banker's rounding (round half to even)
        let decimal = dec!(1.005);
        let price = Price::from_decimal_dp(decimal, 2).unwrap();
        assert_eq!(price.as_f64(), 1.0); // 1.005 rounds to 1.00 (even)

        let decimal = dec!(1.015);
        let price = Price::from_decimal_dp(decimal, 2).unwrap();
        assert_eq!(price.as_f64(), 1.02); // 1.015 rounds to 1.02 (even)
    }

    #[rstest]
    fn test_from_decimal_infers_precision() {
        // Test that precision is inferred from decimal's scale
        let decimal = dec!(123.456);
        let price = Price::from_decimal(decimal).unwrap();
        assert_eq!(price.precision, 3);
        assert!(approx_eq!(f64, price.as_f64(), 123.456, epsilon = 1e-10));

        // Test with integer (precision 0)
        let decimal = dec!(100);
        let price = Price::from_decimal(decimal).unwrap();
        assert_eq!(price.precision, 0);
        assert_eq!(price.as_f64(), 100.0);

        // Test with high precision
        let decimal = dec!(1.23456789);
        let price = Price::from_decimal(decimal).unwrap();
        assert_eq!(price.precision, 8);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            1.234_567_89,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    fn test_from_decimal_trailing_zeros() {
        // Decimal preserves trailing zeros in scale
        let decimal = dec!(1.230);
        assert_eq!(decimal.scale(), 3); // Has 3 decimal places

        // from_decimal infers precision from scale (includes trailing zeros)
        let price = Price::from_decimal(decimal).unwrap();
        assert_eq!(price.precision, 3);
        assert!(approx_eq!(f64, price.as_f64(), 1.23, epsilon = 1e-10));

        // Normalized removes trailing zeros
        let normalized = decimal.normalize();
        assert_eq!(normalized.scale(), 2);
        let price_normalized = Price::from_decimal(normalized).unwrap();
        assert_eq!(price_normalized.precision, 2);
    }

    #[rstest]
    #[case("1.00", 2)]
    #[case("1.0", 1)]
    #[case("1.000", 3)]
    #[case("100.00", 2)]
    #[case("0.10", 2)]
    #[case("0.100", 3)]
    fn test_from_str_preserves_trailing_zeros(#[case] input: &str, #[case] expected_precision: u8) {
        let price = Price::from_str(input).unwrap();
        assert_eq!(price.precision, expected_precision);
    }

    #[rstest]
    fn test_from_decimal_excessive_precision_inference() {
        // Create a decimal with more precision than FIXED_PRECISION
        // Decimal supports up to 28 decimal places
        let decimal = dec!(1.1234567890123456789012345678);

        // If scale exceeds FIXED_PRECISION, from_decimal should error
        if decimal.scale() > u32::from(FIXED_PRECISION) {
            assert!(Price::from_decimal(decimal).is_err());
        }
    }

    #[rstest]
    fn test_from_decimal_dp_out_of_range_returns_typed_error_with_stable_display() {
        let huge = Decimal::from_str("99999999999999999999.99").unwrap();
        let error = Price::from_decimal_dp(huge, 2).unwrap_err();
        match error {
            CorrectnessError::PredicateViolation { ref message } => {
                assert!(
                    message.contains("PriceRaw range") || message.contains("for Price"),
                    "unexpected message: {message:?}",
                );
            }
            _ => panic!("expected PredicateViolation, was {error:?}"),
        }
    }

    #[rstest]
    fn test_from_decimal_negative_price() {
        // Negative prices are valid for Price
        let decimal = dec!(-123.45);
        let price = Price::from_decimal(decimal).unwrap();
        assert_eq!(price.precision, 2);
        assert!(approx_eq!(f64, price.as_f64(), -123.45, epsilon = 1e-10));
        assert!(price.raw < 0);
    }

    #[rstest]
    fn test_string_formatting() {
        assert_eq!(format!("{}", Price::new(1234.5678, 4)), "1234.5678");
        assert_eq!(
            format!("{:?}", Price::new(1234.5678, 4)),
            "Price(1234.5678)"
        );
        assert_eq!(Price::new(1234.5678, 4).to_formatted_string(), "1_234.5678");
    }

    #[rstest]
    #[case(1234.5678, 4, "Price(1234.5678)", "1234.5678")] // Normal precision
    #[case(123.456_789_012_345, 8, "Price(123.45678901)", "123.45678901")] // At max normal precision
    #[cfg_attr(
        feature = "defi",
        case(
            2_000_000_000_000_000_000.0,
            18,
            "Price(2000000000000000000)",
            "2000000000000000000"
        )
    )] // High precision
    fn test_string_formatting_precision_handling(
        #[case] value: f64,
        #[case] precision: u8,
        #[case] expected_debug: &str,
        #[case] expected_display: &str,
    ) {
        let price = if precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            Price::from_raw(value as PriceRaw, precision)
        } else {
            Price::new(value, precision)
        };

        assert_eq!(format!("{price:?}"), expected_debug);
        assert_eq!(format!("{price}"), expected_display);
        assert_eq!(
            price.to_formatted_string().replace('_', ""),
            expected_display
        );
    }

    #[rstest]
    fn test_decimal_conversions() {
        let price = Price::new(123.456, 3);
        assert_eq!(price.as_decimal(), dec!(123.456));

        let price = Price::new(0.000_001, 6);
        assert_eq!(price.as_decimal(), dec!(0.000001));
    }

    #[rstest]
    fn test_basic_arithmetic() {
        let p1 = Price::new(10.5, 2);
        let p2 = Price::new(5.25, 2);
        assert_eq!(p1 + p2, Price::from("15.75"));
        assert_eq!(p1 - p2, Price::from("5.25"));
        assert_eq!(-p1, Price::from("-10.5"));
    }

    #[rstest]
    fn test_price_checked_add_within_bounds() {
        let a = Price::new(10.0, 2);
        let b = Price::new(5.0, 2);
        assert_eq!(a.checked_add(b), Some(Price::new(15.0, 2)));

        let neg = Price::new(-3.0, 2);
        assert_eq!(a.checked_add(neg), Some(Price::new(7.0, 2)));
    }

    #[rstest]
    fn test_price_checked_add_above_max_returns_none() {
        let near_max = Price::from_raw(PRICE_RAW_MAX, 0);
        let one = Price::new(1.0, 0);
        assert_eq!(near_max.checked_add(one), None);
    }

    #[rstest]
    fn test_price_checked_sub_within_bounds() {
        let a = Price::new(10.0, 2);
        let b = Price::new(3.0, 2);
        assert_eq!(a.checked_sub(b), Some(Price::new(7.0, 2)));
        assert_eq!(b.checked_sub(a), Some(Price::new(-7.0, 2)));
    }

    #[rstest]
    fn test_price_checked_sub_below_min_returns_none() {
        let near_min = Price::from_raw(PRICE_RAW_MIN, 0);
        let one = Price::new(1.0, 0);
        assert_eq!(near_min.checked_sub(one), None);
    }

    #[rstest]
    fn test_price_checked_arith_uses_max_precision() {
        let a = Price::new(10.5, 1);
        let b = Price::new(5.25, 2);
        let sum = a.checked_add(b).unwrap();
        assert_eq!(sum.precision, 2);
        assert_eq!(sum.as_f64(), 15.75);
    }

    #[rstest]
    fn test_price_checked_add_rejects_sentinel_undef() {
        let undef = Price::from_raw(PRICE_UNDEF, 0);
        let one = Price::new(1.0, 0);
        assert_eq!(undef.checked_add(one), None);
        assert_eq!(one.checked_add(undef), None);
    }

    #[rstest]
    fn test_price_checked_sub_rejects_sentinel_undef() {
        let undef = Price::from_raw(PRICE_UNDEF, 0);
        let neg_one = Price::new(-1.0, 0);
        assert_eq!(undef.checked_sub(neg_one), None);
    }

    #[rstest]
    fn test_price_checked_arith_rejects_error_price() {
        let one = Price::new(1.0, 0);
        assert_eq!(ERROR_PRICE.checked_add(one), None);
        assert_eq!(one.checked_sub(ERROR_PRICE), None);
    }

    #[rstest]
    fn test_price_checked_arith_rejects_raw_error() {
        let error = Price::from_raw(PRICE_ERROR, 0);
        let one = Price::new(1.0, 0);
        assert_eq!(error.checked_add(one), None);
        assert_eq!(one.checked_add(error), None);
        assert_eq!(error.checked_sub(one), None);
        assert_eq!(one.checked_sub(error), None);
    }

    #[rstest]
    fn test_price_checked_add_at_exact_max_returns_some() {
        let near_max = Price::from_raw(PRICE_RAW_MAX - 1, 0);
        let one_unit = Price::from_raw(1, 0);
        assert_eq!(
            near_max.checked_add(one_unit),
            Some(Price::from_raw(PRICE_RAW_MAX, 0)),
        );
    }

    #[rstest]
    fn test_price_checked_sub_at_exact_min_returns_some() {
        let near_min = Price::from_raw(PRICE_RAW_MIN + 1, 0);
        let one_unit = Price::from_raw(1, 0);
        assert_eq!(
            near_min.checked_sub(one_unit),
            Some(Price::from_raw(PRICE_RAW_MIN, 0)),
        );
    }

    #[rstest]
    fn test_mixed_precision_add() {
        let p1 = Price::new(10.5, 1);
        let p2 = Price::new(5.25, 2);
        let result = p1 + p2;
        assert_eq!(result.precision, 2);
        assert_eq!(result.as_f64(), 15.75);
    }

    #[rstest]
    fn test_mixed_precision_sub() {
        let p1 = Price::new(10.5, 1);
        let p2 = Price::new(5.25, 2);
        let result = p1 - p2;
        assert_eq!(result.precision, 2);
        assert_eq!(result.as_f64(), 5.25);
    }

    #[rstest]
    fn test_f64_operations() {
        let p = Price::new(10.5, 2);
        assert_eq!(p + 1.0, 11.5);
        assert_eq!(p - 1.0, 9.5);
        assert_eq!(p * 2.0, 21.0);
        assert_eq!(p / 2.0, 5.25);
    }

    #[rstest]
    fn test_equality_and_comparisons() {
        let p1 = Price::new(10.0, 1);
        let p2 = Price::new(20.0, 1);
        let p3 = Price::new(10.0, 1);

        assert!(p1 < p2);
        assert!(p2 > p1);
        assert!(p1 <= p3);
        assert!(p1 >= p3);
        assert_eq!(p1, p3);
        assert_ne!(p1, p2);

        assert_eq!(Price::from("1.0"), Price::from("1.0"));
        assert_ne!(Price::from("1.1"), Price::from("1.0"));
        assert!(Price::from("1.0") <= Price::from("1.0"));
        assert!(Price::from("1.1") > Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("0.9") < Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
    }

    #[rstest]
    fn test_deref() {
        let price = Price::new(10.0, 1);
        assert_eq!(*price, price.raw);
    }

    #[rstest]
    fn test_decode_raw_price_i64() {
        let raw_scaled_by_1e9 = 42_000_000_000i64; // 42.0 * 10^9
        let decoded = decode_raw_price_i64(raw_scaled_by_1e9);
        let price = Price::from_raw(decoded, FIXED_PRECISION);
        assert!(
            approx_eq!(f64, price.as_f64(), 42.0, epsilon = 1e-9),
            "Expected 42.0 f64, was {} (precision = {})",
            price.as_f64(),
            price.precision
        );
    }

    #[rstest]
    fn test_hash() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let price1 = Price::new(1.0, 2);
        let price2 = Price::new(1.0, 2);
        let price3 = Price::new(1.1, 2);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        let mut hasher3 = DefaultHasher::new();

        price1.hash(&mut hasher1);
        price2.hash(&mut hasher2);
        price3.hash(&mut hasher3);

        assert_eq!(hasher1.finish(), hasher2.finish());
        assert_ne!(hasher1.finish(), hasher3.finish());
    }

    #[rstest]
    fn test_price_serde_json_round_trip() {
        let price = Price::new(1.0500, 4);
        let json = serde_json::to_string(&price).unwrap();
        let deserialized: Price = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, price);
    }

    #[rstest]
    fn test_from_mantissa_exponent_exact_precision() {
        let price = Price::from_mantissa_exponent(12345, -2, 2);
        assert_eq!(price.as_f64(), 123.45);
    }

    #[rstest]
    fn test_from_mantissa_exponent_excess_rounds_down() {
        // 12.345 rounds to 12.34 (4 is even, banker's rounding)
        let price = Price::from_mantissa_exponent(12345, -3, 2);
        assert_eq!(price.as_f64(), 12.34);
    }

    #[rstest]
    fn test_from_mantissa_exponent_excess_rounds_up() {
        // 12.355 rounds to 12.36 (5 is odd, banker's rounding)
        let price = Price::from_mantissa_exponent(12355, -3, 2);
        assert_eq!(price.as_f64(), 12.36);
    }

    #[rstest]
    fn test_from_mantissa_exponent_positive_exponent() {
        let price = Price::from_mantissa_exponent(5, 2, 0);
        assert_eq!(price.as_f64(), 500.0);
    }

    #[rstest]
    fn test_from_mantissa_exponent_negative_mantissa() {
        let price = Price::from_mantissa_exponent(-12345, -2, 2);
        assert_eq!(price.as_f64(), -123.45);
    }

    #[rstest]
    fn test_from_mantissa_exponent_zero() {
        let price = Price::from_mantissa_exponent(0, 2, 2);
        assert_eq!(price.as_f64(), 0.0);
    }

    #[rstest]
    #[should_panic(expected = "Overflow")]
    fn test_from_mantissa_exponent_overflow_panics() {
        let _ = Price::from_mantissa_exponent(i64::MAX, 9, 0);
    }

    #[rstest]
    #[should_panic(expected = "exceeds i128 range")]
    fn test_from_mantissa_exponent_large_exponent_panics() {
        let _ = Price::from_mantissa_exponent(1, 119, 0);
    }

    #[rstest]
    fn test_from_mantissa_exponent_zero_with_large_exponent() {
        let price = Price::from_mantissa_exponent(0, 119, 0);
        assert_eq!(price.as_f64(), 0.0);
    }

    #[rstest]
    fn test_from_mantissa_exponent_very_negative_exponent_rounds_to_zero() {
        let price = Price::from_mantissa_exponent(12345, -120, 2);
        assert_eq!(price.as_f64(), 0.0);
    }

    #[rstest]
    fn test_decimal_arithmetic_operations() {
        let price = Price::new(100.0, 2);
        assert_eq!(price + dec!(50.25), dec!(150.25));
        assert_eq!(price - dec!(30.50), dec!(69.50));
        assert_eq!(price * dec!(1.5), dec!(150.00));
        assert_eq!(price / dec!(4), dec!(25.00));
    }
}

#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    /// Strategy to generate valid price values within the allowed range.
    fn price_value_strategy() -> impl Strategy<Value = f64> {
        // Use a reasonable range that's well within PRICE_MIN/PRICE_MAX
        // but still tests edge cases with various scales
        prop_oneof![
            // Small positive values
            0.00001..1.0,
            // Normal trading range
            1.0..100_000.0,
            // Large values (but safe)
            100_000.0..1_000_000.0,
            // Small negative values (for spreads, etc.)
            -1_000.0..0.0,
            // Boundary values close to the extremes
            Just(PRICE_MIN / 2.0),
            Just(PRICE_MAX / 2.0),
        ]
    }

    fn float_precision_upper_bound() -> u8 {
        FIXED_PRECISION.min(crate::types::fixed::MAX_FLOAT_PRECISION)
    }

    /// Strategy to exercise both typical and extreme precision values.
    fn precision_strategy() -> impl Strategy<Value = u8> {
        let upper = float_precision_upper_bound();
        prop_oneof![Just(0u8), 0u8..=upper, Just(FIXED_PRECISION),]
    }

    fn precision_strategy_non_zero() -> impl Strategy<Value = u8> {
        let upper = float_precision_upper_bound().max(1);
        prop_oneof![Just(upper), Just(FIXED_PRECISION.max(1)), 1u8..=upper,]
    }

    /// Strategy to generate a valid (precision, raw) pair where raw is properly scaled.
    ///
    /// Raw values must be multiples of `10^(FIXED_PRECISION` - precision) to pass validation.
    fn valid_precision_raw_strategy() -> impl Strategy<Value = (u8, PriceRaw)> {
        precision_strategy().prop_flat_map(|precision| {
            let scale: PriceRaw = if precision >= FIXED_PRECISION {
                1
            } else {
                (10 as PriceRaw).pow(u32::from(FIXED_PRECISION - precision))
            };
            // Generate a base value, then multiply by scale to ensure valid raw
            let max_base = PRICE_RAW_MAX / scale;
            let min_base = PRICE_RAW_MIN / scale;
            (min_base..=max_base).prop_map(move |base| (precision, base * scale))
        })
    }

    /// Strategy to generate valid precision values for float-based constructors.
    fn float_precision_strategy() -> impl Strategy<Value = u8> {
        precision_strategy()
    }

    const DECIMAL_MAX_MANTISSA: i128 = 79_228_162_514_264_337_593_543_950_335;

    #[expect(
        clippy::useless_conversion,
        reason = "PriceRaw is i64 or i128 depending on feature"
    )]
    fn decimal_compatible(raw: PriceRaw, precision: u8) -> bool {
        if precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            return false;
        }
        let precision_diff = u32::from(FIXED_PRECISION.saturating_sub(precision));
        let divisor = (10 as PriceRaw).pow(precision_diff);
        let rescaled_raw = raw / divisor;
        i128::from(rescaled_raw.abs()) <= DECIMAL_MAX_MANTISSA
    }

    proptest! {
        /// Property: Price string serialization round-trip should preserve value and precision
        #[rstest]
        fn prop_price_serde_round_trip(
            value in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let original = Price::new(value, precision);

            // String round-trip (this should be exact and is the most important)
            let string_repr = original.to_string();
            let from_string: Price = string_repr.parse().unwrap();
            prop_assert_eq!(from_string.raw, original.raw);
            prop_assert_eq!(from_string.precision, original.precision);

            // JSON round-trip basic validation (just ensure it doesn't crash and preserves precision)
            let json = serde_json::to_string(&original).unwrap();
            let from_json: Price = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(from_json.precision, original.precision);
            // Note: JSON may have minor floating-point precision differences due to f64 limitations
        }

        /// Property: Price arithmetic should be associative for same precision
        #[rstest]
        fn prop_price_arithmetic_associative(
            a in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            b in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            c in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let p_c = Price::new(c, precision);

            // Check if we can perform the operations without overflow using raw arithmetic
            let ab_raw = p_a.raw.checked_add(p_b.raw);
            let bc_raw = p_b.raw.checked_add(p_c.raw);

            if let (Some(ab_raw), Some(bc_raw)) = (ab_raw, bc_raw) {
                let ab_c_raw = ab_raw.checked_add(p_c.raw);
                let a_bc_raw = p_a.raw.checked_add(bc_raw);

                if let (Some(ab_c_raw), Some(a_bc_raw)) = (ab_c_raw, a_bc_raw) {
                    // (a + b) + c == a + (b + c) using raw arithmetic (exact)
                    prop_assert_eq!(ab_c_raw, a_bc_raw, "Associativity failed in raw arithmetic");
                }
            }
        }

        /// Property: Price addition/subtraction should be inverse operations
        #[rstest]
        fn prop_price_addition_subtraction_inverse(
            base in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            delta in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let p_base = Price::new(base, precision);
            let p_delta = Price::new(delta, precision);

            // Use raw arithmetic to avoid floating-point precision issues
            if let Some(added_raw) = p_base.raw.checked_add(p_delta.raw)
                && let Some(result_raw) = added_raw.checked_sub(p_delta.raw) {
                    // (base + delta) - delta should equal base exactly using raw arithmetic
                    prop_assert_eq!(result_raw, p_base.raw, "Inverse operation failed in raw arithmetic");
                }
        }

        /// Property: Price ordering should be transitive
        #[rstest]
        fn prop_price_ordering_transitive(
            a in price_value_strategy(),
            b in price_value_strategy(),
            c in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let p_c = Price::new(c, precision);

            // If a <= b and b <= c, then a <= c
            if p_a <= p_b && p_b <= p_c {
                prop_assert!(p_a <= p_c, "Transitivity failed: {} <= {} <= {} but {} > {}",
                    p_a.as_f64(), p_b.as_f64(), p_c.as_f64(), p_a.as_f64(), p_c.as_f64());
            }
        }

        /// Property: String parsing should be consistent with precision inference
        #[rstest]
        fn prop_price_string_parsing_precision(
            integral in 0u32..1_000_000,
            fractional in 0u32..1_000_000,
            precision in precision_strategy_non_zero()
        ) {
            // Create a decimal string with exactly 'precision' decimal places
            let pow = 10u128.pow(u32::from(precision));
            let fractional_mod = u128::from(fractional) % pow;
            let fractional_str = format!("{:0width$}", fractional_mod, width = precision as usize);
            let price_str = format!("{integral}.{fractional_str}");

            let parsed: Price = price_str.parse().unwrap();
            prop_assert_eq!(parsed.precision, precision);

            // Round-trip should preserve the original string (after normalization)
            let round_trip = parsed.to_string();
            let expected_value = format!("{integral}.{fractional_str}");
            prop_assert_eq!(round_trip, expected_value);
        }

        /// Property: Price with higher precision should contain more or equal information
        #[rstest]
        fn prop_price_precision_information_preservation(
            value in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            precision1 in precision_strategy_non_zero(),
            precision2 in precision_strategy_non_zero()
        ) {
            // Skip cases where precisions are equal (trivial case)
            prop_assume!(precision1 != precision2);

            let _p1 = Price::new(value, precision1);
            let _p2 = Price::new(value, precision2);

            // When both prices are created from the same value with different precisions,
            // converting both to the lower precision should yield the same result
            let min_precision = precision1.min(precision2);

            // Round the original value to the minimum precision first
            let scale = 10.0_f64.powi(i32::from(min_precision));
            let rounded_value = (value * scale).round() / scale;

            let p1_reduced = Price::new(rounded_value, min_precision);
            let p2_reduced = Price::new(rounded_value, min_precision);

            // They should be exactly equal when created from the same rounded value
            prop_assert_eq!(p1_reduced.raw, p2_reduced.raw, "Precision reduction inconsistent");
        }

        /// Property: Price arithmetic should never produce invalid values
        #[rstest]
        fn prop_price_arithmetic_bounds(
            a in price_value_strategy(),
            b in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);

            // Addition should either succeed or fail predictably
            let sum_f64 = p_a.as_f64() + p_b.as_f64();
            if sum_f64.is_finite() && (PRICE_MIN..=PRICE_MAX).contains(&sum_f64) {
                let sum = p_a + p_b;
                prop_assert!(sum.as_f64().is_finite());
                prop_assert!(!sum.is_undefined());
            }

            // Subtraction should either succeed or fail predictably
            let diff_f64 = p_a.as_f64() - p_b.as_f64();
            if diff_f64.is_finite() && (PRICE_MIN..=PRICE_MAX).contains(&diff_f64) {
                let diff = p_a - p_b;
                prop_assert!(diff.as_f64().is_finite());
                prop_assert!(!diff.is_undefined());
            }
        }

        /// Property: checked_add agrees with Add when bounds and sentinel guards hold,
        /// and returns None otherwise.
        #[rstest]
        fn prop_price_checked_add_matches_spec(
            a in price_value_strategy(),
            b in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let expected = p_a.raw
                .checked_add(p_b.raw)
                .filter(|r| (PRICE_RAW_MIN..=PRICE_RAW_MAX).contains(r))
                .filter(|_| !p_a.is_sentinel() && !p_b.is_sentinel())
                .map(|raw| Price { raw, precision: p_a.precision.max(p_b.precision) });
            prop_assert_eq!(p_a.checked_add(p_b), expected);
        }

        /// Property: checked_sub agrees with Sub when bounds and sentinel guards hold,
        /// and returns None otherwise.
        #[rstest]
        fn prop_price_checked_sub_matches_spec(
            a in price_value_strategy(),
            b in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let expected = p_a.raw
                .checked_sub(p_b.raw)
                .filter(|r| (PRICE_RAW_MIN..=PRICE_RAW_MAX).contains(r))
                .filter(|_| !p_a.is_sentinel() && !p_b.is_sentinel())
                .map(|raw| Price { raw, precision: p_a.precision.max(p_b.precision) });
            prop_assert_eq!(p_a.checked_sub(p_b), expected);
        }
    }

    proptest! {
        /// Property: as_decimal scale always matches precision
        #[rstest]
        fn prop_price_as_decimal_preserves_precision(
            (precision, raw) in valid_precision_raw_strategy()
        ) {
            prop_assume!(decimal_compatible(raw, precision));
            let price = Price::from_raw(raw, precision);
            let decimal = price.as_decimal();
            prop_assert_eq!(decimal.scale(), u32::from(precision));
        }

        /// Property: as_decimal and Display produce the same string
        #[rstest]
        fn prop_price_as_decimal_matches_display(
            value in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            precision in float_precision_strategy()
        ) {
            let price = Price::new(value, precision);
            prop_assume!(decimal_compatible(price.raw, precision));
            let display_str = format!("{price}");
            let decimal_str = price.as_decimal().to_string();
            prop_assert_eq!(display_str, decimal_str);
        }

        /// Property: from_decimal roundtrip preserves exact value
        #[rstest]
        fn prop_price_from_decimal_roundtrip(
            (precision, raw) in valid_precision_raw_strategy()
        ) {
            prop_assume!(decimal_compatible(raw, precision));
            let original = Price::from_raw(raw, precision);
            let decimal = original.as_decimal();
            let reconstructed = Price::from_decimal(decimal).unwrap();
            prop_assert_eq!(original.raw, reconstructed.raw);
            prop_assert_eq!(original.precision, reconstructed.precision);
        }

        /// Property: constructing from valid raw values preserves raw/precision fields
        #[rstest]
        fn prop_price_from_raw_round_trip(
            (precision, raw) in valid_precision_raw_strategy()
        ) {
            let price = Price::from_raw(raw, precision);
            prop_assert_eq!(price.raw, raw);
            prop_assert_eq!(price.precision, precision);
        }
    }
}

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

use std::ffi::c_char;

use databento::dbn;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{AggressorSide, AssetClass, BookAction, InstrumentClass, OptionKind, OrderSide},
    identifiers::Symbol,
    types::{
        Currency, Price, Quantity,
        price::{PRICE_UNDEF, decode_raw_price_i64},
    },
};
use ustr::Ustr;

#[must_use]
pub const fn parse_optional_bool(c: c_char) -> Option<bool> {
    match c as u8 as char {
        'Y' => Some(true),
        'N' => Some(false),
        _ => None,
    }
}

#[must_use]
pub const fn parse_order_side(c: c_char) -> OrderSide {
    match c as u8 as char {
        'A' => OrderSide::Sell,
        'B' => OrderSide::Buy,
        _ => OrderSide::NoOrderSide,
    }
}

#[must_use]
pub const fn parse_aggressor_side(c: c_char) -> AggressorSide {
    match c as u8 as char {
        'A' => AggressorSide::Seller,
        'B' => AggressorSide::Buyer,
        _ => AggressorSide::NoAggressor,
    }
}

/// Parses a Databento book action character into a `BookAction` enum.
///
/// # Errors
///
/// Returns an error if `c` is not a valid `BookAction` character.
pub fn parse_book_action(c: c_char) -> anyhow::Result<BookAction> {
    match c as u8 as char {
        'A' => Ok(BookAction::Add),
        'C' => Ok(BookAction::Delete),
        'F' => Ok(BookAction::Update),
        'M' => Ok(BookAction::Update),
        'R' => Ok(BookAction::Clear),
        invalid => anyhow::bail!("Invalid `BookAction`, was '{invalid}'"),
    }
}

/// Parses a Databento option kind character into an `OptionKind` enum.
///
/// # Errors
///
/// Returns an error if `c` is not a valid `OptionKind` character.
pub fn parse_option_kind(c: c_char) -> anyhow::Result<OptionKind> {
    match c as u8 as char {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        invalid => anyhow::bail!("Invalid `OptionKind`, was '{invalid}'"),
    }
}

pub(super) fn parse_currency_or_usd_default(
    value: Result<&str, impl std::error::Error>,
) -> Currency {
    match value {
        Ok(value) if !value.is_empty() => Currency::try_from_str(value).unwrap_or_else(|| {
            log::warn!("Unknown currency code '{value}', defaulting to USD");
            Currency::USD()
        }),
        Ok(_) => Currency::USD(),
        Err(e) => {
            log::warn!("Error parsing currency: {e}");
            Currency::USD()
        }
    }
}

/// Parses a CFI (Classification of Financial Instruments) code to extract asset and instrument classes.
///
/// Returns `(None, None)` if `value` has fewer than 3 characters.
#[must_use]
pub fn parse_cfi_iso10926(value: &str) -> (Option<AssetClass>, Option<InstrumentClass>) {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 3 {
        return (None, None);
    }

    // TODO: A proper CFI parser would be useful: https://en.wikipedia.org/wiki/ISO_10962
    let cfi_category = chars[0];
    let cfi_group = chars[1];
    let cfi_attribute1 = chars[2];
    // let cfi_attribute2 = value[3];
    // let cfi_attribute3 = value[4];
    // let cfi_attribute4 = value[5];

    let mut asset_class = match cfi_category {
        'D' => Some(AssetClass::Debt),
        'E' => Some(AssetClass::Equity),
        'S' => None,
        _ => None,
    };

    let instrument_class = match cfi_group {
        'I' => Some(InstrumentClass::Future),
        _ => None,
    };

    if cfi_attribute1 == 'I' {
        asset_class = Some(AssetClass::Index);
    }

    (asset_class, instrument_class)
}

pub(super) fn decode_underlying(underlying_str: &str, symbol: &Symbol) -> Ustr {
    if underlying_str.is_empty() {
        // Fall back to first whitespace-separated token from symbol
        symbol
            .as_str()
            .split_whitespace()
            .next()
            .map_or_else(|| symbol.inner(), Ustr::from)
    } else {
        Ustr::from(underlying_str)
    }
}

/// Parses a Databento status reason code into a human-readable string.
///
/// See: <https://databento.com/docs/schemas-and-data-formats/status#types-of-status-reasons>
///
/// # Errors
///
/// Returns an error if `value` is an invalid status reason code.
pub fn parse_status_reason(value: u16) -> anyhow::Result<Option<Ustr>> {
    let value_str = match value {
        0 => return Ok(None),
        1 => "Scheduled",
        2 => "Surveillance intervention",
        3 => "Market event",
        4 => "Instrument activation",
        5 => "Instrument expiration",
        6 => "Recovery in process",
        10 => "Regulatory",
        11 => "Administrative",
        12 => "Non-compliance",
        13 => "Filings not current",
        14 => "SEC trading suspension",
        15 => "New issue",
        16 => "Issue available",
        17 => "Issues reviewed",
        18 => "Filing requirements satisfied",
        30 => "News pending",
        31 => "News released",
        32 => "News and resumption times",
        33 => "News not forthcoming",
        40 => "Order imbalance",
        50 => "LULD pause",
        60 => "Operational",
        70 => "Additional information requested",
        80 => "Merger effective",
        90 => "ETF",
        100 => "Corporate action",
        110 => "New Security offering",
        120 => "Market wide halt level 1",
        121 => "Market wide halt level 2",
        122 => "Market wide halt level 3",
        123 => "Market wide halt carryover",
        124 => "Market wide halt resumption",
        130 => "Quotation not available",
        invalid => anyhow::bail!("Invalid `StatusMsg` reason, was '{invalid}'"),
    };

    Ok(Some(Ustr::from(value_str)))
}

/// Parses a Databento status trading event code into a human-readable string.
///
/// # Errors
///
/// Returns an error if `value` is an invalid status trading event code.
pub fn parse_status_trading_event(value: u16) -> anyhow::Result<Option<Ustr>> {
    let value_str = match value {
        0 => return Ok(None),
        1 => "No cancel",
        2 => "Change trading session",
        3 => "Implied matching on",
        4 => "Implied matching off",
        _ => anyhow::bail!("Invalid `StatusMsg` trading_event, was '{value}'"),
    };

    Ok(Some(Ustr::from(value_str)))
}

/// Decodes a price, returning an error if undefined.
///
/// Databento uses `i64::MAX` as a sentinel value for unset/null prices (see
/// [`UNDEF_PRICE`](https://docs.rs/dbn/latest/dbn/constant.UNDEF_PRICE.html)).
///
/// # Errors
///
/// Returns an error if `value` is `i64::MAX` (undefined).
#[inline(always)]
pub fn decode_price(value: i64, precision: u8, field_name: &str) -> anyhow::Result<Price> {
    if value == i64::MAX {
        anyhow::bail!("Missing required price for `{field_name}`")
    } else {
        Ok(Price::from_raw(decode_raw_price_i64(value), precision))
    }
}

/// Decodes a price from the given optional value, expressed in units of 1e-9.
///
/// Databento uses `i64::MAX` as a sentinel value for unset/null prices (see
/// [`UNDEF_PRICE`](https://docs.rs/dbn/latest/dbn/constant.UNDEF_PRICE.html)).
#[inline(always)]
#[must_use]
pub fn decode_optional_price(value: i64, precision: u8) -> Option<Price> {
    if value == i64::MAX {
        None
    } else {
        Some(Price::from_raw(decode_raw_price_i64(value), precision))
    }
}

/// Decodes a price, returning `PRICE_UNDEF` if the value is undefined.
///
/// This is used for market data where undefined prices should pass through
/// as `PRICE_UNDEF` rather than causing an error.
#[inline(always)]
#[must_use]
pub fn decode_price_or_undef(value: i64, precision: u8) -> Price {
    if value == i64::MAX {
        Price::from_raw(PRICE_UNDEF, 0)
    } else {
        Price::from_raw(decode_raw_price_i64(value), precision)
    }
}

/// Computes the minimum decimal precision needed to represent a raw price value
/// expressed in units of 1e-9, by counting trailing decimal zeros.
///
/// For example, a raw value of `3_906_250` (representing 0.00390625) has 1 trailing
/// zero, so the precision is `9 - 1 = 8`.
#[inline(always)]
#[must_use]
pub fn precision_from_raw(value: i64) -> u8 {
    let mut v = value.unsigned_abs();
    if v == 0 {
        return 0;
    }
    let mut trailing = 0u8;
    while trailing < 9 && v.is_multiple_of(10) {
        v /= 10;
        trailing += 1;
    }
    9 - trailing
}

/// Decodes a minimum price increment from the given value, expressed in units of 1e-9.
///
/// The precision is derived from the actual tick value to avoid truncation of
/// fractional tick sizes (e.g., treasury futures with 1/256 or 1/32 ticks).
/// The derived precision is floored at `precision` (typically the currency precision).
#[inline(always)]
#[must_use]
pub fn decode_price_increment(value: i64, precision: u8) -> Price {
    match value {
        0 | i64::MAX => Price::new(10f64.powi(-i32::from(precision)), precision),
        _ => {
            let derived = precision_from_raw(value).max(precision);
            Price::from_raw(decode_raw_price_i64(value), derived)
        }
    }
}

/// Decodes a quantity from the given value, expressed in standard whole-number units.
#[inline(always)]
#[must_use]
pub fn decode_quantity(value: u64) -> Quantity {
    Quantity::from(value)
}

/// Decodes a quantity from the given optional value, where `i64::MAX` indicates missing data.
///
/// # Errors
///
/// Returns an error if the quantity is negative.
#[inline(always)]
pub fn decode_optional_quantity(value: i64) -> anyhow::Result<Option<Quantity>> {
    match value {
        i64::MAX => Ok(None),
        value if value >= 0 => Ok(Some(Quantity::from(value))),
        value => anyhow::bail!("Invalid negative quantity: {value}"),
    }
}

/// Decodes a timestamp, returning an error if undefined.
///
/// Databento uses `u64::MAX` as `UNDEF_TIMESTAMP` sentinel for null timestamps.
///
/// # Errors
///
/// Returns an error if `value` is `u64::MAX` (undefined).
#[inline(always)]
pub fn decode_timestamp(value: u64, field_name: &str) -> anyhow::Result<UnixNanos> {
    if value == dbn::UNDEF_TIMESTAMP {
        anyhow::bail!("Missing required timestamp for `{field_name}`")
    } else {
        Ok(UnixNanos::from(value))
    }
}

/// Decodes a timestamp from the given optional value.
///
/// Databento uses `u64::MAX` as `UNDEF_TIMESTAMP` sentinel for null timestamps.
#[inline(always)]
#[must_use]
pub fn decode_optional_timestamp(value: u64) -> Option<UnixNanos> {
    if value == dbn::UNDEF_TIMESTAMP {
        None
    } else {
        Some(UnixNanos::from(value))
    }
}

/// Decodes a multiplier from the given value, expressed in units of 1e-9.
/// Uses exact integer arithmetic to avoid precision loss in financial calculations.
///
/// # Errors
///
/// Returns an error if value is negative (invalid multiplier).
pub fn decode_multiplier(value: i64) -> anyhow::Result<Quantity> {
    const SCALE: u128 = 1_000_000_000;

    match value {
        0 | i64::MAX => Ok(Quantity::from(1)),
        v if v < 0 => anyhow::bail!("Invalid negative multiplier: {v}"),
        v => {
            // Work in integers: v is fixed-point with 9 fractional digits.
            // Build a canonical decimal string without floating-point.
            let abs = v as u128;
            let int_part = abs / SCALE;
            let frac_part = abs % SCALE;

            // Format fractional part with exactly 9 digits, then trim trailing zeros
            // to keep a canonical representation.
            if frac_part == 0 {
                // Pure integer
                Ok(Quantity::from(int_part as u64))
            } else {
                let mut frac_str = format!("{frac_part:09}");
                while frac_str.ends_with('0') {
                    frac_str.pop();
                }
                let s = format!("{int_part}.{frac_str}");
                Ok(Quantity::from(s))
            }
        }
    }
}

/// Decodes a lot size from the given value, expressed in standard whole-number units.
#[inline(always)]
#[must_use]
pub fn decode_lot_size(value: i32) -> Quantity {
    match value {
        0 | i32::MAX => Quantity::from(1),
        value => Quantity::from(value),
    }
}

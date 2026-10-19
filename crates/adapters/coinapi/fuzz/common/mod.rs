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

//! Instruments and invariant checks shared by the CoinAPI fuzz targets.
//!
//! Each target compiles this module separately and uses a subset of it.

#![allow(dead_code)]

use jiff::Timestamp;
use nautilus_coinapi::websocket::messages::Book;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::OrderBookDeltas,
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity, fixed::FIXED_PRECISION},
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Deserialize;
use serde_json::value::RawValue;

/// A typical crypto pair and one at the build's maximum precision.
#[must_use]
pub(crate) fn instruments() -> [InstrumentAny; 2] {
    [
        currency_pair("BTC-USD.COINBASE", 2, 8),
        currency_pair("ETH-USD.COINBASE", FIXED_PRECISION, FIXED_PRECISION),
    ]
}

#[must_use]
pub(crate) fn currency_pair(
    instrument_id: &str,
    price_precision: u8,
    size_precision: u8,
) -> InstrumentAny {
    let instrument_id = InstrumentId::from(instrument_id);
    let increment = |precision| Decimal::new(1, u32::from(precision));

    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(instrument_id.symbol)
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USD())
            .price_precision(price_precision)
            .size_precision(size_precision)
            .price_increment(
                Price::from_decimal_dp(increment(price_precision), price_precision).unwrap(),
            )
            .size_increment(
                Quantity::from_decimal_dp(increment(size_precision), size_precision).unwrap(),
            )
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

/// Asserts that `value` holds exactly the number the JSON token `raw` spells.
///
/// Compares canonical digit strings so the check shares no parsing with the decoder. Skips tokens
/// outside plain JSON number syntax, which the decoder may accept in quoted form.
pub(crate) fn assert_exact(raw: &RawValue, value: Decimal, field: &str) {
    let Some(expected) = token_digits(raw.get()) else {
        return;
    };

    let actual = canonical(
        value.is_sign_negative(),
        &value.mantissa().unsigned_abs().to_string(),
        -i128::from(value.scale()),
    );

    assert_eq!(
        actual, expected,
        "{field} {value} does not equal token {raw}"
    );
}

pub(crate) fn assert_volume(source: Decimal, volume: Quantity, precision: u8) {
    let actual = volume.as_decimal();
    let adjustment = (source - actual).abs();

    assert_eq!(volume.precision, precision);
    assert_eq!(
        actual,
        source.round_dp_with_strategy(u32::from(precision), RoundingStrategy::MidpointNearestEven)
    );
    assert!(source >= Decimal::ZERO);
    assert!(adjustment <= Decimal::new(1, u32::from(precision) + 6));
    assert!(adjustment * Decimal::new(1_000_000_000_000_000, 0) <= source);
}

/// Returns the sign, significant digits, and exponent of a plain JSON number token.
fn token_digits(raw: &str) -> Option<(bool, String, i128)> {
    let text = raw
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(raw);

    let (negative, body) = match text.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, text),
    };

    let (significand, exponent) = match body.split_once(['e', 'E']) {
        Some((significand, exponent)) => {
            let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if !is_digits(digits) {
                return None;
            }

            (significand, exponent.parse::<i128>().ok()?)
        }
        None => (body, 0),
    };

    let (whole, fraction) = match significand.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (significand, ""),
    };

    if !is_digits(whole) || !(fraction.is_empty() || is_digits(fraction)) {
        return None;
    }

    let exponent = exponent.checked_sub(i128::try_from(fraction.len()).ok()?)?;
    Some(canonical(negative, &format!("{whole}{fraction}"), exponent))
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn canonical(negative: bool, digits: &str, exponent: i128) -> (bool, String, i128) {
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return (false, "0".to_string(), 0);
    }

    let significant = digits.trim_end_matches('0');
    let zeros = i128::try_from(digits.len() - significant.len()).unwrap();
    (negative, significant.to_string(), exponent + zeros)
}

/// Raw tokens of every decimal field a CoinAPI data record can carry.
#[derive(Deserialize)]
pub(crate) struct Tokens<'a> {
    #[serde(borrow)]
    pub(crate) price: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) size: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) ask_price: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) ask_size: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) bid_price: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) bid_size: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) price_open: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) price_high: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) price_low: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) price_close: Option<&'a RawValue>,
    #[serde(borrow)]
    pub(crate) volume_traded: Option<&'a RawValue>,
    #[serde(borrow, default)]
    pub(crate) asks: Vec<LevelTokens<'a>>,
    #[serde(borrow, default)]
    pub(crate) bids: Vec<LevelTokens<'a>>,
}

#[derive(Deserialize)]
pub(crate) struct LevelTokens<'a> {
    #[serde(borrow)]
    pub(crate) price: &'a RawValue,
    #[serde(borrow)]
    pub(crate) size: &'a RawValue,
}

/// Asserts that decoded `(price, size)` levels equal the raw tokens of their sides.
pub(crate) fn assert_levels_exact(
    asks: impl ExactSizeIterator<Item = (Decimal, Decimal)>,
    bids: impl ExactSizeIterator<Item = (Decimal, Decimal)>,
    tokens: &Tokens,
) {
    assert_eq!(asks.len(), tokens.asks.len());
    assert_eq!(bids.len(), tokens.bids.len());

    for ((price, size), raw) in asks.chain(bids).zip(tokens.asks.iter().chain(&tokens.bids)) {
        assert_exact(raw.price, price, "price");
        assert_exact(raw.size, size, "size");
    }
}

/// Returns the UNIX nanoseconds of `time`, or `None` before the epoch.
#[must_use]
pub(crate) fn nanos(time: Timestamp) -> Option<UnixNanos> {
    u64::try_from(time.as_nanosecond())
        .ok()
        .map(UnixNanos::from)
}

/// Asserts that `deltas` is the level-by-level conversion of `book` at emitted `sequence`.
///
/// A snapshot clears the book and adds every level; an update replaces a level, or deletes it at
/// zero size. Bids precede asks in message order, and only the last delta ends the batch.
pub(crate) fn assert_level_deltas(book: &Book, deltas: &OrderBookDeltas, sequence: u64) {
    let is_snapshot = book.is_snapshot == Some(true);

    let snapshot_flag = if is_snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let levels: Vec<_> = book
        .bids
        .iter()
        .map(|level| (OrderSide::Buy, level))
        .chain(book.asks.iter().map(|level| (OrderSide::Sell, level)))
        .collect();

    let converted = if is_snapshot {
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        &deltas.deltas[1..]
    } else {
        &deltas.deltas[..]
    };

    assert_eq!(converted.len(), levels.len());

    for (delta, (side, level)) in converted.iter().zip(levels) {
        let action = if is_snapshot {
            BookAction::Add
        } else if level.size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        assert_eq!(delta.action, action);
        assert_eq!(delta.order.side, Some(side));
        assert_eq!(delta.order.price.as_decimal(), level.price);
        assert_eq!(delta.order.size.as_decimal(), level.size);
        assert_eq!(delta.order.order_id, 0);
    }

    for delta in &deltas.deltas {
        assert_eq!(Some(delta.ts_event), nanos(book.time_exchange));
        assert_eq!(Some(delta.ts_init), nanos(book.time_coinapi));
    }

    assert_batch(deltas, sequence, snapshot_flag);
}

/// Asserts the batch framing every emitted book message shares.
pub(crate) fn assert_batch(deltas: &OrderBookDeltas, sequence: u64, snapshot_flag: u8) {
    let last = deltas.deltas.len() - 1;
    for (i, delta) in deltas.deltas.iter().enumerate() {
        let last_flag = if i == last {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        assert_eq!(delta.flags, snapshot_flag | last_flag, "delta {i} flags");
        assert_eq!(delta.sequence, sequence, "delta {i} sequence");
        assert_eq!(
            delta.action == BookAction::Clear,
            snapshot_flag != 0 && i == 0,
            "delta {i} clear"
        );
    }

    assert_eq!(deltas.sequence, sequence);
}

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

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    enums::AggressorSide,
    identifiers::TradeId,
    instruments::BinaryOption,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;

use crate::http::models::{BookSide, FixedPoint, OutcomeSide};

/// Builds one trade tick from a public trade payload.
///
/// Returns `Ok(None)` for a block trade. A trade price and size must match the instrument
/// precision exactly.
///
/// # Errors
///
/// Returns an error if the payload is malformed, the YES and NO prices are inconsistent, the
/// taker direction is missing or inconsistent, a price or size is outside its exact domain, or the
/// timestamp is missing or invalid.
pub fn trade(
    value: Value,
    instrument: &BinaryOption,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<TradeTick>> {
    let trade: Trade = serde_json::from_value(value)?;
    if trade.is_block_trade {
        return Ok(None);
    }

    if let Some(no_price) = trade.no_price_dollars {
        anyhow::ensure!(
            trade.yes_price_dollars.0.checked_add(no_price.0) == Some(Decimal::ONE),
            "inconsistent trade prices"
        );
    }

    let aggressor_side = match (trade.taker_book_side, trade.taker_outcome_side) {
        (Some(BookSide::Bid), Some(OutcomeSide::Yes) | None) | (None, Some(OutcomeSide::Yes)) => {
            AggressorSide::Buy
        }
        (Some(BookSide::Ask), Some(OutcomeSide::No) | None) | (None, Some(OutcomeSide::No)) => {
            AggressorSide::Sell
        }
        _ => anyhow::bail!("missing or inconsistent canonical taker direction"),
    };

    let price = price(trade.yes_price_dollars, instrument.price_precision)?;
    let size = size(trade.count_fp, instrument.size_precision)?;
    let trade_id = TradeId::new_checked(trade.trade_id)?;
    let ts_event = timestamp(trade.ts_ms, trade.ts)?;
    let tick = TradeTick::new_checked(
        instrument.id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )?;
    Ok(Some(tick))
}

/// Builds one quote tick from a ticker payload.
///
/// Returns `Ok(None)` when either side lacks a price or has a zero size. A negative size is an
/// error.
///
/// # Errors
///
/// Returns an error if the payload is malformed, the quote is crossed, a price or size is outside
/// its exact domain, or the timestamp is missing or invalid.
pub fn quote(
    value: Value,
    instrument: &BinaryOption,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    let quote: Ticker = serde_json::from_value(value)?;

    let (Some(bid), Some(ask), Some(bid_size), Some(ask_size)) = (
        quote.yes_bid_dollars,
        quote.yes_ask_dollars,
        quote.yes_bid_size_fp,
        quote.yes_ask_size_fp,
    ) else {
        return Ok(None);
    };

    if bid_size.0.is_zero() || ask_size.0.is_zero() {
        return Ok(None);
    }

    anyhow::ensure!(bid.0 <= ask.0, "crossed ticker quote");
    let bid_price = price(bid, instrument.price_precision)?;
    let ask_price = price(ask, instrument.price_precision)?;
    let bid_size = size(bid_size, instrument.size_precision)?;
    let ask_size = size(ask_size, instrument.size_precision)?;
    let ts_event = timestamp(quote.ts_ms, quote.ts)?;
    let tick = QuoteTick::new_checked(
        instrument.id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )?;
    Ok(Some(tick))
}

pub(crate) fn price(value: FixedPoint, precision: u8) -> anyhow::Result<Price> {
    anyhow::ensure!(
        value.0 >= Decimal::ZERO && value.0 <= Decimal::ONE,
        "price outside binary payout interval"
    );
    let price = Price::from_decimal_dp(value.0, precision)?;
    anyhow::ensure!(
        price.as_decimal() == value.0,
        "price exceeds instrument precision"
    );
    Ok(price)
}

pub(crate) fn size(value: FixedPoint, precision: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(value.0 > Decimal::ZERO, "size must be positive");
    let size = Quantity::from_decimal_dp(value.0, precision)?;
    anyhow::ensure!(
        size.as_decimal() == value.0,
        "size exceeds instrument precision"
    );
    Ok(size)
}

fn timestamp(millis: Option<u64>, seconds: Option<u64>) -> anyhow::Result<UnixNanos> {
    let nanos = match millis {
        Some(value) => value.checked_mul(1_000_000),
        None => seconds.and_then(|value| value.checked_mul(1_000_000_000)),
    }
    .context("missing or invalid venue timestamp")?;

    Ok(UnixNanos::from(nanos))
}

#[derive(Deserialize)]
struct Trade {
    trade_id: String,
    yes_price_dollars: FixedPoint,
    no_price_dollars: Option<FixedPoint>,
    count_fp: FixedPoint,
    taker_book_side: Option<BookSide>,
    taker_outcome_side: Option<OutcomeSide>,
    #[serde(default)]
    is_block_trade: bool,
    ts_ms: Option<u64>,
    ts: Option<u64>,
}

#[derive(Deserialize)]
struct Ticker {
    yes_bid_dollars: Option<FixedPoint>,
    yes_ask_dollars: Option<FixedPoint>,
    yes_bid_size_fp: Option<FixedPoint>,
    yes_ask_size_fp: Option<FixedPoint>,
    ts_ms: Option<u64>,
    ts: Option<u64>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::{
        models::{Event, Market},
        parse::parse_instrument,
    };

    #[rstest]
    #[case(Some("bid"), Some("yes"), AggressorSide::Buy)]
    #[case(Some("ask"), Some("no"), AggressorSide::Sell)]
    #[case(None, Some("yes"), AggressorSide::Buy)]
    #[case(Some("ask"), None, AggressorSide::Sell)]
    fn trade_preserves_exact_fields(
        #[case] book: Option<&str>,
        #[case] outcome: Option<&str>,
        #[case] side: AggressorSide,
    ) {
        let instrument = instrument();
        let mut value = frame("trade");
        value["taker_book_side"] = book.into();
        value["taker_outcome_side"] = outcome.into();
        let tick = trade(value, &instrument, UnixNanos::from(987))
            .unwrap()
            .unwrap();
        assert_eq!(
            tick,
            TradeTick::new_checked(
                instrument.id,
                Price::from_decimal_dp("0.123456".parse().unwrap(), 6).unwrap(),
                Quantity::from_decimal_dp("7.25".parse().unwrap(), 2).unwrap(),
                side,
                TradeId::new("e8cb18d4-8b99-40d3-9fce-cc0e33422e67"),
                UnixNanos::from(1_700_000_123_456_000_000),
                UnixNanos::from(987)
            )
            .unwrap()
        );
    }

    #[rstest]
    fn quote_preserves_every_field_without_last_price_or_complement() {
        let instrument = instrument();
        let tick = quote(frame("ticker"), &instrument, UnixNanos::from(654))
            .unwrap()
            .unwrap();
        assert_eq!(
            tick,
            QuoteTick::new_checked(
                instrument.id,
                Price::from_decimal_dp("0.123456".parse().unwrap(), 6).unwrap(),
                Price::from_decimal_dp("0.789123".parse().unwrap(), 6).unwrap(),
                Quantity::from_decimal_dp("3.25".parse().unwrap(), 2).unwrap(),
                Quantity::from_decimal_dp("6.75".parse().unwrap(), 2).unwrap(),
                UnixNanos::from(1_700_000_123_789_000_000),
                UnixNanos::from(654)
            )
            .unwrap()
        );
    }

    #[rstest]
    #[case("yes_bid_dollars")]
    #[case("yes_ask_dollars")]
    #[case("yes_bid_size_fp")]
    #[case("yes_ask_size_fp")]
    fn incomplete_ticker_never_invents_a_side(#[case] field: &str) {
        let mut value = frame("ticker");
        value.as_object_mut().unwrap().remove(field);
        assert_eq!(
            quote(value, &instrument(), UnixNanos::from(1)).unwrap(),
            None
        );
    }

    #[rstest]
    #[case("yes_bid_size_fp")]
    #[case("yes_ask_size_fp")]
    fn empty_side_is_not_a_complete_quote(#[case] field: &str) {
        let mut value = frame("ticker");
        value[field] = "0.00".into();
        assert_eq!(
            quote(value, &instrument(), UnixNanos::from(1)).unwrap(),
            None
        );
    }

    #[rstest]
    #[case("yes_price_dollars", "0.1234567")]
    #[case("yes_price_dollars", "1.010000")]
    #[case("count_fp", "7.251")]
    #[case("count_fp", "-1.00")]
    #[case("count_fp", "0.00")]
    #[case("taker_book_side", "bid")]
    #[case("taker_outcome_side", "yes")]
    #[case("no_price_dollars", "0.100000")]
    #[case("trade_id", "")]
    fn invalid_trade_is_rejected(#[case] field: &str, #[case] invalid: &str) {
        let mut value = frame("trade");
        value[field] = invalid.into();
        if field == "yes_price_dollars" {
            value.as_object_mut().unwrap().remove("no_price_dollars");
        }

        assert!(trade(value, &instrument(), UnixNanos::from(1)).is_err());
    }

    #[rstest]
    #[case(Decimal::MAX)]
    #[case(Decimal::MIN)]
    fn overflowing_trade_prices_are_rejected_without_panicking(#[case] price: Decimal) {
        let mut value = frame("trade");
        value["yes_price_dollars"] = price.to_string().into();
        value["no_price_dollars"] = price.to_string().into();

        assert_eq!(
            trade(value, &instrument(), UnixNanos::from(1))
                .unwrap_err()
                .to_string(),
            "inconsistent trade prices"
        );
    }

    #[rstest]
    #[case("yes_bid_dollars", "0.1234567")]
    #[case("yes_ask_dollars", "0.100000")]
    #[case("yes_bid_size_fp", "3.251")]
    #[case("yes_ask_size_fp", "-1.00")]
    fn invalid_quote_is_rejected(#[case] field: &str, #[case] invalid: &str) {
        let mut value = frame("ticker");
        value[field] = invalid.into();
        assert!(quote(value, &instrument(), UnixNanos::from(1)).is_err());
    }

    #[rstest]
    fn legacy_direction_is_not_a_fallback_and_block_trades_are_excluded() {
        let mut value = frame("trade");
        value.as_object_mut().unwrap().remove("taker_book_side");
        value.as_object_mut().unwrap().remove("taker_outcome_side");
        value["taker_side"] = "yes".into();
        assert!(trade(value, &instrument(), UnixNanos::from(1)).is_err());
        let mut value = frame("trade");
        value["is_block_trade"] = true.into();
        assert_eq!(
            trade(value, &instrument(), UnixNanos::from(1)).unwrap(),
            None
        );
    }

    #[rstest]
    fn timestamps_require_venue_evidence_and_reject_overflow() {
        assert_eq!(
            timestamp(None, Some(7)).unwrap(),
            UnixNanos::from(7_000_000_000)
        );
        assert!(timestamp(None, None).is_err());
        assert!(timestamp(Some(u64::MAX), Some(7)).is_err());
    }

    fn frame(name: &str) -> Value {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/market_data.json")).unwrap();
        fixtures[name].clone()
    }

    fn instrument() -> BinaryOption {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
    }
}

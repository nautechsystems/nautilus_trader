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

//! Checked conversions with exact prices and raw sizes, and bounded OHLCV volume normalization.

use std::collections::HashSet;

use anyhow::Context;
use jiff::{Span, Timestamp, tz::TimeZone};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, QuoteTick, TradeTick},
    enums::{AggressorSide, BookType, OrderSide},
    identifiers::TradeId,
    instruments::BinaryOption,
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::{
    common::symbol::from_instrument_id,
    http::models::{Book, Ohlcv, OhlcvPeriod, Quote, Trade, TradeSide},
};

pub(super) fn quote(source: &Quote, instrument: &BinaryOption) -> anyhow::Result<QuoteTick> {
    let bid = price(source.bid, instrument)?;
    let ask = price(source.ask, instrument)?;
    let bid_qty = qty(source.bid_volume, instrument)?;
    let ask_qty = qty(source.ask_volume, instrument)?;
    let ts_event = timestamp(source.entry_time)?;
    let ts_init = timestamp(source.recv_time)?;
    QuoteTick::new_checked(instrument.id, bid, ask, bid_qty, ask_qty, ts_event, ts_init)
}

pub(super) fn trade(source: &Trade, instrument: &BinaryOption) -> anyhow::Result<TradeTick> {
    anyhow::ensure!(
        source.market_id.as_deref() == Some(instrument.id.symbol.as_str()),
        "FinFeed trade market identity mismatch"
    );

    if let Some(outcome) = &source.outcome {
        anyhow::ensure!(
            instrument
                .outcome
                .as_ref()
                .is_some_and(|expected| expected.as_str() == outcome),
            "FinFeed trade outcome mismatch"
        );
    }

    let id = source
        .id
        .as_deref()
        .context("FinFeed trade identifier is missing")?;
    let trade_id = TradeId::new_checked(id)?;
    let value = price(source.price, instrument)?;
    let size = qty(source.quantity, instrument)?;
    let ts_event = timestamp(source.timestamp)?;

    if source.side == TradeSide::Other {
        log::warn!(
            "Unknown FinFeed trade side for {}, trade_id={id}, no aggressor-side inference",
            instrument.id
        );
    }

    TradeTick::new_checked(
        instrument.id,
        value,
        size,
        AggressorSide::NoAggressor,
        trade_id,
        ts_event,
        ts_event,
    )
}

pub(super) fn bar(
    source: &Ohlcv,
    bar_type: BarType,
    instrument: &BinaryOption,
    period: &OhlcvPeriod,
) -> anyhow::Result<Option<Bar>> {
    if source.trades_count == 0
        && source.volume_traded.is_zero()
        && [
            source.price_open,
            source.price_high,
            source.price_low,
            source.price_close,
        ]
        .iter()
        .all(Option::is_none)
    {
        return Ok(None);
    }

    let expected_end = source
        .time_period_start
        .to_zoned(TimeZone::UTC)
        .checked_add(
            Span::new()
                .seconds(period.length_seconds)
                .months(period.length_months),
        )?
        .timestamp();
    anyhow::ensure!(
        source.time_period_end == expected_end,
        "FinFeed OHLCV period does not match requested bar specification"
    );
    anyhow::ensure!(
        source.time_period_start < source.time_period_end,
        "invalid FinFeed OHLCV period"
    );
    anyhow::ensure!(
        source.trades_count >= 0,
        "negative FinFeed OHLCV trade count"
    );

    for time in [source.time_open, source.time_close].into_iter().flatten() {
        anyhow::ensure!(
            time >= source.time_period_start && time <= source.time_period_end,
            "FinFeed OHLCV activity time is outside period"
        );
    }

    anyhow::ensure!(
        !source
            .time_open
            .zip(source.time_close)
            .is_some_and(|(open, close)| open > close),
        "FinFeed OHLCV activity times are reversed"
    );
    let open = price(
        source.price_open.context("FinFeed OHLCV open is missing")?,
        instrument,
    )?;
    let high = price(
        source.price_high.context("FinFeed OHLCV high is missing")?,
        instrument,
    )?;
    let low = price(
        source.price_low.context("FinFeed OHLCV low is missing")?,
        instrument,
    )?;
    let close = price(
        source
            .price_close
            .context("FinFeed OHLCV close is missing")?,
        instrument,
    )?;
    let volume = volume(source.volume_traded, instrument)?;
    let ts_event = timestamp(source.time_period_end)?;
    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_event).map(Some)
}

pub(super) fn book(
    source: &Book,
    instrument: &BinaryOption,
    depth: Option<usize>,
) -> anyhow::Result<OrderBook> {
    let (exchange_id, market_id) = from_instrument_id(instrument.id)?;
    anyhow::ensure!(
        source.exchange_id.as_deref() == Some(exchange_id.as_str())
            && source.market_id.as_deref() == Some(market_id.as_str()),
        "FinFeed book identity mismatch"
    );
    let ts_event = timestamp(source.time_exchange)?;
    timestamp(source.time_coinapi)?;
    let mut orders = Vec::new();

    for (side, levels) in [
        (OrderSide::Buy, &source.bids),
        (OrderSide::Sell, &source.asks),
    ] {
        let mut prices = HashSet::new();

        for level in levels.as_ref().context("FinFeed book side is missing")? {
            let value = price(level.price, instrument)?;
            let size = qty(level.size, instrument)?;
            anyhow::ensure!(
                size.as_decimal() > Decimal::ZERO,
                "FinFeed snapshot level size must be positive"
            );
            anyhow::ensure!(
                prices.insert(value),
                "duplicate FinFeed snapshot price level"
            );
            orders.push(BookOrder::new(side, value, size, 0));
        }
    }

    let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
    book.clear(0, ts_event);
    for order in orders {
        book.add(order, 0, 0, ts_event);
    }

    book_check_integrity(&book)?;

    if let Some(depth) = depth {
        book = limit_depth(&book, depth);
    }

    Ok(book)
}

pub(super) fn limit_depth(book: &OrderBook, depth: usize) -> OrderBook {
    let mut limited = OrderBook::new(book.instrument_id, book.book_type);
    limited.clear(0, book.ts_last);

    for level in book.bids(Some(depth)).chain(book.asks(Some(depth))) {
        for order in level.iter() {
            limited.add(*order, 0, 0, book.ts_last);
        }
    }

    limited
}

pub(super) fn timestamp(value: Timestamp) -> anyhow::Result<UnixNanos> {
    let value = u64::try_from(value.as_nanosecond())
        .context("FinFeed timestamp is outside supported nanoseconds")?;
    Ok(UnixNanos::from(value))
}

pub(super) fn price(value: Decimal, instrument: &BinaryOption) -> anyhow::Result<Price> {
    anyhow::ensure!(
        value >= Decimal::ZERO && value <= Decimal::ONE,
        "FinFeed prediction price must be in [0, 1]"
    );
    let price = Price::from_decimal_dp(value, instrument.price_precision)?;
    anyhow::ensure!(
        price.as_decimal() == value,
        "FinFeed price requires rounding"
    );
    Ok(price)
}

pub(super) fn qty(value: Decimal, instrument: &BinaryOption) -> anyhow::Result<Quantity> {
    let qty = Quantity::from_decimal_dp(value, instrument.size_precision)?;
    anyhow::ensure!(qty.as_decimal() == value, "FinFeed size requires rounding");
    Ok(qty)
}

fn volume(value: Decimal, instrument: &BinaryOption) -> anyhow::Result<Quantity> {
    const RELATIVE_SCALE: i64 = 1_000_000_000_000_000;

    let precision = u32::from(instrument.size_precision);
    let rounded = value.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven);

    if rounded != value {
        anyhow::ensure!(
            value > Decimal::ZERO
                && value < Decimal::new(RELATIVE_SCALE / 2, precision)
                && (value - rounded).abs() * Decimal::new(RELATIVE_SCALE, 0) <= value,
            "FinFeed OHLCV volume requires rounding"
        );
    }

    qty(rounded, instrument)
}

#[cfg(test)]
mod tests {
    use nautilus_model::orderbook::BookIntegrityError;
    use rstest::rstest;

    use super::*;
    use crate::http::parse::parse_instrument;

    #[rstest]
    #[case(None)]
    #[case(Some(1))]
    #[case(Some(10))]
    fn locked_books_remain_valid_and_crossed_books_fail_before_depth(#[case] depth: Option<usize>) {
        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        let mut source: Book =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        source.bids.as_mut().unwrap()[2].price = Decimal::new(4, 1);
        let locked = book(&source, &instrument, depth).unwrap();
        source.bids.as_mut().unwrap()[2].price = Decimal::new(6, 1);
        let crossed = book(&source, &instrument, depth).unwrap_err();

        assert_eq!(
            locked.best_bid_price().unwrap().as_decimal(),
            Decimal::new(4, 1)
        );
        assert_eq!(
            locked.best_ask_price().unwrap().as_decimal(),
            Decimal::new(4, 1)
        );
        assert_eq!(locked.bids_as_map(None).len(), depth.unwrap_or(3).min(3));
        assert_eq!(locked.asks_as_map(None).len(), depth.unwrap_or(2).min(2));
        assert!(matches!(
            crossed.downcast_ref::<BookIntegrityError>(),
            Some(BookIntegrityError::OrdersCrossed(_, _))
        ));
    }

    #[rstest]
    fn bars_normalize_provider_volume_tokens_and_keep_prices_exact() {
        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/ohlcv-volume-noise.json")).unwrap();
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
        let period = periods
            .iter()
            .find(|period| period.period_id.as_ref().map(AsRef::as_ref) == Some("1MIN"))
            .unwrap();
        let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id).as_str());
        let expected = [
            "28372.630000",
            "1242.046344",
            "24146.200195",
            "9.827369",
            "3601.435903",
            "1118.515791",
            "10991.426318",
        ];
        assert_eq!(records.len(), expected.len());

        for (record, expected) in records.iter().zip(expected) {
            let actual = bar(record, bar_type, &instrument, period).unwrap().unwrap();
            let expected = Bar::new_checked(
                bar_type,
                Price::from("0.2000"),
                Price::from("0.4000"),
                Price::from("0.1000"),
                Price::from("0.3000"),
                Quantity::from(expected),
                UnixNanos::from(1_602_374_520_000_000_000),
                UnixNanos::from(1_602_374_520_000_000_000),
            )
            .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.volume.precision, 6);
            assert_eq!(
                qty(record.volume_traded, &instrument)
                    .unwrap_err()
                    .to_string(),
                "FinFeed size requires rounding"
            );
        }

        let mut finer_price = records[0].clone();
        finer_price.price_open = Some(Decimal::new(20001, 5));
        assert_eq!(
            bar(&finer_price, bar_type, &instrument, period)
                .unwrap_err()
                .to_string(),
            "FinFeed price requires rounding"
        );
    }

    #[rstest]
    #[case("28372.630000000028", Some("28372.630000"))]
    #[case("28372.630000000029", None)]
    #[case("10000.00000000001", Some("10000.000000"))]
    #[case("9999.99999999999", None)]
    #[case("499999999.0000004", Some("499999999.000000"))]
    #[case("500000000.0000001", None)]
    #[case("500000000.000001", Some("500000000.000001"))]
    #[case("12.3456785", None)]
    #[case("12.3456781", None)]
    #[case("0.0000004", None)]
    #[case("0.00000000000000000001", None)]
    #[case("-0.0000000000000001", None)]
    #[case("0", Some("0.000000"))]
    #[case("12.345678", Some("12.345678"))]
    fn volume_normalization_enforces_relative_and_size_unit_bounds(
        #[case] source: &str,
        #[case] expected: Option<&str>,
    ) {
        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        let result = volume(Decimal::from_str_exact(source).unwrap(), &instrument);
        if let Some(expected) = expected {
            let actual = result.unwrap();
            assert_eq!(actual, Quantity::from(expected));
            assert_eq!(actual.precision, 6);
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "FinFeed OHLCV volume requires rounding"
            );
        }
    }

    #[rstest]
    #[case("quote")]
    #[case("trade")]
    #[case("book")]
    fn raw_sizes_stay_exact_despite_bar_volume_normalization(#[case] kind: &str) {
        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        let size = Decimal::from_str_exact("3601.4359029999996").unwrap();

        let result = match kind {
            "quote" => {
                let mut rows: Vec<Quote> =
                    serde_json::from_str(include_str!("../../test_data/client-quotes.json"))
                        .unwrap();
                rows[0].bid_volume = size;
                quote(&rows[0], &instrument).map(|_| ())
            }
            "trade" => {
                let mut rows: Vec<Trade> =
                    serde_json::from_str(include_str!("../../test_data/client-trades.json"))
                        .unwrap();
                rows[0].quantity = size;
                trade(&rows[0], &instrument).map(|_| ())
            }
            "book" => {
                let mut row: Book =
                    serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
                row.bids.as_mut().unwrap()[0].size = size;
                book(&row, &instrument, None).map(|_| ())
            }
            _ => unreachable!(),
        };

        assert_eq!(
            result.unwrap_err().to_string(),
            "FinFeed size requires rounding"
        );
    }
}

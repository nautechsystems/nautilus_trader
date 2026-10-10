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

//! Conversion of CoinAPI WebSocket messages into Nautilus market data.
//!
//! Instrument precisions come from CoinAPI data resolutions, which can lag the stream. A value
//! with more decimal places than its instrument precision is rejected, except for bounded
//! numeric tails in OHLCV volume.

use jiff::Timestamp;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth,
        QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, RoundingStrategy};
use ustr::Ustr;

use super::messages::{Book, Ohlcv, Quote, Trade};
use crate::http::{models::BookLevel, parse::unix_nanos};

/// Converts a full L2 `book` snapshot or update into deltas, taking `ts_event` from
/// `time_exchange` and `ts_init` from `time_coinapi`.
///
/// A snapshot becomes a `Clear` followed by an `Add` per level, every delta flagged `F_SNAPSHOT`;
/// an empty snapshot is a lone `Clear`. An update sets each listed level to its new size, and a
/// zero size removes the level. The last delta carries `F_LAST`, and an update listing no levels
/// returns `None`.
///
/// # Errors
///
/// Returns an error if a level is finer than the instrument precision, a snapshot level has zero
/// size, or a timestamp is invalid; a single invalid level rejects the whole message.
pub fn parse_book_deltas(
    book: &Book,
    instrument: &InstrumentAny,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let instrument_id = instrument.id();
    let ts_event = timestamp(book.time_exchange, "time_exchange")?;
    let ts_init = timestamp(book.time_coinapi, "time_coinapi")?;
    let is_snapshot = book.is_snapshot == Some(true);
    let mut deltas = Vec::with_capacity(book.bids.len() + book.asks.len() + 1);
    let flags = if is_snapshot {
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            book.sequence,
            ts_event,
            ts_init,
        ));
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    for (side, level) in levels(book) {
        let order = book_order(side, level, instrument)?;

        let action = if is_snapshot {
            BookAction::Add
        } else if order.size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            book.sequence,
            ts_event,
            ts_init,
        )?);
    }

    let Some(last) = deltas.last_mut() else {
        return Ok(None);
    };

    last.flags |= RecordFlag::F_LAST as u8;

    Ok(Some(OrderBookDeltas::new(instrument_id, deltas)))
}

/// Converts a fixed-depth `book5`, `book20`, or `book50` message into its best `depth` levels
/// per side, taking `ts_event` from `time_exchange` and `ts_init` from `time_coinapi`.
///
/// CoinAPI reports no order counts, so every level count is zero.
///
/// # Errors
///
/// Returns an error if a kept level is finer than the instrument precision or a timestamp is
/// invalid.
pub fn parse_book_depth(
    book: &Book,
    depth: usize,
    instrument: &InstrumentAny,
) -> anyhow::Result<OrderBookDepth> {
    let side = |side: OrderSide, levels: &[BookLevel]| {
        levels
            .iter()
            .take(depth)
            .map(|level| book_order(side, level, instrument))
            .collect::<anyhow::Result<Vec<BookOrder>>>()
    };

    let bids = side(OrderSide::Buy, &book.bids)?;
    let asks = side(OrderSide::Sell, &book.asks)?;
    let bid_counts = vec![0; bids.len()];
    let ask_counts = vec![0; asks.len()];

    OrderBookDepth::new_checked(
        instrument.id(),
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        book.sequence,
        timestamp(book.time_exchange, "time_exchange")?,
        timestamp(book.time_coinapi, "time_coinapi")?,
    )
}

/// Converts a best bid and ask, taking `ts_event` from `time_exchange` and `ts_init` from
/// `time_coinapi`.
///
/// # Errors
///
/// Returns an error if a value is finer than the instrument precision or a timestamp is invalid.
pub fn parse_quote_tick(quote: &Quote, instrument: &InstrumentAny) -> anyhow::Result<QuoteTick> {
    QuoteTick::new_checked(
        instrument.id(),
        price(quote.bid_price, instrument, "bid_price")?,
        price(quote.ask_price, instrument, "ask_price")?,
        quantity(quote.bid_size, instrument, "bid_size")?,
        quantity(quote.ask_size, instrument, "ask_size")?,
        timestamp(quote.time_exchange, "time_exchange")?,
        timestamp(quote.time_coinapi, "time_coinapi")?,
    )
}

/// Converts a trade, taking `ts_event` from `time_exchange` and `ts_init` from `time_coinapi`.
///
/// Only exchange-reported taker sides set the aggressor; CoinAPI estimates and unknown sides
/// become [`AggressorSide::NoAggressor`].
///
/// # Errors
///
/// Returns an error if a value is finer than the instrument precision, the size is not positive, or
/// a timestamp or the trade identifier is invalid.
pub fn parse_trade_tick(trade: &Trade, instrument: &InstrumentAny) -> anyhow::Result<TradeTick> {
    TradeTick::new_checked(
        instrument.id(),
        price(trade.price, instrument, "price")?,
        quantity(trade.size, instrument, "size")?,
        parse_aggressor_side(trade.taker_side),
        TradeId::new_checked(&trade.uuid)?,
        timestamp(trade.time_exchange, "time_exchange")?,
        timestamp(trade.time_coinapi, "time_coinapi")?,
    )
}

/// Converts a completed OHLCV period into an external last-price bar closing at
/// `time_period_end`.
///
/// Rounds volume to the instrument's size precision when the adjustment is at most both
/// `1e-15` times the reported volume and `1e-6` times one unit at that precision.
///
/// # Errors
///
/// Returns an error if the period is unsupported, a price is finer than the instrument precision,
/// volume needs rounding beyond the noise tolerance, the prices are inconsistent, or a timestamp is
/// invalid.
pub fn parse_bar(
    ohlcv: &Ohlcv,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let spec = period_id_to_bar_spec(ohlcv.period_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported period_id {}", ohlcv.period_id))?;

    Bar::new_checked(
        BarType::new(instrument.id(), spec, AggregationSource::External),
        price(ohlcv.price_open, instrument, "price_open")?,
        price(ohlcv.price_high, instrument, "price_high")?,
        price(ohlcv.price_low, instrument, "price_low")?,
        price(ohlcv.price_close, instrument, "price_close")?,
        volume(ohlcv.volume_traded, instrument)?,
        timestamp(ohlcv.time_period_end, "time_period_end")?,
        ts_init,
    )
}

/// Maps an exchange taker side to the aggressor side.
#[must_use]
pub fn parse_aggressor_side(taker_side: Ustr) -> AggressorSide {
    match taker_side.as_str() {
        "BUY" => AggressorSide::Buy,
        "SELL" => AggressorSide::Sell,
        "BUY_ESTIMATED" | "SELL_ESTIMATED" | "UNKNOWN" => AggressorSide::NoAggressor,
        other => {
            log::warn!("Unknown CoinAPI taker_side {other}, using no aggressor");
            AggressorSide::NoAggressor
        }
    }
}

/// Returns the CoinAPI streaming `period_id` for a last-price bar specification.
///
/// CoinAPI streams OHLCV periods from one second to one day.
///
/// # Errors
///
/// Returns an error if the price type is not last or CoinAPI has no matching period.
pub fn bar_spec_to_period_id(spec: BarSpecification) -> anyhow::Result<Ustr> {
    anyhow::ensure!(
        spec.price_type == PriceType::Last,
        "CoinAPI OHLCV bars use last prices, was {}",
        spec.price_type
    );

    let step = spec.step.get();
    let unit = match spec.aggregation {
        BarAggregation::Second if SUB_HOUR_STEPS.contains(&step) => "SEC",
        BarAggregation::Minute if SUB_HOUR_STEPS.contains(&step) => "MIN",
        BarAggregation::Hour if HOUR_STEPS.contains(&step) => "HRS",
        BarAggregation::Day if step == 1 => "DAY",
        _ => anyhow::bail!("CoinAPI has no streaming OHLCV period for {spec}"),
    };

    Ok(Ustr::from(&format!("{step}{unit}")))
}

/// Returns the last-price bar specification for a CoinAPI streaming `period_id`.
#[must_use]
pub fn period_id_to_bar_spec(period_id: Ustr) -> Option<BarSpecification> {
    let period = period_id.as_str();
    let split = period.find(|c: char| !c.is_ascii_digit())?;
    let (step, unit) = period.split_at(split);

    let aggregation = match unit {
        "SEC" => BarAggregation::Second,
        "MIN" => BarAggregation::Minute,
        "HRS" => BarAggregation::Hour,
        "DAY" => BarAggregation::Day,
        _ => return None,
    };

    let spec =
        BarSpecification::new_checked(step.parse().ok()?, aggregation, PriceType::Last).ok()?;

    // Rejects leading zeros and steps CoinAPI does not stream
    (bar_spec_to_period_id(spec).ok()? == period_id).then_some(spec)
}

fn levels(book: &Book) -> impl Iterator<Item = (OrderSide, &BookLevel)> {
    let bids = book.bids.iter().map(|level| (OrderSide::Buy, level));
    let asks = book.asks.iter().map(|level| (OrderSide::Sell, level));
    bids.chain(asks)
}

fn book_order(
    side: OrderSide,
    level: &BookLevel,
    instrument: &InstrumentAny,
) -> anyhow::Result<BookOrder> {
    Ok(BookOrder::new(
        side,
        price(level.price, instrument, "price")?,
        quantity(level.size, instrument, "size")?,
        0,
    ))
}

const SUB_HOUR_STEPS: [usize; 10] = [1, 2, 3, 4, 5, 6, 10, 15, 20, 30];
const HOUR_STEPS: [usize; 7] = [1, 2, 3, 4, 6, 8, 12];

pub(crate) fn price(
    value: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> anyhow::Result<Price> {
    price_at(value, instrument.price_precision(), field)
}

pub(crate) fn quantity(
    value: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> anyhow::Result<Quantity> {
    quantity_at(value, instrument.size_precision(), field)
}

pub(crate) fn volume(value: Decimal, instrument: &InstrumentAny) -> anyhow::Result<Quantity> {
    let precision = u32::from(instrument.size_precision());
    let rounded = value.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven);
    let adjustment = (value - rounded).abs();

    // The size-unit cap rules out meaningful rounding even for coarse resolutions. Check it
    // before scaling the adjustment, which keeps the relative comparison exact and bounded.
    let normalized = if value >= Decimal::ZERO
        && adjustment <= Decimal::new(1, precision + 6)
        && adjustment * Decimal::new(1_000_000_000_000_000, 0) <= value
    {
        rounded
    } else {
        value
    };

    quantity_at(normalized, instrument.size_precision(), "volume_traded")
}

/// Converts `value` at `precision`, rejecting a value that would need rounding.
pub(crate) fn price_at(value: Decimal, precision: u8, field: &str) -> anyhow::Result<Price> {
    check_scale(value, precision, field)?;
    Ok(Price::from_decimal_dp(value, precision)?)
}

/// Converts `value` at `precision`, rejecting a value that would need rounding.
pub(crate) fn quantity_at(value: Decimal, precision: u8, field: &str) -> anyhow::Result<Quantity> {
    check_scale(value, precision, field)?;
    Ok(Quantity::from_decimal_dp(value, precision)?)
}

fn check_scale(value: Decimal, precision: u8, field: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.normalize().scale() <= u32::from(precision),
        "{field} {value} is finer than instrument precision {precision}"
    );
    Ok(())
}

pub(crate) fn timestamp(value: Timestamp, field: &str) -> anyhow::Result<UnixNanos> {
    unix_nanos(value).map_err(|e| anyhow::anyhow!("invalid {field}: {e}"))
}

#[cfg(test)]
mod tests {
    use nautilus_model::{identifiers::InstrumentId, instruments::CurrencyPair, types::Currency};
    use rstest::rstest;

    use super::*;

    fn instrument(price_precision: u8, size_precision: u8) -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from("BTC-USD.COINBASE"))
                .raw_symbol("BTC-USD".into())
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .price_precision(price_precision)
                .size_precision(size_precision)
                .price_increment(Price::new(
                    10f64.powi(-i32::from(price_precision)),
                    price_precision,
                ))
                .size_increment(Quantity::new(
                    10f64.powi(-i32::from(size_precision)),
                    size_precision,
                ))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn quote() -> Quote {
        serde_json::from_str(include_str!("../../test_data/ws_quote.json")).unwrap()
    }

    fn trade() -> Trade {
        serde_json::from_str(include_str!("../../test_data/ws_trade.json")).unwrap()
    }

    fn bar_spec(spec: &str) -> BarSpecification {
        BarType::from(format!("BTC-USD.COINBASE-{spec}-EXTERNAL").as_str()).spec()
    }

    fn ohlcv() -> Ohlcv {
        serde_json::from_str(include_str!("../../test_data/ws_ohlcv.json")).unwrap()
    }

    #[rstest]
    fn quote_maps_every_field_and_ignores_trailing_zero_padding() {
        let tick = parse_quote_tick(&quote(), &instrument(2, 0)).unwrap();

        assert_eq!(
            tick,
            QuoteTick::new(
                InstrumentId::from("BTC-USD.COINBASE"),
                Price::from("760.00"),
                Price::from("770.00"),
                Quantity::from("124"),
                Quantity::from("3252"),
                UnixNanos::from(1_380_408_050_000_000_000),
                UnixNanos::from(1_489_876_941_376_334_200),
            )
        );
    }

    #[rstest]
    fn trade_maps_every_field_and_both_timestamps() {
        let tick = parse_trade_tick(&trade(), &instrument(2, 8)).unwrap();

        assert_eq!(
            tick,
            TradeTick::new(
                InstrumentId::from("BTC-USD.COINBASE"),
                Price::from("84520.02"),
                Quantity::from("0.00000004"),
                AggressorSide::Sell,
                TradeId::new("c2519dfb-79a6-40e9-a1c8-a95d94209399"),
                UnixNanos::from(1_790_299_746_065_618_000),
                UnixNanos::from(1_790_299_746_143_827_200),
            )
        );
    }

    #[rstest]
    fn bar_closes_at_period_end_with_given_ts_init() {
        let ts_init = UnixNanos::from(1_790_299_803_000_000_000);
        let bar = parse_bar(&ohlcv(), &instrument(2, 8), ts_init).unwrap();

        assert_eq!(
            bar,
            Bar::new(
                BarType::from("BTC-USD.COINBASE-1-SECOND-LAST-EXTERNAL"),
                Price::from("84523.69"),
                Price::from("84523.69"),
                Price::from("84523.68"),
                Price::from("84523.69"),
                Quantity::from("0.00468951"),
                UnixNanos::from(1_790_299_802_000_000_000),
                ts_init,
            )
        );
    }

    #[rstest]
    #[case("7.087936829999999", 8, "7.08793683")]
    #[case("1.2582318399999999", 8, "1.25823184")]
    #[case("28119.500000000004", 1, "28119.5")]
    #[case("0.9707556800000001", 8, "0.97075568")]
    #[case("1.000000000000001", 8, "1.00000000")]
    #[case("100.00000000000001", 8, "100.00000000")]
    #[case("0.000000001", 9, "0.000000001")]
    #[case("0", 8, "0.00000000")]
    fn bar_normalizes_volume_noise(
        #[case] volume: &str,
        #[case] size_precision: u8,
        #[case] expected_volume: &str,
    ) {
        let mut ohlcv = ohlcv();
        ohlcv.volume_traded = Decimal::from_str_exact(volume).unwrap();
        let ts_init = UnixNanos::from(1_790_299_803_000_000_000);

        let bar = parse_bar(&ohlcv, &instrument(2, size_precision), ts_init).unwrap();

        assert_eq!(
            bar,
            Bar::new(
                BarType::from("BTC-USD.COINBASE-1-SECOND-LAST-EXTERNAL"),
                Price::from("84523.69"),
                Price::from("84523.69"),
                Price::from("84523.68"),
                Price::from("84523.69"),
                Quantity::from(expected_volume),
                UnixNanos::from(1_790_299_802_000_000_000),
                ts_init,
            )
        );
        assert_eq!(bar.volume.precision, size_precision);
    }

    #[rstest]
    #[case("1.000000000000002")]
    #[case("100.00000000000002")]
    #[case("1000000000.000000001")]
    #[case("0.00000000000000000001")]
    #[case("-0.0000000000000001")]
    #[case("0.004689515")]
    #[case("0.999999999999999")]
    fn bar_rejects_volume_outside_noise_tolerance(#[case] volume: &str) {
        let mut ohlcv = ohlcv();
        ohlcv.volume_traded = Decimal::from_str_exact(volume).unwrap();

        let error = parse_bar(&ohlcv, &instrument(2, 8), UnixNanos::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("volume_traded {volume} is finer than instrument precision 8")
        );
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    #[case("0.0000000000000600000000000001", None)]
    #[case("0.0000000000000999999999999999", None)]
    #[case("0.0000000000001000000000000001", Some("0.0000000000001000"))]
    fn volume_relative_bound_at_high_precision(
        #[case] source: &str,
        #[case] expected: Option<&str>,
    ) {
        let result = volume(Decimal::from_str_exact(source).unwrap(), &instrument(2, 16));
        if let Some(expected) = expected {
            let quantity = result.unwrap();
            assert_eq!(quantity, Quantity::from(expected));
            assert_eq!(quantity.precision, 16);
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                format!("volume_traded {source} is finer than instrument precision 16")
            );
        }
    }

    #[rstest]
    fn quote_finer_than_instrument_precision_is_rejected() {
        let mut quote = quote();
        quote.bid_price = Decimal::from_str_exact("760.005").unwrap();

        let error = parse_quote_tick(&quote, &instrument(2, 0)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "bid_price 760.005 is finer than instrument precision 2"
        );
    }

    #[rstest]
    #[case(1, 8, "price 84520.02 is finer than instrument precision 1")]
    #[case(2, 7, "size 0.00000004 is finer than instrument precision 7")]
    fn trade_finer_than_instrument_precision_is_rejected(
        #[case] price_precision: u8,
        #[case] size_precision: u8,
        #[case] expected: &str,
    ) {
        let error =
            parse_trade_tick(&trade(), &instrument(price_precision, size_precision)).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn bar_finer_than_instrument_precision_is_rejected() {
        let error = parse_bar(&ohlcv(), &instrument(2, 7), UnixNanos::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "volume_traded 0.00468951 is finer than instrument precision 7"
        );
    }

    fn book(is_snapshot: Option<bool>, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> Book {
        let level = |(price, size): &(&str, &str)| BookLevel {
            id: None,
            price: Decimal::from_str_exact(price).unwrap(),
            size: Decimal::from_str_exact(size).unwrap(),
        };

        let mut book: Book =
            serde_json::from_str(include_str!("../../test_data/ws_book.json")).unwrap();
        book.is_snapshot = is_snapshot;
        book.bids = bids.iter().map(level).collect();
        book.asks = asks.iter().map(level).collect();
        book
    }

    fn delta(
        action: BookAction,
        side: OrderSide,
        price: &str,
        size: &str,
        flags: u8,
    ) -> OrderBookDelta {
        OrderBookDelta::new(
            InstrumentId::from("BTC-USD.COINBASE"),
            action,
            BookOrder::new(side, Price::from(price), Quantity::from(size), 0),
            flags,
            2_323_346,
            UnixNanos::from(1_380_408_050_000_000_000),
            UnixNanos::from(1_489_876_941_376_334_200),
        )
    }

    const SNAPSHOT: u8 = RecordFlag::F_SNAPSHOT as u8;
    const LAST: u8 = RecordFlag::F_LAST as u8;

    #[rstest]
    fn book_snapshot_clears_then_adds_every_level() {
        let deltas = parse_book_deltas(
            &book(
                Some(true),
                &[("456.10", "42")],
                &[("456.35", "123"), ("456.36", "23")],
            ),
            &instrument(2, 0),
        )
        .unwrap()
        .unwrap();

        let mut clear = OrderBookDelta::clear(
            InstrumentId::from("BTC-USD.COINBASE"),
            2_323_346,
            UnixNanos::from(1_380_408_050_000_000_000),
            UnixNanos::from(1_489_876_941_376_334_200),
        );
        clear.flags = SNAPSHOT;

        assert_eq!(
            deltas.deltas,
            vec![
                clear,
                delta(BookAction::Add, OrderSide::Buy, "456.10", "42", SNAPSHOT),
                delta(BookAction::Add, OrderSide::Sell, "456.35", "123", SNAPSHOT),
                delta(
                    BookAction::Add,
                    OrderSide::Sell,
                    "456.36",
                    "23",
                    SNAPSHOT | LAST
                ),
            ]
        );
    }

    #[rstest]
    fn empty_book_snapshot_is_a_lone_clear() {
        let deltas = parse_book_deltas(&book(Some(true), &[], &[]), &instrument(2, 0))
            .unwrap()
            .unwrap();

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].flags, SNAPSHOT | LAST);
    }

    #[rstest]
    fn book_update_sets_levels_and_removes_zero_sizes() {
        let deltas = parse_book_deltas(
            &book(Some(false), &[("456.10", "0")], &[("456.35", "7")]),
            &instrument(2, 0),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            deltas.deltas,
            vec![
                delta(BookAction::Delete, OrderSide::Buy, "456.10", "0", 0),
                delta(BookAction::Update, OrderSide::Sell, "456.35", "7", LAST),
            ]
        );
    }

    #[rstest]
    fn book_update_without_levels_emits_nothing() {
        let deltas = parse_book_deltas(&book(Some(false), &[], &[]), &instrument(2, 0)).unwrap();

        assert_eq!(deltas, None);
    }

    #[rstest]
    fn book_update_rejects_whole_message_for_one_finer_level() {
        let error = parse_book_deltas(
            &book(Some(false), &[("456.10", "1")], &[("456.355", "1")]),
            &instrument(2, 0),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "price 456.355 is finer than instrument precision 2"
        );
    }

    #[rstest]
    fn book_depth_keeps_best_levels_per_side() {
        let mut fixed = book(
            None,
            &[("456.10", "1"), ("456.09", "2"), ("456.08", "3")],
            &[("456.35", "4"), ("456.36", "5")],
        );
        fixed.data_type = super::super::messages::DataType::Book5;

        let depth = parse_book_depth(&fixed, 2, &instrument(2, 0)).unwrap();

        assert_eq!(
            depth,
            OrderBookDepth::new(
                InstrumentId::from("BTC-USD.COINBASE"),
                vec![
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("456.10"),
                        Quantity::from("1"),
                        0
                    ),
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("456.09"),
                        Quantity::from("2"),
                        0
                    ),
                ],
                vec![
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("456.35"),
                        Quantity::from("4"),
                        0
                    ),
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("456.36"),
                        Quantity::from("5"),
                        0
                    ),
                ],
                vec![0, 0],
                vec![0, 0],
                SNAPSHOT,
                2_323_346,
                UnixNanos::from(1_380_408_050_000_000_000),
                UnixNanos::from(1_489_876_941_376_334_200),
            )
        );
    }

    #[rstest]
    #[case("BUY", AggressorSide::Buy)]
    #[case("SELL", AggressorSide::Sell)]
    #[case("BUY_ESTIMATED", AggressorSide::NoAggressor)]
    #[case("SELL_ESTIMATED", AggressorSide::NoAggressor)]
    #[case("UNKNOWN", AggressorSide::NoAggressor)]
    #[case("NEW_SIDE", AggressorSide::NoAggressor)]
    fn taker_side_maps_only_reported_sides(#[case] wire: &str, #[case] expected: AggressorSide) {
        assert_eq!(parse_aggressor_side(Ustr::from(wire)), expected);
    }

    #[rstest]
    #[case("1-SECOND-LAST", "1SEC")]
    #[case("30-SECOND-LAST", "30SEC")]
    #[case("1-MINUTE-LAST", "1MIN")]
    #[case("15-MINUTE-LAST", "15MIN")]
    #[case("1-HOUR-LAST", "1HRS")]
    #[case("12-HOUR-LAST", "12HRS")]
    #[case("1-DAY-LAST", "1DAY")]
    fn bar_spec_maps_to_period_id_and_back(#[case] spec: &str, #[case] period_id: &str) {
        let spec = bar_spec(spec);

        assert_eq!(bar_spec_to_period_id(spec).unwrap(), period_id);
        assert_eq!(period_id_to_bar_spec(Ustr::from(period_id)), Some(spec));
    }

    #[rstest]
    #[case("12-SECOND-LAST")]
    #[case("12-MINUTE-LAST")]
    #[case("2-DAY-LAST")]
    #[case("1-WEEK-LAST")]
    #[case("1-MINUTE-MID")]
    #[case("100-TICK-LAST")]
    fn bar_spec_without_streaming_period_is_rejected(#[case] spec: &str) {
        assert!(bar_spec_to_period_id(bar_spec(spec)).is_err());
    }

    #[rstest]
    #[case("01MIN")]
    #[case("7SEC")]
    #[case("1MTH")]
    #[case("MIN")]
    #[case("1")]
    #[case("")]
    fn unsupported_period_id_has_no_bar_spec(#[case] period_id: &str) {
        assert_eq!(period_id_to_bar_spec(Ustr::from(period_id)), None);
    }
}

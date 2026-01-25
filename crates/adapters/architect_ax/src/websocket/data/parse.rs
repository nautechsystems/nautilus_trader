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

//! Parsing functions to convert Ax WebSocket messages to Nautilus domain types.

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggregationSource, AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    http::parse::candle_width_to_bar_spec,
    websocket::messages::{
        AxBookLevel, AxBookLevelL3, AxMdBookL1, AxMdBookL2, AxMdBookL3, AxMdCandle, AxMdTrade,
    },
};

const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;

/// Converts a Decimal to Price with specified precision.
fn decimal_to_price_dp(value: Decimal, precision: u8, field: &str) -> anyhow::Result<Price> {
    Price::from_decimal_dp(value, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

/// Parses an Ax L1 book message into a [`QuoteTick`].
///
/// L1 contains best bid/ask only, which maps directly to a quote tick.
///
/// # Errors
///
/// Returns an error if price or quantity parsing fails.
pub fn parse_book_l1_quote(
    book: &AxMdBookL1,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let (bid_price, bid_size) = if let Some(bid) = book.b.first() {
        (
            decimal_to_price_dp(bid.p, price_precision, "book.bid.price")?,
            Quantity::new(bid.q as f64, size_precision),
        )
    } else {
        (Price::zero(price_precision), Quantity::zero(size_precision))
    };

    let (ask_price, ask_size) = if let Some(ask) = book.a.first() {
        (
            decimal_to_price_dp(ask.p, price_precision, "book.ask.price")?,
            Quantity::new(ask.q as f64, size_precision),
        )
    } else {
        (Price::zero(price_precision), Quantity::zero(size_precision))
    };

    let ts_event = UnixNanos::from((book.ts as u64) * NANOSECONDS_IN_SECOND);

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("Failed to construct QuoteTick from Ax L1 book")
}

/// Parses a book level into price and quantity.
fn parse_book_level(
    level: &AxBookLevel,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<(Price, Quantity)> {
    let price = decimal_to_price_dp(level.p, price_precision, "book.level.price")?;
    let size = Quantity::new(level.q as f64, size_precision);
    Ok((price, size))
}

/// Parses an Ax L2 book message into [`OrderBookDeltas`].
///
/// L2 contains aggregated price levels. Each message is treated as a snapshot
/// that clears the book and adds all levels.
///
/// # Errors
///
/// Returns an error if price or quantity parsing fails.
pub fn parse_book_l2_deltas(
    book: &AxMdBookL2,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let ts_event = UnixNanos::from((book.ts as u64) * NANOSECONDS_IN_SECOND);

    let total_levels = book.b.len() + book.a.len();
    let capacity = total_levels + 1;

    let mut deltas = Vec::with_capacity(capacity);

    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    let mut processed = 0_usize;

    for level in &book.b {
        let (price, size) = parse_book_level(level, price_precision, size_precision)?;
        processed += 1;

        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("Failed to construct OrderBookDelta from Ax L2 bid level")?;

        deltas.push(delta);
    }

    for level in &book.a {
        let (price, size) = parse_book_level(level, price_precision, size_precision)?;
        processed += 1;

        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("Failed to construct OrderBookDelta from Ax L2 ask level")?;

        deltas.push(delta);
    }

    if total_levels == 0
        && let Some(first) = deltas.first_mut()
    {
        first.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("Failed to assemble OrderBookDeltas from Ax L2 message")
}

/// Parses a L3 book level into price and quantity.
fn parse_book_level_l3(
    level: &AxBookLevelL3,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<(Price, Quantity)> {
    let price = decimal_to_price_dp(level.p, price_precision, "book.level.price")?;
    let size = Quantity::new(level.q as f64, size_precision);
    Ok((price, size))
}

/// Parses an Ax L3 book message into [`OrderBookDeltas`].
///
/// L3 contains individual order quantities at each price level.
/// Each message is treated as a snapshot that clears the book and adds all orders.
///
/// # Errors
///
/// Returns an error if price or quantity parsing fails.
pub fn parse_book_l3_deltas(
    book: &AxMdBookL3,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let ts_event = UnixNanos::from((book.ts as u64) * NANOSECONDS_IN_SECOND);

    let total_orders: usize = book.b.iter().map(|l| l.o.len()).sum::<usize>()
        + book.a.iter().map(|l| l.o.len()).sum::<usize>();
    let capacity = total_orders + 1;

    let mut deltas = Vec::with_capacity(capacity);

    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    let mut processed = 0_usize;
    let mut order_id_counter = 1_u64;

    for level in &book.b {
        let (price, _) = parse_book_level_l3(level, price_precision, size_precision)?;

        for &order_qty in &level.o {
            processed += 1;

            let mut flags = 0_u8;
            if processed == total_orders {
                flags |= RecordFlag::F_LAST as u8;
            }

            let size = Quantity::new(order_qty as f64, size_precision);
            let order = BookOrder::new(OrderSide::Buy, price, size, order_id_counter);
            order_id_counter += 1;

            let delta = OrderBookDelta::new_checked(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                sequence,
                ts_event,
                ts_init,
            )
            .context("Failed to construct OrderBookDelta from Ax L3 bid order")?;

            deltas.push(delta);
        }
    }

    for level in &book.a {
        let (price, _) = parse_book_level_l3(level, price_precision, size_precision)?;

        for &order_qty in &level.o {
            processed += 1;

            let mut flags = 0_u8;
            if processed == total_orders {
                flags |= RecordFlag::F_LAST as u8;
            }

            let size = Quantity::new(order_qty as f64, size_precision);
            let order = BookOrder::new(OrderSide::Sell, price, size, order_id_counter);
            order_id_counter += 1;

            let delta = OrderBookDelta::new_checked(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                sequence,
                ts_event,
                ts_init,
            )
            .context("Failed to construct OrderBookDelta from Ax L3 ask order")?;

            deltas.push(delta);
        }
    }

    if total_orders == 0
        && let Some(first) = deltas.first_mut()
    {
        first.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("Failed to assemble OrderBookDeltas from Ax L3 message")
}

/// Parses an Ax trade message into a [`TradeTick`].
///
/// # Errors
///
/// Returns an error if price or quantity parsing fails.
pub fn parse_trade_tick(
    trade: &AxMdTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = decimal_to_price_dp(trade.p, price_precision, "trade.price")?;
    let size = Quantity::new(trade.q as f64, size_precision);
    let aggressor_side: AggressorSide = trade.d.map_or(AggressorSide::NoAggressor, |d| d.into());

    // Use transaction number as trade ID
    let trade_id = TradeId::new_checked(trade.tn.to_string())
        .context("Failed to create TradeId from transaction number")?;

    let ts_event = UnixNanos::from((trade.ts as u64) * NANOSECONDS_IN_SECOND);

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Ax trade message")
}

/// Parses an Ax candle message into a [`Bar`].
///
/// # Errors
///
/// Returns an error if price or quantity parsing fails.
pub fn parse_candle_bar(
    candle: &AxMdCandle,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = decimal_to_price_dp(candle.open, price_precision, "candle.open")?;
    let high = decimal_to_price_dp(candle.high, price_precision, "candle.high")?;
    let low = decimal_to_price_dp(candle.low, price_precision, "candle.low")?;
    let close = decimal_to_price_dp(candle.close, price_precision, "candle.close")?;
    let volume = Quantity::new(candle.volume as f64, size_precision);

    let ts_event = UnixNanos::from((candle.ts as u64) * NANOSECONDS_IN_SECOND);

    let bar_spec = candle_width_to_bar_spec(candle.width);
    let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Ax candle message")
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::CryptoPerpetual,
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{consts::AX_VENUE, enums::AxOrderSide},
        websocket::messages::{AxMdBookL1, AxMdBookL2, AxMdBookL3, AxMdCandle, AxMdTrade},
    };

    fn create_test_instrument() -> InstrumentAny {
        create_instrument_with_precision("BTC-PERP", 2, 3)
    }

    fn create_eurusd_instrument() -> InstrumentAny {
        create_instrument_with_precision("EURUSD-PERP", 4, 0)
    }

    fn create_instrument_with_precision(
        symbol: &str,
        price_precision: u8,
        size_precision: u8,
    ) -> InstrumentAny {
        let price_increment =
            Price::from_decimal_dp(Decimal::new(1, price_precision as u32), price_precision)
                .unwrap();
        let size_increment =
            Quantity::from_decimal_dp(Decimal::new(1, size_precision as u32), size_precision)
                .unwrap();

        let instrument = CryptoPerpetual::new(
            InstrumentId::new(Symbol::new(symbol), *AX_VENUE),
            Symbol::new(symbol),
            Currency::USD(),
            Currency::USD(),
            Currency::USD(),
            false,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None,
            Some(size_increment),
            None,
            Some(size_increment),
            None,
            None,
            None,
            None,
            Some(Decimal::new(1, 2)),
            Some(Decimal::new(5, 3)),
            Some(Decimal::new(2, 4)),
            Some(Decimal::new(5, 4)),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        InstrumentAny::CryptoPerpetual(instrument)
    }

    #[rstest]
    fn test_parse_book_l1_quote() {
        let book = AxMdBookL1 {
            t: "1".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("BTC-PERP"),
            b: vec![AxBookLevel {
                p: dec!(50000.50),
                q: 100,
            }],
            a: vec![AxBookLevel {
                p: dec!(50001.00),
                q: 150,
            }],
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let quote = parse_book_l1_quote(&book, &instrument, ts_init).unwrap();

        assert_eq!(quote.bid_price.as_f64(), 50000.50);
        assert_eq!(quote.ask_price.as_f64(), 50001.00);
        assert_eq!(quote.bid_size.as_f64(), 100.0);
        assert_eq!(quote.ask_size.as_f64(), 150.0);
    }

    #[rstest]
    fn test_parse_book_l2_deltas() {
        let book = AxMdBookL2 {
            t: "2".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("BTC-PERP"),
            b: vec![
                AxBookLevel {
                    p: dec!(50000.50),
                    q: 100,
                },
                AxBookLevel {
                    p: dec!(50000.00),
                    q: 200,
                },
            ],
            a: vec![
                AxBookLevel {
                    p: dec!(50001.00),
                    q: 150,
                },
                AxBookLevel {
                    p: dec!(50001.50),
                    q: 250,
                },
            ],
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let deltas = parse_book_l2_deltas(&book, &instrument, 1, ts_init).unwrap();

        // 1 clear + 4 levels
        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_book_l3_deltas() {
        let book = AxMdBookL3 {
            t: "3".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("BTC-PERP"),
            b: vec![AxBookLevelL3 {
                p: dec!(50000.50),
                q: 300,
                o: vec![100, 200],
            }],
            a: vec![AxBookLevelL3 {
                p: dec!(50001.00),
                q: 250,
                o: vec![150, 100],
            }],
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let deltas = parse_book_l3_deltas(&book, &instrument, 1, ts_init).unwrap();

        // 1 clear + 4 individual orders
        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let trade = AxMdTrade {
            t: "s".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("BTC-PERP"),
            p: dec!(50000.50),
            q: 100,
            d: Some(AxOrderSide::Buy),
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let tick = parse_trade_tick(&trade, &instrument, ts_init).unwrap();

        assert_eq!(tick.price.as_f64(), 50000.50);
        assert_eq!(tick.size.as_f64(), 100.0);
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
    }

    #[rstest]
    fn test_parse_book_l1_from_captured_data() {
        let json = include_str!("../../../test_data/ws_md_book_l1_captured.json");
        let book: AxMdBookL1 = serde_json::from_str(json).unwrap();

        assert_eq!(book.s.as_str(), "EURUSD-PERP");
        assert_eq!(book.b.len(), 1);
        assert_eq!(book.a.len(), 1);

        let instrument = create_eurusd_instrument();
        let ts_init = UnixNanos::default();

        let quote = parse_book_l1_quote(&book, &instrument, ts_init).unwrap();

        assert_eq!(quote.instrument_id.symbol.as_str(), "EURUSD-PERP");
        assert_eq!(quote.bid_price.as_f64(), 1.1712);
        assert_eq!(quote.ask_price.as_f64(), 1.1717);
        assert_eq!(quote.bid_size.as_f64(), 300.0);
        assert_eq!(quote.ask_size.as_f64(), 100.0);
    }

    #[rstest]
    fn test_parse_book_l2_from_captured_data() {
        let json = include_str!("../../../test_data/ws_md_book_l2_captured.json");
        let book: AxMdBookL2 = serde_json::from_str(json).unwrap();

        assert_eq!(book.s.as_str(), "EURUSD-PERP");
        assert_eq!(book.b.len(), 13);
        assert_eq!(book.a.len(), 12);

        let instrument = create_eurusd_instrument();
        let ts_init = UnixNanos::default();

        let deltas = parse_book_l2_deltas(&book, &instrument, 1, ts_init).unwrap();

        // 1 clear + 13 bids + 12 asks = 26 deltas
        assert_eq!(deltas.deltas.len(), 26);
        assert_eq!(deltas.instrument_id.symbol.as_str(), "EURUSD-PERP");

        // First delta should be clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Check first bid level
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.order.price.as_f64(), 1.1712);
        assert_eq!(first_bid.order.size.as_f64(), 300.0);

        // Check first ask level (after 13 bids + 1 clear = index 14)
        let first_ask = &deltas.deltas[14];
        assert_eq!(first_ask.order.side, OrderSide::Sell);
        assert_eq!(first_ask.order.price.as_f64(), 1.1719);
        assert_eq!(first_ask.order.size.as_f64(), 400.0);

        // Last delta should have F_LAST flag
        let last_delta = deltas.deltas.last().unwrap();
        assert!(last_delta.flags & RecordFlag::F_LAST as u8 != 0);
    }

    #[rstest]
    fn test_parse_book_l3_from_captured_data() {
        let json = include_str!("../../../test_data/ws_md_book_l3_captured.json");
        let book: AxMdBookL3 = serde_json::from_str(json).unwrap();

        assert_eq!(book.s.as_str(), "EURUSD-PERP");
        assert_eq!(book.b.len(), 15);
        assert_eq!(book.a.len(), 14);

        let instrument = create_eurusd_instrument();
        let ts_init = UnixNanos::default();

        let deltas = parse_book_l3_deltas(&book, &instrument, 1, ts_init).unwrap();

        // 1 clear + individual orders from each level
        // Each level has one order in the captured data
        assert_eq!(deltas.deltas.len(), 30); // 1 clear + 15 bids + 14 asks
        assert_eq!(deltas.instrument_id.symbol.as_str(), "EURUSD-PERP");

        // First delta should be clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Check first bid order
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.order.price.as_f64(), 1.1714);
        assert_eq!(first_bid.order.size.as_f64(), 100.0);

        // Last delta should have F_LAST flag
        let last_delta = deltas.deltas.last().unwrap();
        assert!(last_delta.flags & RecordFlag::F_LAST as u8 != 0);
    }

    #[rstest]
    fn test_parse_trade_from_captured_data() {
        let json = include_str!("../../../test_data/ws_md_trade_captured.json");
        let trade: AxMdTrade = serde_json::from_str(json).unwrap();

        assert_eq!(trade.s.as_str(), "EURUSD-PERP");
        assert_eq!(trade.p, dec!(1.1719));
        assert_eq!(trade.q, 400);
        assert_eq!(trade.d, Some(AxOrderSide::Buy));

        let instrument = create_eurusd_instrument();
        let ts_init = UnixNanos::default();

        let tick = parse_trade_tick(&trade, &instrument, ts_init).unwrap();

        assert_eq!(tick.instrument_id.symbol.as_str(), "EURUSD-PERP");
        assert_eq!(tick.price.as_f64(), 1.1719);
        assert_eq!(tick.size.as_f64(), 400.0);
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.trade_id.to_string(), "334589144");
    }

    #[rstest]
    fn test_parse_book_l1_empty_sides() {
        let book = AxMdBookL1 {
            t: "1".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("TEST-PERP"),
            b: vec![],
            a: vec![],
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let quote = parse_book_l1_quote(&book, &instrument, ts_init).unwrap();

        assert_eq!(quote.bid_price.as_f64(), 0.0);
        assert_eq!(quote.ask_price.as_f64(), 0.0);
        assert_eq!(quote.bid_size.as_f64(), 0.0);
        assert_eq!(quote.ask_size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_book_l2_empty_book() {
        let book = AxMdBookL2 {
            t: "2".to_string(),
            ts: 1700000000,
            tn: 12345,
            s: Ustr::from("TEST-PERP"),
            b: vec![],
            a: vec![],
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let deltas = parse_book_l2_deltas(&book, &instrument, 1, ts_init).unwrap();

        // Just clear delta with F_LAST
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(deltas.deltas[0].flags & RecordFlag::F_LAST as u8 != 0);
    }

    #[rstest]
    fn test_parse_candle_bar() {
        use crate::common::enums::AxCandleWidth;

        let candle = AxMdCandle {
            t: "c".to_string(),
            symbol: Ustr::from("BTC-PERP"),
            ts: 1700000000,
            open: dec!(50000.00),
            high: dec!(51000.00),
            low: dec!(49500.00),
            close: dec!(50500.00),
            volume: 1000,
            buy_volume: 600,
            sell_volume: 400,
            width: AxCandleWidth::Minutes1,
        };

        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let bar = parse_candle_bar(&candle, &instrument, ts_init).unwrap();

        assert_eq!(bar.open.as_f64(), 50000.00);
        assert_eq!(bar.high.as_f64(), 51000.00);
        assert_eq!(bar.low.as_f64(), 49500.00);
        assert_eq!(bar.close.as_f64(), 50500.00);
        assert_eq!(bar.volume.as_f64(), 1000.0);
        assert_eq!(bar.bar_type.instrument_id().symbol.as_str(), "BTC-PERP");
    }

    #[rstest]
    fn test_parse_candle_from_test_data() {
        let json = include_str!("../../../test_data/ws_md_candle.json");
        let candle: AxMdCandle = serde_json::from_str(json).unwrap();

        assert_eq!(candle.symbol.as_str(), "BTCUSD-PERP");
        assert_eq!(candle.open, dec!(49500.00));
        assert_eq!(candle.close, dec!(50000.00));

        let instrument = create_instrument_with_precision("BTCUSD-PERP", 2, 3);
        let ts_init = UnixNanos::default();

        let bar = parse_candle_bar(&candle, &instrument, ts_init).unwrap();

        assert_eq!(bar.open.as_f64(), 49500.00);
        assert_eq!(bar.high.as_f64(), 50500.00);
        assert_eq!(bar.low.as_f64(), 49000.00);
        assert_eq!(bar.close.as_f64(), 50000.00);
        assert_eq!(bar.volume.as_f64(), 5000.0);
        assert_eq!(bar.bar_type.instrument_id().symbol.as_str(), "BTCUSD-PERP");
    }
}

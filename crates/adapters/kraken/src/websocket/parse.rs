// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! WebSocket message parsers for converting Kraken streaming data to Nautilus domain models.

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide},
    identifiers::{InstrumentId, TradeId},
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};

use crate::{
    common::enums::KrakenOrderSide,
    websocket::messages::{
        KrakenWsBookData, KrakenWsBookLevel, KrakenWsTickerData, KrakenWsTradeData,
    },
};

/// Parses Kraken WebSocket ticker data into a Nautilus quote tick.
///
/// # Errors
///
/// Returns an error if:
/// - Bid or ask price/quantity cannot be parsed.
pub fn parse_quote_tick(
    ticker: &KrakenWsTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = Price::new_checked(ticker.bid, price_precision).with_context(|| {
        format!("Failed to construct bid Price with precision {price_precision}")
    })?;
    let bid_size = Quantity::new_checked(ticker.bid_qty, size_precision).with_context(|| {
        format!("Failed to construct bid Quantity with precision {size_precision}")
    })?;

    let ask_price = Price::new_checked(ticker.ask, price_precision).with_context(|| {
        format!("Failed to construct ask Price with precision {price_precision}")
    })?;
    let ask_size = Quantity::new_checked(ticker.ask_qty, size_precision).with_context(|| {
        format!("Failed to construct ask Quantity with precision {size_precision}")
    })?;

    // Kraken ticker doesn't include timestamp
    let ts_event = ts_init;

    Ok(QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

/// Parses Kraken WebSocket trade data into a Nautilus trade tick.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_trade_tick(
    trade: &KrakenWsTradeData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = Price::new_checked(trade.price, price_precision)
        .with_context(|| format!("Failed to construct Price with precision {price_precision}"))?;
    let size = Quantity::new_checked(trade.qty, size_precision)
        .with_context(|| format!("Failed to construct Quantity with precision {size_precision}"))?;

    let aggressor = match trade.side {
        KrakenOrderSide::Buy => AggressorSide::Buyer,
        KrakenOrderSide::Sell => AggressorSide::Seller,
    };

    let trade_id = TradeId::new_checked(trade.trade_id.to_string())?;
    let ts_event = parse_rfc3339_timestamp(&trade.timestamp, "trade.timestamp")?;

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken WebSocket trade")
}

/// Parses Kraken WebSocket book data into Nautilus order book deltas.
///
/// Returns a vector of deltas, one for each bid and ask level.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_book_deltas(
    book: &KrakenWsBookData,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<OrderBookDelta>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    // Parse timestamp if available, otherwise use ts_init
    let ts_event = if let Some(ref timestamp) = book.timestamp {
        parse_rfc3339_timestamp(timestamp, "book.timestamp")?
    } else {
        ts_init
    };

    let mut deltas = Vec::new();
    let mut current_sequence = sequence;

    // Process bids
    if let Some(ref bids) = book.bids {
        for level in bids {
            let delta = parse_book_level(
                level,
                OrderSide::Buy,
                instrument_id,
                price_precision,
                size_precision,
                current_sequence,
                ts_event,
                ts_init,
            )?;
            deltas.push(delta);
            current_sequence += 1;
        }
    }

    // Process asks
    if let Some(ref asks) = book.asks {
        for level in asks {
            let delta = parse_book_level(
                level,
                OrderSide::Sell,
                instrument_id,
                price_precision,
                size_precision,
                current_sequence,
                ts_event,
                ts_init,
            )?;
            deltas.push(delta);
            current_sequence += 1;
        }
    }

    Ok(deltas)
}

#[allow(clippy::too_many_arguments)]
fn parse_book_level(
    level: &KrakenWsBookLevel,
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = Price::new_checked(level.price, price_precision)
        .with_context(|| format!("Failed to construct Price with precision {price_precision}"))?;
    let size = Quantity::new_checked(level.qty, size_precision)
        .with_context(|| format!("Failed to construct Quantity with precision {size_precision}"))?;

    // Determine action based on quantity
    let action = if size.raw == 0 {
        BookAction::Delete
    } else {
        BookAction::Update
    };

    // Create order ID from price (Kraken doesn't provide order IDs)
    let order_id = price.raw as u64;
    let order = BookOrder::new(side, price, size, order_id);

    Ok(OrderBookDelta::new(
        instrument_id,
        action,
        order,
        0, // flags
        sequence,
        ts_event,
        ts_init,
    ))
}

fn parse_rfc3339_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    use chrono::DateTime;

    let dt = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("Failed to parse {field}='{value}' as RFC3339 timestamp"))?;

    Ok(UnixNanos::from(
        dt.timestamp_nanos_opt()
            .with_context(|| format!("Timestamp out of range for {field}"))? as u64,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{identifiers::Symbol, types::Currency};
    use rstest::rstest;

    use super::*;
    use crate::{common::consts::KRAKEN_VENUE, websocket::messages::KrakenWsMessage};

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn load_test_json(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    fn create_mock_instrument() -> InstrumentAny {
        use nautilus_model::instruments::currency_pair::CurrencyPair;

        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            1, // price_precision
            8, // size_precision
            Price::from("0.1"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            TS,
            TS,
        ))
    }

    #[rstest]
    fn test_parse_quote_tick() {
        let json = load_test_json("ws_ticker_snapshot.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let ticker: KrakenWsTickerData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let quote_tick = parse_quote_tick(&ticker, &instrument, TS).unwrap();

        assert_eq!(quote_tick.instrument_id, instrument.id());
        assert!(quote_tick.bid_price.as_f64() > 0.0);
        assert!(quote_tick.ask_price.as_f64() > 0.0);
        assert!(quote_tick.bid_size.as_f64() > 0.0);
        assert!(quote_tick.ask_size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let json = load_test_json("ws_trade_update.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let trade: KrakenWsTradeData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let trade_tick = parse_trade_tick(&trade, &instrument, TS).unwrap();

        assert_eq!(trade_tick.instrument_id, instrument.id());
        assert!(trade_tick.price.as_f64() > 0.0);
        assert!(trade_tick.size.as_f64() > 0.0);
        assert!(matches!(
            trade_tick.aggressor_side,
            AggressorSide::Buyer | AggressorSide::Seller
        ));
    }

    #[rstest]
    fn test_parse_book_deltas_snapshot() {
        let json = load_test_json("ws_book_snapshot.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let book: KrakenWsBookData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let deltas = parse_book_deltas(&book, &instrument, 1, TS).unwrap();

        assert!(!deltas.is_empty());

        // Check that we have both bids and asks
        let bid_count = deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Buy)
            .count();
        let ask_count = deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Sell)
            .count();

        assert!(bid_count > 0);
        assert!(ask_count > 0);

        // Check first delta
        let first_delta = &deltas[0];
        assert_eq!(first_delta.instrument_id, instrument.id());
        assert!(first_delta.order.price.as_f64() > 0.0);
        assert!(first_delta.order.size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_book_deltas_update() {
        let json = load_test_json("ws_book_update.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let book: KrakenWsBookData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let deltas = parse_book_deltas(&book, &instrument, 1, TS).unwrap();

        assert!(!deltas.is_empty());

        // Check that we have at least one delta
        let first_delta = &deltas[0];
        assert_eq!(first_delta.instrument_id, instrument.id());
        assert!(first_delta.order.price.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_rfc3339_timestamp() {
        let timestamp = "2023-10-06T17:35:55.440295Z";
        let result = parse_rfc3339_timestamp(timestamp, "test").unwrap();
        assert!(result.as_u64() > 0);
    }
}

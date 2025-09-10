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

//! Parsing utilities for Delta Exchange WebSocket messages.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BarAggregation, BookAction, OrderSide, PriceType},
    identifiers::{InstrumentId, Symbol, TradeId, Venue},
    orderbook::level::BookLevel,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    error::DeltaExchangeWsError,
    messages::{
        DeltaExchangeWsCandleMsg, DeltaExchangeWsMarkPriceMsg, DeltaExchangeWsOrderBookSnapshotMsg,
        DeltaExchangeWsOrderBookUpdateMsg, DeltaExchangeWsTickerMsg, DeltaExchangeWsTradeMsg,
    },
};
use crate::common::{
    parse::{parse_price, parse_quantity, parse_symbol, parse_timestamp_us, parse_venue},
};

/// Parse a ticker message to QuoteTick.
pub fn parse_ticker_msg(
    msg: &DeltaExchangeWsTickerMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> Result<QuoteTick, DeltaExchangeWsError> {
    let ts_event = parse_timestamp_us(msg.timestamp);
    let ts_init = ts_event; // Use same timestamp for init

    let bid_price = msg.bid
        .map(|p| parse_price(&p.to_string(), price_precision))
        .transpose()
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    let ask_price = msg.ask
        .map(|p| parse_price(&p.to_string(), price_precision))
        .transpose()
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    // Use default size if not available
    let bid_size = parse_quantity("1.0", size_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
    let ask_size = parse_quantity("1.0", size_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    if let (Some(bid), Some(ask)) = (bid_price, ask_price) {
        Ok(QuoteTick::new(
            instrument_id,
            bid,
            ask,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?)
    } else {
        Err(DeltaExchangeWsError::parsing_error(
            "Missing bid or ask price in ticker message",
        ))
    }
}

/// Parse a trade message to TradeTick.
pub fn parse_trade_msg(
    msg: &DeltaExchangeWsTradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> Result<TradeTick, DeltaExchangeWsError> {
    let price = parse_price(&msg.price.to_string(), price_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    let size = parse_quantity(&msg.size.to_string(), size_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    let aggressor_side = match msg.buyer_role.as_str() {
        "taker" => AggressorSide::Buyer,
        "maker" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    let ts_event = parse_timestamp_us(msg.timestamp);
    let ts_init = ts_event;

    // Generate a trade ID from timestamp and price
    let trade_id = TradeId::new(&format!("{}_{}", msg.timestamp, msg.price))
        .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?)
}

/// Parse an order book snapshot message to OrderBookDeltas.
pub fn parse_orderbook_snapshot_msg(
    msg: &DeltaExchangeWsOrderBookSnapshotMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> Result<OrderBookDeltas, DeltaExchangeWsError> {
    let ts_event = parse_timestamp_us(msg.timestamp);
    let ts_init = ts_event;

    let mut deltas = Vec::new();

    // Process buy levels (bids)
    for level in &msg.buy {
        let price = parse_price(&level.price.to_string(), price_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
        let size = parse_quantity(&level.size.to_string(), size_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

        let book_level = BookLevel::new(price, size)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            book_level,
            ts_event,
            ts_init,
            msg.last_sequence_no,
        ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        deltas.push(delta);
    }

    // Process sell levels (asks)
    for level in &msg.sell {
        let price = parse_price(&level.price.to_string(), price_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
        let size = parse_quantity(&level.size.to_string(), size_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

        let book_level = BookLevel::new(price, size)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            book_level,
            ts_event,
            ts_init,
            msg.last_sequence_no,
        ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        deltas.push(delta);
    }

    OrderBookDeltas::new(instrument_id, deltas)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))
}

/// Parse an order book update message to OrderBookDeltas.
pub fn parse_orderbook_update_msg(
    msg: &DeltaExchangeWsOrderBookUpdateMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> Result<OrderBookDeltas, DeltaExchangeWsError> {
    let ts_event = parse_timestamp_us(msg.timestamp);
    let ts_init = ts_event;

    let mut deltas = Vec::new();

    // Process buy level updates (bids)
    for level in &msg.buy {
        let price = parse_price(&level.price.to_string(), price_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
        let size = parse_quantity(&level.size.to_string(), size_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

        let book_level = BookLevel::new(price, size)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        let action = if size.as_f64() == 0.0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let delta = OrderBookDelta::new(
            instrument_id,
            action,
            book_level,
            ts_event,
            ts_init,
            msg.sequence_no,
        ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        deltas.push(delta);
    }

    // Process sell level updates (asks)
    for level in &msg.sell {
        let price = parse_price(&level.price.to_string(), price_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
        let size = parse_quantity(&level.size.to_string(), size_precision)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

        let book_level = BookLevel::new(price, size)
            .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        let action = if size.as_f64() == 0.0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let delta = OrderBookDelta::new(
            instrument_id,
            action,
            book_level,
            ts_event,
            ts_init,
            msg.sequence_no,
        ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

        deltas.push(delta);
    }

    OrderBookDeltas::new(instrument_id, deltas)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))
}

/// Parse a candle message to Bar.
pub fn parse_candle_msg(
    msg: &DeltaExchangeWsCandleMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> Result<Bar, DeltaExchangeWsError> {
    let open = parse_price(&msg.open.to_string(), price_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
    let high = parse_price(&msg.high.to_string(), price_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
    let low = parse_price(&msg.low.to_string(), price_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
    let close = parse_price(&msg.close.to_string(), price_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;
    let volume = parse_quantity(&msg.volume.to_string(), size_precision)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e))?;

    let ts_event = parse_timestamp_us(msg.timestamp);
    let ts_init = ts_event;

    // Parse resolution to BarAggregation
    let aggregation = match msg.resolution.as_str() {
        "1m" => BarAggregation::Minute,
        "5m" => BarAggregation::Minute,
        "15m" => BarAggregation::Minute,
        "30m" => BarAggregation::Minute,
        "1h" => BarAggregation::Hour,
        "4h" => BarAggregation::Hour,
        "1d" => BarAggregation::Day,
        _ => BarAggregation::Minute, // Default fallback
    };

    // Extract step from resolution
    let step = match msg.resolution.as_str() {
        "1m" => 1,
        "5m" => 5,
        "15m" => 15,
        "30m" => 30,
        "1h" => 1,
        "4h" => 4,
        "1d" => 1,
        _ => 1,
    };

    let bar_spec = BarSpecification::new(step, aggregation, PriceType::Last)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

    let bar_type = BarType::new(instrument_id, bar_spec, None)
        .map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))?;

    Bar::new(
        bar_type,
        open,
        high,
        low,
        close,
        volume,
        ts_event,
        ts_init,
    ).map_err(|e| DeltaExchangeWsError::parsing_error(e.to_string()))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::*;

    #[test]
    fn test_parse_trade_msg() {
        let msg = DeltaExchangeWsTradeMsg {
            message_type: super::super::enums::WsMessageType::Update,
            symbol: "BTCUSD".into(),
            price: Decimal::new(50000, 0),
            size: Decimal::new(100, 0),
            buyer_role: "taker".to_string(),
            timestamp: 1704110400000000, // microseconds
        };

        let instrument_id = InstrumentId::new(
            Symbol::new("BTCUSD").unwrap(),
            Venue::new("DELTA_EXCHANGE").unwrap(),
        );

        let trade_tick = parse_trade_msg(&msg, instrument_id, 2, 0).unwrap();
        
        assert_eq!(trade_tick.instrument_id, instrument_id);
        assert_eq!(trade_tick.price.as_f64(), 50000.0);
        assert_eq!(trade_tick.size.as_f64(), 100.0);
        assert_eq!(trade_tick.aggressor_side, AggressorSide::Buyer);
    }

    #[test]
    fn test_parse_candle_msg() {
        let msg = DeltaExchangeWsCandleMsg {
            message_type: super::super::enums::WsMessageType::Update,
            symbol: "BTCUSD".into(),
            resolution: "1m".to_string(),
            open: Decimal::new(49500, 0),
            high: Decimal::new(50500, 0),
            low: Decimal::new(49000, 0),
            close: Decimal::new(50000, 0),
            volume: Decimal::new(1000, 0),
            timestamp: 1704110400000000, // microseconds
        };

        let instrument_id = InstrumentId::new(
            Symbol::new("BTCUSD").unwrap(),
            Venue::new("DELTA_EXCHANGE").unwrap(),
        );

        let bar = parse_candle_msg(&msg, instrument_id, 2, 0).unwrap();
        
        assert_eq!(bar.bar_type.instrument_id, instrument_id);
        assert_eq!(bar.open.as_f64(), 49500.0);
        assert_eq!(bar.high.as_f64(), 50500.0);
        assert_eq!(bar.low.as_f64(), 49000.0);
        assert_eq!(bar.close.as_f64(), 50000.0);
        assert_eq!(bar.volume.as_f64(), 1000.0);
    }
}

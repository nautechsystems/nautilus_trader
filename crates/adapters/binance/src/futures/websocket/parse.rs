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

//! Parsing utilities for Binance Futures WebSocket JSON messages.

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use ustr::Ustr;

use super::messages::{
    BinanceFuturesAggTradeMsg, BinanceFuturesBookTickerMsg, BinanceFuturesDepthUpdateMsg,
    BinanceFuturesTradeMsg,
};
use crate::{common::enums::BinanceWsEventType, websocket::error::BinanceWsResult};

/// Parses an aggregate trade message into a `TradeTick`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_agg_trade(
    msg: &BinanceFuturesAggTradeMsg,
    instrument: &InstrumentAny,
) -> BinanceWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = msg
        .price
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
    let size = msg
        .quantity
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;

    let aggressor_side = if msg.is_buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };

    let ts_event = UnixNanos::from(msg.trade_time as u64 * 1_000_000); // ms to ns
    let trade_id = TradeId::new(msg.agg_trade_id.to_string());

    Ok(TradeTick::new(
        instrument_id,
        Price::new(price, price_precision),
        Quantity::new(size, size_precision),
        aggressor_side,
        trade_id,
        ts_event,
        ts_event,
    ))
}

/// Parses a trade message into a `TradeTick`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_trade(
    msg: &BinanceFuturesTradeMsg,
    instrument: &InstrumentAny,
) -> BinanceWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = msg
        .price
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
    let size = msg
        .quantity
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;

    let aggressor_side = if msg.is_buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };

    let ts_event = UnixNanos::from(msg.trade_time as u64 * 1_000_000); // ms to ns
    let trade_id = TradeId::new(msg.trade_id.to_string());

    Ok(TradeTick::new(
        instrument_id,
        Price::new(price, price_precision),
        Quantity::new(size, size_precision),
        aggressor_side,
        trade_id,
        ts_event,
        ts_event,
    ))
}

/// Parses a book ticker message into a `QuoteTick`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_book_ticker(
    msg: &BinanceFuturesBookTickerMsg,
    instrument: &InstrumentAny,
) -> BinanceWsResult<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = msg
        .best_bid_price
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
    let bid_size = msg
        .best_bid_qty
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
    let ask_price = msg
        .best_ask_price
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
    let ask_size = msg
        .best_ask_qty
        .parse::<f64>()
        .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;

    let ts_event = UnixNanos::from(msg.transaction_time as u64 * 1_000_000); // ms to ns

    Ok(QuoteTick::new(
        instrument_id,
        Price::new(bid_price, price_precision),
        Price::new(ask_price, price_precision),
        Quantity::new(bid_size, size_precision),
        Quantity::new(ask_size, size_precision),
        ts_event,
        ts_event,
    ))
}

/// Parses a depth update message into `OrderBookDeltas`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_depth_update(
    msg: &BinanceFuturesDepthUpdateMsg,
    instrument: &InstrumentAny,
) -> BinanceWsResult<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let ts_event = UnixNanos::from(msg.transaction_time as u64 * 1_000_000); // ms to ns

    let mut deltas = Vec::with_capacity(msg.bids.len() + msg.asks.len());

    // Process bids
    for (i, bid) in msg.bids.iter().enumerate() {
        let price = bid[0]
            .parse::<f64>()
            .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
        let size = bid[1]
            .parse::<f64>()
            .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;

        let action = if size == 0.0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let is_last = i == msg.bids.len() - 1 && msg.asks.is_empty();
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(price, price_precision),
            Quantity::new(size, size_precision),
            0,
        );

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            msg.final_update_id,
            ts_event,
            ts_event,
        ));
    }

    // Process asks
    for (i, ask) in msg.asks.iter().enumerate() {
        let price = ask[0]
            .parse::<f64>()
            .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;
        let size = ask[1]
            .parse::<f64>()
            .map_err(|e| crate::websocket::error::BinanceWsError::ParseError(e.to_string()))?;

        let action = if size == 0.0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let is_last = i == msg.asks.len() - 1;
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(price, price_precision),
            Quantity::new(size, size_precision),
            0,
        );

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            msg.final_update_id,
            ts_event,
            ts_event,
        ));
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Extracts the symbol from a raw JSON message.
pub fn extract_symbol(json: &serde_json::Value) -> Option<Ustr> {
    json.get("s").and_then(|v| v.as_str()).map(Ustr::from)
}

/// Extracts the event type from a raw JSON message.
pub fn extract_event_type(json: &serde_json::Value) -> Option<BinanceWsEventType> {
    json.get("e")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

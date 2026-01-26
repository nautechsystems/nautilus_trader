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
    data::{
        Bar, BarSpecification, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use ustr::Ustr;

use super::{
    error::{BinanceWsError, BinanceWsResult},
    messages::{
        BinanceFuturesAggTradeMsg, BinanceFuturesBookTickerMsg, BinanceFuturesDepthUpdateMsg,
        BinanceFuturesKlineMsg, BinanceFuturesMarkPriceMsg, BinanceFuturesTradeMsg,
    },
};
use crate::common::enums::{BinanceKlineInterval, BinanceWsEventType};

/// Parses an aggregate trade message into a `TradeTick`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_agg_trade(
    msg: &BinanceFuturesAggTradeMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> BinanceWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = msg
        .price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let size = msg
        .quantity
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

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
        ts_init,
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
    ts_init: UnixNanos,
) -> BinanceWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = msg
        .price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let size = msg
        .quantity
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

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
        ts_init,
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
    ts_init: UnixNanos,
) -> BinanceWsResult<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = msg
        .best_bid_price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let bid_size = msg
        .best_bid_qty
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let ask_price = msg
        .best_ask_price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let ask_size = msg
        .best_ask_qty
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

    let ts_event = UnixNanos::from(msg.transaction_time as u64 * 1_000_000); // ms to ns

    Ok(QuoteTick::new(
        instrument_id,
        Price::new(bid_price, price_precision),
        Price::new(ask_price, price_precision),
        Quantity::new(bid_size, size_precision),
        Quantity::new(ask_size, size_precision),
        ts_event,
        ts_init,
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
    ts_init: UnixNanos,
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
            .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
        let size = bid[1]
            .parse::<f64>()
            .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

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
            ts_init,
        ));
    }

    // Process asks
    for (i, ask) in msg.asks.iter().enumerate() {
        let price = ask[0]
            .parse::<f64>()
            .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
        let size = ask[1]
            .parse::<f64>()
            .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

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
            ts_init,
        ));
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a mark price message into `MarkPriceUpdate`, `IndexPriceUpdate`, and `FundingRateUpdate`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_mark_price(
    msg: &BinanceFuturesMarkPriceMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> BinanceWsResult<(MarkPriceUpdate, IndexPriceUpdate, FundingRateUpdate)> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();

    let mark_price = msg
        .mark_price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let index_price = msg
        .index_price
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let funding_rate = msg
        .funding_rate
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

    let ts_event = UnixNanos::from(msg.event_time as u64 * 1_000_000); // ms to ns
    let next_funding_ns = if msg.next_funding_time > 0 {
        Some(UnixNanos::from(msg.next_funding_time as u64 * 1_000_000))
    } else {
        None
    };

    let mark_update = MarkPriceUpdate::new(
        instrument_id,
        Price::new(mark_price, price_precision),
        ts_event,
        ts_init,
    );

    let index_update = IndexPriceUpdate::new(
        instrument_id,
        Price::new(index_price, price_precision),
        ts_event,
        ts_init,
    );

    let funding_update = FundingRateUpdate::new(
        instrument_id,
        Decimal::from_f64(funding_rate).unwrap_or_default(),
        next_funding_ns,
        ts_event,
        ts_init,
    );

    Ok((mark_update, index_update, funding_update))
}

/// Converts a Binance kline interval to a Nautilus `BarSpecification`.
fn interval_to_bar_spec(interval: BinanceKlineInterval) -> BarSpecification {
    match interval {
        BinanceKlineInterval::Second1 => {
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last)
        }
        BinanceKlineInterval::Minute1 => {
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
        }
        BinanceKlineInterval::Minute3 => {
            BarSpecification::new(3, BarAggregation::Minute, PriceType::Last)
        }
        BinanceKlineInterval::Minute5 => {
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last)
        }
        BinanceKlineInterval::Minute15 => {
            BarSpecification::new(15, BarAggregation::Minute, PriceType::Last)
        }
        BinanceKlineInterval::Minute30 => {
            BarSpecification::new(30, BarAggregation::Minute, PriceType::Last)
        }
        BinanceKlineInterval::Hour1 => {
            BarSpecification::new(1, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Hour2 => {
            BarSpecification::new(2, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Hour4 => {
            BarSpecification::new(4, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Hour6 => {
            BarSpecification::new(6, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Hour8 => {
            BarSpecification::new(8, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Hour12 => {
            BarSpecification::new(12, BarAggregation::Hour, PriceType::Last)
        }
        BinanceKlineInterval::Day1 => {
            BarSpecification::new(1, BarAggregation::Day, PriceType::Last)
        }
        BinanceKlineInterval::Day3 => {
            BarSpecification::new(3, BarAggregation::Day, PriceType::Last)
        }
        BinanceKlineInterval::Week1 => {
            BarSpecification::new(1, BarAggregation::Week, PriceType::Last)
        }
        BinanceKlineInterval::Month1 => {
            BarSpecification::new(1, BarAggregation::Month, PriceType::Last)
        }
    }
}

/// Parses a kline message into a `Bar`.
///
/// Returns `None` if the kline is not closed yet.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_kline(
    msg: &BinanceFuturesKlineMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> BinanceWsResult<Option<Bar>> {
    // Only emit bars when the kline is closed
    if !msg.kline.is_closed {
        return Ok(None);
    }

    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let spec = interval_to_bar_spec(msg.kline.interval);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = msg
        .kline
        .open
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let high = msg
        .kline
        .high
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let low = msg
        .kline
        .low
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let close = msg
        .kline
        .close
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;
    let volume = msg
        .kline
        .volume
        .parse::<f64>()
        .map_err(|e| BinanceWsError::ParseError(e.to_string()))?;

    // Use the kline close time as the event timestamp
    let ts_event = UnixNanos::from(msg.kline.close_time as u64 * 1_000_000); // ms to ns

    let bar = Bar::new(
        bar_type,
        Price::new(open, price_precision),
        Price::new(high, price_precision),
        Price::new(low, price_precision),
        Price::new(close, price_precision),
        Quantity::new(volume, size_precision),
        ts_event,
        ts_init,
    );

    Ok(Some(bar))
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

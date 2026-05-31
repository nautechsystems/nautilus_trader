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

//! Parsing utilities for Binance Spot public JSON WebSocket messages.

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick,
        TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};

use super::messages::{
    BinanceSpotBookTickerMsg, BinanceSpotKlineMsg, BinanceSpotPartialDepthMsg, BinanceSpotTradeMsg,
};
use crate::common::{
    enums::BinanceKlineInterval,
    parse::{parse_price_at_precision, parse_quantity_at_precision},
};

/// Parses a trade message into a `TradeTick`.
pub fn parse_trade(
    msg: &BinanceSpotTradeMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = msg
        .price
        .parse::<f64>()
        .with_context(|| format!("invalid trade price `{}`", msg.price))?;
    let size = msg
        .quantity
        .parse::<f64>()
        .with_context(|| format!("invalid trade quantity `{}`", msg.quantity))?;

    let aggressor_side = if msg.is_buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };

    let ts_event = UnixNanos::from_millis(msg.trade_time as u64);

    Ok(TradeTick::new(
        instrument_id,
        Price::new(price, price_precision),
        Quantity::new(size, size_precision),
        aggressor_side,
        TradeId::new(msg.trade_id.to_string()),
        ts_event,
        ts_init,
    ))
}

/// Parses a book ticker message into a `QuoteTick`.
pub fn parse_book_ticker(
    msg: &BinanceSpotBookTickerMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = msg
        .best_bid_price
        .parse::<f64>()
        .with_context(|| format!("invalid bid price `{}`", msg.best_bid_price))?;
    let bid_size = msg
        .best_bid_qty
        .parse::<f64>()
        .with_context(|| format!("invalid bid quantity `{}`", msg.best_bid_qty))?;
    let ask_price = msg
        .best_ask_price
        .parse::<f64>()
        .with_context(|| format!("invalid ask price `{}`", msg.best_ask_price))?;
    let ask_size = msg
        .best_ask_qty
        .parse::<f64>()
        .with_context(|| format!("invalid ask quantity `{}`", msg.best_ask_qty))?;

    let ts_event = msg.transaction_time.map_or_else(
        || UnixNanos::from_millis(msg.event_time as u64),
        |ts| UnixNanos::from_millis(ts as u64),
    );

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

/// Parses a partial depth snapshot message into `OrderBookDeltas`.
///
/// Returns `None` when there are no usable levels.
pub fn parse_depth_snapshot(
    msg: &BinanceSpotPartialDepthMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Option<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut deltas = Vec::with_capacity(msg.bids.len() + msg.asks.len() + 1);
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_init, ts_init));

    for (i, level) in msg.bids.iter().enumerate() {
        let Some(price) = parse_price_at_precision(&level[0], price_precision) else {
            continue;
        };
        let Some(size) = parse_quantity_at_precision(&level[1], size_precision) else {
            continue;
        };

        let flags = if i == msg.bids.len() - 1 && msg.asks.is_empty() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Buy, price, size, 0),
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let Some(price) = parse_price_at_precision(&level[0], price_precision) else {
            continue;
        };
        let Some(size) = parse_quantity_at_precision(&level[1], size_precision) else {
            continue;
        };

        let flags = if i == msg.asks.len() - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Sell, price, size, 0),
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    if deltas.len() <= 1 {
        return None;
    }

    Some(OrderBookDeltas::new(instrument_id, deltas))
}

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

/// Parses a kline message into a closed `Bar`.
///
/// Returns `None` if the kline is not closed yet.
pub fn parse_kline(
    msg: &BinanceSpotKlineMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<Bar>> {
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
        .with_context(|| format!("invalid open price `{}`", msg.kline.open))?;
    let high = msg
        .kline
        .high
        .parse::<f64>()
        .with_context(|| format!("invalid high price `{}`", msg.kline.high))?;
    let low = msg
        .kline
        .low
        .parse::<f64>()
        .with_context(|| format!("invalid low price `{}`", msg.kline.low))?;
    let close = msg
        .kline
        .close
        .parse::<f64>()
        .with_context(|| format!("invalid close price `{}`", msg.kline.close))?;
    let volume = msg
        .kline
        .volume
        .parse::<f64>()
        .with_context(|| format!("invalid volume `{}`", msg.kline.volume))?;

    let ts_event = UnixNanos::from_millis(msg.kline.close_time as u64);

    Ok(Some(Bar::new(
        bar_type,
        Price::new(open, price_precision),
        Price::new(high, price_precision),
        Price::new(low, price_precision),
        Price::new(close, price_precision),
        Quantity::new(volume, size_precision),
        ts_event,
        ts_init,
    )))
}

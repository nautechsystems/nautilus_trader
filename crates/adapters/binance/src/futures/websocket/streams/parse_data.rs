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

    let ts_event = UnixNanos::from_millis(msg.trade_time as u64);
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

    let ts_event = UnixNanos::from_millis(msg.trade_time as u64);
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

    let ts_event = UnixNanos::from_millis(msg.transaction_time as u64);

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

    let ts_event = UnixNanos::from_millis(msg.transaction_time as u64);

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

    let ts_event = UnixNanos::from_millis(msg.event_time as u64);
    let next_funding_ns = if msg.next_funding_time > 0 {
        Some(UnixNanos::from_millis(msg.next_funding_time as u64))
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
        None, // Binance does not provide the funding interval through WebSocket API
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
    let ts_event = UnixNanos::from_millis(msg.kline.close_time as u64);

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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;
    use crate::{
        common::{
            enums::{BinanceOrderStatus, BinanceSide, BinanceTradingStatus},
            parse::parse_usdm_instrument,
            testing::{load_fixture_string, load_json_fixture},
        },
        futures::{
            http::models::BinanceFuturesUsdSymbol,
            websocket::streams::messages::{BinanceFuturesLiquidationMsg, BinanceFuturesTickerMsg},
        },
    };

    const PRICE_PRECISION: u8 = 8;
    const SIZE_PRECISION: u8 = 3;

    fn sample_futures_symbol() -> BinanceFuturesUsdSymbol {
        BinanceFuturesUsdSymbol {
            symbol: Ustr::from("BTCUSDT"),
            pair: Ustr::from("BTCUSDT"),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            status: BinanceTradingStatus::Trading,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: Ustr::from("BTC"),
            quote_asset: Ustr::from("USDT"),
            margin_asset: Ustr::from("USDT"),
            price_precision: PRICE_PRECISION as i32,
            quantity_precision: SIZE_PRECISION as i32,
            base_asset_precision: 8,
            quote_precision: 8,
            underlying_type: Some("COIN".to_string()),
            underlying_sub_type: vec!["PoW".to_string()],
            settle_plan: None,
            trigger_protect: Some("0.0500".to_string()),
            liquidation_fee: Some("0.012500".to_string()),
            market_take_bound: Some("0.05".to_string()),
            order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
            time_in_force: vec!["GTC".to_string(), "IOC".to_string()],
            filters: vec![
                json!({
                    "filterType": "PRICE_FILTER",
                    "tickSize": "0.00000001",
                    "maxPrice": "1000000",
                    "minPrice": "0.00000001"
                }),
                json!({
                    "filterType": "LOT_SIZE",
                    "stepSize": "0.001",
                    "maxQty": "1000",
                    "minQty": "0.001"
                }),
            ],
        }
    }

    fn sample_instrument() -> InstrumentAny {
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        parse_usdm_instrument(&sample_futures_symbol(), ts, ts).unwrap()
    }

    fn load_market_fixture<T: DeserializeOwned>(filename: &str) -> T {
        let path = format!("futures/market_data_json/{filename}");
        serde_json::from_str(&load_fixture_string(&path))
            .unwrap_or_else(|e| panic!("Failed to parse fixture {path}: {e}"))
    }

    #[rstest]
    fn test_parse_agg_trade() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesAggTradeMsg = load_market_fixture("agg_trade_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let trade = parse_agg_trade(&msg, &instrument, ts_init).unwrap();

        assert_eq!(trade.instrument_id, instrument.id());
        assert_eq!(trade.price, Price::new(0.001, PRICE_PRECISION));
        assert_eq!(trade.size, Quantity::new(100.0, SIZE_PRECISION));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id, TradeId::new("5933014"));
        assert_eq!(trade.ts_event, UnixNanos::from(123_456_785_000_000u64));
        assert_eq!(trade.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_trade() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesTradeMsg = load_market_fixture("trade_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let trade = parse_trade(&msg, &instrument, ts_init).unwrap();

        assert_eq!(trade.instrument_id, instrument.id());
        assert_eq!(trade.price, Price::new(0.001, PRICE_PRECISION));
        assert_eq!(trade.size, Quantity::new(100.0, SIZE_PRECISION));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id, TradeId::new("5933014"));
        assert_eq!(trade.ts_event, UnixNanos::from(123_456_785_000_000u64));
        assert_eq!(trade.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_book_ticker() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesBookTickerMsg = load_market_fixture("book_ticker_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let quote = parse_book_ticker(&msg, &instrument, ts_init).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, Price::new(25.3519, PRICE_PRECISION));
        assert_eq!(quote.ask_price, Price::new(25.3652, PRICE_PRECISION));
        assert_eq!(quote.bid_size, Quantity::new(31.21, SIZE_PRECISION));
        assert_eq!(quote.ask_size, Quantity::new(40.66, SIZE_PRECISION));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_568_014_460_891_000_000u64)
        );
        assert_eq!(quote.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_depth_update() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesDepthUpdateMsg = load_market_fixture("depth_update_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let deltas = parse_depth_update(&msg, &instrument, ts_init).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(deltas.sequence, 160);
        assert_eq!(deltas.ts_event, UnixNanos::from(123_456_788_000_000u64));
        assert_eq!(deltas.ts_init, ts_init);
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
        assert_eq!(
            deltas.deltas[0].order.price,
            Price::new(0.0024, PRICE_PRECISION)
        );
        assert_eq!(
            deltas.deltas[0].order.size,
            Quantity::new(10.0, SIZE_PRECISION)
        );
        assert_eq!(deltas.deltas[1].action, BookAction::Update);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Sell);
        assert_eq!(
            deltas.deltas[1].order.price,
            Price::new(0.0026, PRICE_PRECISION)
        );
        assert_eq!(
            deltas.deltas[1].order.size,
            Quantity::new(100.0, SIZE_PRECISION)
        );
        assert_eq!(deltas.deltas[1].flags, RecordFlag::F_LAST as u8);
    }

    #[rstest]
    fn test_parse_mark_price() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesMarkPriceMsg = load_market_fixture("mark_price_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let (mark, index, funding) = parse_mark_price(&msg, &instrument, ts_init).unwrap();

        assert_eq!(mark.instrument_id, instrument.id());
        assert_eq!(mark.value, Price::new(11794.15, PRICE_PRECISION));
        assert_eq!(index.value, Price::new(11784.62659091, PRICE_PRECISION));
        assert_eq!(mark.ts_event, UnixNanos::from(1_562_305_380_000_000_000u64));
        assert_eq!(funding.instrument_id, instrument.id());
        assert_eq!(funding.rate.to_string(), "0.00038167");
        assert_eq!(
            funding.next_funding_ns,
            Some(UnixNanos::from(1_562_306_400_000_000_000u64))
        );
        assert_eq!(
            funding.ts_event,
            UnixNanos::from(1_562_305_380_000_000_000u64)
        );
        assert_eq!(funding.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_kline_closed() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesKlineMsg = load_market_fixture("kline_stream_closed.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let bar = parse_kline(&msg, &instrument, ts_init).unwrap().unwrap();

        assert_eq!(bar.bar_type.instrument_id(), instrument.id());
        assert_eq!(bar.open, Price::new(0.001, PRICE_PRECISION));
        assert_eq!(bar.high, Price::new(0.0025, PRICE_PRECISION));
        assert_eq!(bar.low, Price::new(0.001, PRICE_PRECISION));
        assert_eq!(bar.close, Price::new(0.002, PRICE_PRECISION));
        assert_eq!(bar.volume, Quantity::new(1000.0, SIZE_PRECISION));
        assert_eq!(bar.ts_event, UnixNanos::from(1_638_747_719_999_000_000u64));
        assert_eq!(bar.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_kline_open_returns_none() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesKlineMsg = load_market_fixture("kline_stream_open.json");

        let bar = parse_kline(&msg, &instrument, UnixNanos::default()).unwrap();

        assert!(bar.is_none());
    }

    #[rstest]
    fn test_parse_mark_price_funding_rate_fields() {
        let instrument = sample_instrument();
        let msg: BinanceFuturesMarkPriceMsg = load_market_fixture("mark_price_stream.json");
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let (_mark, _index, funding) = parse_mark_price(&msg, &instrument, ts_init).unwrap();

        assert_eq!(funding.instrument_id, instrument.id());
        assert_eq!(funding.rate.to_string(), "0.00038167");
        assert!(funding.interval.is_none());
        assert_eq!(
            funding.next_funding_ns,
            Some(UnixNanos::from(1_562_306_400_000_000_000u64))
        );
        assert_eq!(
            funding.ts_event,
            UnixNanos::from(1_562_305_380_000_000_000u64)
        );
        assert_eq!(funding.ts_init, ts_init);
    }

    #[rstest]
    fn test_deserialize_liquidation_msg() {
        let msg: BinanceFuturesLiquidationMsg = load_market_fixture("liquidation_stream.json");

        assert_eq!(msg.event_type, "forceOrder");
        assert_eq!(msg.event_time, 1_568_014_460_893);
        assert_eq!(msg.order.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(msg.order.side, BinanceSide::Sell);
        assert_eq!(msg.order.original_qty, "0.014");
        assert_eq!(msg.order.average_price, "9910.12345678");
        assert_eq!(msg.order.status, BinanceOrderStatus::Filled);
        assert_eq!(msg.order.accumulated_qty, "0.014");
        assert_eq!(msg.order.trade_time, 1_568_014_460_893);
    }

    #[rstest]
    fn test_deserialize_ticker_msg() {
        let msg: BinanceFuturesTickerMsg = load_market_fixture("ticker_stream.json");

        assert_eq!(msg.event_type, "24hrTicker");
        assert_eq!(msg.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(msg.price_change, "-131.40000000");
        assert_eq!(msg.price_change_percent, "-0.786");
        assert_eq!(msg.weighted_avg_price, "16628.97377498");
        assert_eq!(msg.last_price, "16584.60000000");
        assert_eq!(msg.open_price, "16716.00000000");
        assert_eq!(msg.high_price, "16764.89000000");
        assert_eq!(msg.low_price, "16456.51000000");
        assert_eq!(msg.volume, "122474.816");
        assert_eq!(msg.quote_volume, "2036102085.69746400");
        assert_eq!(msg.num_trades, 142853);
    }

    #[rstest]
    fn test_extract_symbol() {
        let json = load_json_fixture("futures/market_data_json/book_ticker_stream.json");

        let symbol = extract_symbol(&json);

        assert_eq!(symbol, Some(Ustr::from("BNBUSDT")));
    }

    #[rstest]
    fn test_extract_event_type() {
        let json = load_json_fixture("futures/market_data_json/mark_price_stream.json");

        let event_type = extract_event_type(&json);

        assert_eq!(event_type, Some(BinanceWsEventType::MarkPriceUpdate));
    }

    #[rstest]
    fn test_extract_event_type_force_order() {
        let json = load_json_fixture("futures/market_data_json/liquidation_stream.json");

        let event_type = extract_event_type(&json);

        assert_eq!(event_type, Some(BinanceWsEventType::ForceOrder));
    }

    #[rstest]
    fn test_extract_event_type_ticker() {
        let json = load_json_fixture("futures/market_data_json/ticker_stream.json");

        let event_type = extract_event_type(&json);

        assert_eq!(event_type, Some(BinanceWsEventType::Ticker24Hr));
    }
}

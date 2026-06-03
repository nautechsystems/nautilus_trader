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

use std::str::FromStr;

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
use rust_decimal::Decimal;

use super::messages::{
    BinanceSpotBookTickerMsg, BinanceSpotKlineMsg, BinanceSpotPartialDepthMsg, BinanceSpotTradeMsg,
};
use crate::common::{
    enums::BinanceKlineInterval,
    parse::{parse_price_at_precision, parse_quantity_at_precision},
};

fn parse_positive_price(raw: &str, precision: u8, field: &str) -> anyhow::Result<Price> {
    parse_price_at_precision(raw, precision)
        .ok_or_else(|| anyhow::anyhow!("invalid {field} `{raw}`"))
}

fn parse_positive_quantity(raw: &str, precision: u8, field: &str) -> anyhow::Result<Quantity> {
    parse_quantity_at_precision(raw, precision)
        .ok_or_else(|| anyhow::anyhow!("invalid {field} `{raw}`"))
}

fn parse_non_negative_quantity(raw: &str, precision: u8, field: &str) -> anyhow::Result<Quantity> {
    let decimal = Decimal::from_str(raw).with_context(|| format!("invalid {field} `{raw}`"))?;
    if decimal.is_sign_negative() {
        anyhow::bail!("invalid {field} `{raw}`");
    }

    Quantity::from_decimal_dp(decimal, precision)
        .map_err(|e| anyhow::anyhow!("invalid {field} `{raw}`: {e}"))
}

/// Parses a trade message into a `TradeTick`.
///
/// # Errors
///
/// Returns an error if price or quantity fields cannot be parsed.
pub fn parse_trade(
    msg: &BinanceSpotTradeMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = parse_positive_price(&msg.price, price_precision, "trade price")?;
    let size = parse_positive_quantity(&msg.quantity, size_precision, "trade quantity")?;

    let aggressor_side = if msg.is_buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };

    let ts_event = UnixNanos::from_millis(msg.trade_time as u64);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        TradeId::new(msg.trade_id.to_string()),
        ts_event,
        ts_init,
    ))
}

/// Parses a book ticker message into a `QuoteTick`.
///
/// # Errors
///
/// Returns an error if price or quantity fields cannot be parsed.
pub fn parse_book_ticker(
    msg: &BinanceSpotBookTickerMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = parse_positive_price(&msg.best_bid_price, price_precision, "bid price")?;
    // A side that empties reports a zero size, which is a valid quote state.
    let bid_size = parse_non_negative_quantity(&msg.best_bid_qty, size_precision, "bid quantity")?;
    let ask_price = parse_positive_price(&msg.best_ask_price, price_precision, "ask price")?;
    let ask_size = parse_non_negative_quantity(&msg.best_ask_qty, size_precision, "ask quantity")?;

    // Spot bookTicker payloads on public streams do not consistently include
    // event timestamps; fall back to receive time when absent.
    let ts_event = msg
        .transaction_time
        .or(msg.event_time)
        .and_then(|ts| u64::try_from(ts).ok())
        .map_or(ts_init, UnixNanos::from_millis);

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

    for level in &msg.bids {
        let Some(price) = parse_price_at_precision(&level[0], price_precision) else {
            continue;
        };
        let Some(size) = parse_quantity_at_precision(&level[1], size_precision) else {
            continue;
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Buy, price, size, 0),
            0,
            0,
            ts_init,
            ts_init,
        ));
    }

    for level in &msg.asks {
        let Some(price) = parse_price_at_precision(&level[0], price_precision) else {
            continue;
        };
        let Some(size) = parse_quantity_at_precision(&level[1], size_precision) else {
            continue;
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Sell, price, size, 0),
            0,
            0,
            ts_init,
            ts_init,
        ));
    }

    if deltas.len() <= 1 {
        return None;
    }

    // Mark the final emitted delta as the snapshot terminator. Assigning F_LAST by
    // source index would drop the terminator whenever the last level fails to parse
    // and is skipped above.
    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
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
///
/// # Errors
///
/// Returns an error if any OHLCV field cannot be parsed.
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

    let open = parse_positive_price(&msg.kline.open, price_precision, "open price")?;
    let high = parse_positive_price(&msg.kline.high, price_precision, "high price")?;
    let low = parse_positive_price(&msg.kline.low, price_precision, "low price")?;
    let close = parse_positive_price(&msg.kline.close, price_precision, "close price")?;
    let volume = parse_non_negative_quantity(&msg.kline.volume, size_precision, "volume")?;

    let ts_event = UnixNanos::from_millis(msg.kline.close_time as u64);

    Ok(Some(Bar::new(
        bar_type, open, high, low, close, volume, ts_event, ts_init,
    )))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::parse::parse_spot_instrument_sbe,
        spot::http::models::{
            BinanceLotSizeFilterSbe, BinancePriceFilterSbe, BinanceSymbolFiltersSbe,
            BinanceSymbolSbe,
        },
    };

    fn sample_instrument() -> InstrumentAny {
        let symbol = BinanceSymbolSbe {
            symbol: "ETHUSDT".to_string(),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            base_asset_precision: 8,
            quote_asset_precision: 8,
            status: 0,
            order_types: 0,
            iceberg_allowed: true,
            oco_allowed: true,
            oto_allowed: false,
            quote_order_qty_market_allowed: true,
            allow_trailing_stop: true,
            cancel_replace_allowed: true,
            amend_allowed: true,
            is_spot_trading_allowed: true,
            is_margin_trading_allowed: false,
            filters: BinanceSymbolFiltersSbe {
                price_filter: Some(BinancePriceFilterSbe {
                    price_exponent: -8,
                    min_price: 1,
                    max_price: 100_000_000_000_000,
                    tick_size: 1,
                }),
                lot_size_filter: Some(BinanceLotSizeFilterSbe {
                    qty_exponent: -8,
                    min_qty: 1,
                    max_qty: 900_000_000_000,
                    step_size: 1,
                }),
            },
            permissions: vec![vec!["SPOT".to_string()]],
        };

        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        parse_spot_instrument_sbe(&symbol, ts, ts).unwrap()
    }

    #[rstest]
    fn test_parse_trade_preserves_decimal_precision() {
        let instrument = sample_instrument();
        let msg = BinanceSpotTradeMsg {
            event_type: "trade".to_string(),
            event_time: 1_700_000_000_000,
            symbol: Ustr::from("ETHUSDT"),
            trade_id: 42,
            price: "123.45678901".to_string(),
            quantity: "0.10000001".to_string(),
            trade_time: 1_700_000_000_001,
            is_buyer_maker: false,
        };

        let tick = parse_trade(&msg, &instrument, UnixNanos::from(1)).unwrap();
        assert_eq!(
            tick.price.as_decimal(),
            Decimal::from_str("123.45678901").unwrap()
        );
        assert_eq!(
            tick.size.as_decimal(),
            Decimal::from_str("0.10000001").unwrap()
        );
    }

    #[rstest]
    fn test_parse_book_ticker_preserves_decimal_precision() {
        let instrument = sample_instrument();
        let msg = BinanceSpotBookTickerMsg {
            event_type: None,
            event_time: None,
            symbol: Ustr::from("ETHUSDT"),
            book_update_id: 100,
            best_bid_price: "123.45678901".to_string(),
            best_bid_qty: "1.23000000".to_string(),
            best_ask_price: "123.45678909".to_string(),
            best_ask_qty: "4.56000000".to_string(),
            transaction_time: Some(1_700_000_000_002),
        };

        let quote = parse_book_ticker(&msg, &instrument, UnixNanos::from(1)).unwrap();
        assert_eq!(
            quote.bid_price.as_decimal(),
            Decimal::from_str("123.45678901").unwrap()
        );
        assert_eq!(
            quote.ask_price.as_decimal(),
            Decimal::from_str("123.45678909").unwrap()
        );
        assert_eq!(
            quote.bid_size.as_decimal(),
            Decimal::from_str("1.23000000").unwrap()
        );
        assert_eq!(
            quote.ask_size.as_decimal(),
            Decimal::from_str("4.56000000").unwrap()
        );
    }

    #[rstest]
    fn test_parse_book_ticker_accepts_zero_bid_size() {
        let instrument = sample_instrument();
        // A side that empties reports a zero size; the quote must still be produced.
        let msg = BinanceSpotBookTickerMsg {
            event_type: None,
            event_time: None,
            symbol: Ustr::from("ETHUSDT"),
            book_update_id: 1,
            best_bid_price: "100.00000000".to_string(),
            best_bid_qty: "0.00000000".to_string(),
            best_ask_price: "101.00000000".to_string(),
            best_ask_qty: "1.00000000".to_string(),
            transaction_time: None,
        };

        let quote = parse_book_ticker(&msg, &instrument, UnixNanos::from(1))
            .expect("zero bid size is a valid quote");
        assert_eq!(quote.bid_size.as_decimal(), Decimal::from_str("0").unwrap());
    }

    #[rstest]
    fn test_parse_depth_snapshot_sets_last_flag_when_final_level_skipped() {
        let instrument = sample_instrument();
        // The final ask level has a zero quantity and is skipped during parsing; the
        // F_LAST terminator must still land on the last emitted delta.
        let msg = BinanceSpotPartialDepthMsg {
            symbol: Ustr::from("ETHUSDT"),
            last_update_id: 1,
            bids: vec![["100.00000000".to_string(), "1.00000000".to_string()]],
            asks: vec![
                ["101.00000000".to_string(), "2.00000000".to_string()],
                ["102.00000000".to_string(), "0.00000000".to_string()],
            ],
        };

        let deltas = parse_depth_snapshot(&msg, &instrument, UnixNanos::from(1))
            .expect("snapshot should produce deltas");

        let last = deltas.deltas.last().expect("at least one delta");
        assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
    }
}

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
        BarSpecification, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
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
    BinanceSpotBookTickerMsg, BinanceSpotDepthDiffMsg, BinanceSpotKlineMsg,
    BinanceSpotPartialDepthMsg, BinanceSpotTickerMsg, BinanceSpotTradeMsg,
};
use crate::{
    common::{
        bar::BinanceBar,
        enums::BinanceKlineInterval,
        parse::{parse_millis_or_init, parse_price_at_precision, parse_quantity_at_precision},
    },
    data_types::BinanceSpotTicker,
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
        AggressorSide::Sell
    } else {
        AggressorSide::Buy
    };

    let ts_event = parse_millis_or_init(msg.trade_time, "Spot JSON trade time", ts_init);

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
        .map_or(ts_init, |value| {
            parse_millis_or_init(value, "Spot JSON book ticker time", ts_init)
        });

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

/// Parses a depth diff message into `OrderBookDeltas`.
///
/// # Errors
///
/// Returns an error if any price or quantity update cannot be parsed.
pub fn parse_depth_diff(
    msg: &BinanceSpotDepthDiffMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_millis_or_init(msg.event_time, "Spot JSON depth event time", ts_init);
    let sequence = msg.final_update_id;

    let mut deltas = Vec::with_capacity(msg.bids.len() + msg.asks.len());

    for (i, level) in msg.bids.iter().enumerate() {
        let price = parse_positive_price(&level[0], price_precision, "bid price")?;
        let size = parse_non_negative_quantity(&level[1], size_precision, "bid quantity")?;
        let action = if size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };
        let flags = if i == msg.bids.len() - 1 && msg.asks.is_empty() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            BookOrder::new(OrderSide::Buy, price, size, 0),
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let price = parse_positive_price(&level[0], price_precision, "ask price")?;
        let size = parse_non_negative_quantity(&level[1], size_precision, "ask quantity")?;
        let action = if size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };
        let flags = if i == msg.asks.len() - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            BookOrder::new(OrderSide::Sell, price, size, 0),
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    if deltas.is_empty() {
        return Ok(None);
    }

    Ok(Some(OrderBookDeltas::new(instrument_id, deltas)))
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
) -> anyhow::Result<Option<BinanceBar>> {
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
    let quote_volume = Decimal::from_str(&msg.kline.quote_volume)
        .with_context(|| format!("invalid quote volume `{}`", msg.kline.quote_volume))?;
    let taker_buy_base_volume =
        Decimal::from_str(&msg.kline.taker_buy_base_volume).with_context(|| {
            format!(
                "invalid taker buy base volume `{}`",
                msg.kline.taker_buy_base_volume
            )
        })?;
    let taker_buy_quote_volume = Decimal::from_str(&msg.kline.taker_buy_quote_volume)
        .with_context(|| {
            format!(
                "invalid taker buy quote volume `{}`",
                msg.kline.taker_buy_quote_volume
            )
        })?;
    let count = u64::try_from(msg.kline.num_trades).map_err(|_| {
        anyhow::anyhow!(
            "invalid negative kline trade count {}",
            msg.kline.num_trades
        )
    })?;

    let ts_event =
        parse_millis_or_init(msg.kline.close_time, "Spot JSON kline close time", ts_init);

    Ok(Some(BinanceBar::new(
        bar_type,
        open,
        high,
        low,
        close,
        volume,
        quote_volume,
        count,
        taker_buy_base_volume,
        taker_buy_quote_volume,
        ts_event,
        ts_init,
    )))
}

/// Parses a rolling 24-hour ticker message.
///
/// # Errors
///
/// Returns an error if any numeric field is invalid.
pub fn parse_ticker(
    msg: &BinanceSpotTickerMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<BinanceSpotTicker> {
    let decimal = |field: &str, value: &str| {
        Decimal::from_str(value).with_context(|| format!("invalid {field} `{value}`"))
    };
    let millis = |field: &str, value: i64| parse_millis_or_init(value, field, ts_init);

    Ok(BinanceSpotTicker {
        instrument_id: instrument.id(),
        price_change: decimal("price change", &msg.price_change)?,
        price_change_percent: decimal("price change percent", &msg.price_change_percent)?,
        weighted_avg_price: decimal("weighted average price", &msg.weighted_avg_price)?,
        prev_close_price: decimal("previous close price", &msg.prev_close_price)?,
        last_price: decimal("last price", &msg.last_price)?,
        last_qty: decimal("last quantity", &msg.last_qty)?,
        bid_price: decimal("bid price", &msg.bid_price)?,
        bid_qty: decimal("bid quantity", &msg.bid_qty)?,
        ask_price: decimal("ask price", &msg.ask_price)?,
        ask_qty: decimal("ask quantity", &msg.ask_qty)?,
        open_price: decimal("open price", &msg.open_price)?,
        high_price: decimal("high price", &msg.high_price)?,
        low_price: decimal("low price", &msg.low_price)?,
        volume: decimal("volume", &msg.volume)?,
        quote_volume: decimal("quote volume", &msg.quote_volume)?,
        open_time: millis("Spot JSON ticker open time", msg.open_time),
        close_time: millis("Spot JSON ticker close time", msg.close_time),
        first_trade_id: msg.first_trade_id,
        last_trade_id: msg.last_trade_id,
        num_trades: msg.num_trades,
        ts_event: millis("Spot JSON ticker event time", msg.event_time),
        ts_init,
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
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
    #[case::negative(-1)]
    #[case::overflow(i64::MAX)]
    fn test_parse_trade_falls_back_for_invalid_timestamp(#[case] trade_time: i64) {
        let instrument = sample_instrument();
        let msg = BinanceSpotTradeMsg {
            event_type: "trade".to_string(),
            event_time: 1_700_000_000_000,
            symbol: Ustr::from("ETHUSDT"),
            trade_id: 42,
            price: "123.45678901".to_string(),
            quantity: "0.10000001".to_string(),
            trade_time,
            is_buyer_maker: false,
        };

        let ts_init = UnixNanos::from(1);
        let trade = parse_trade(&msg, &instrument, ts_init).unwrap();

        assert_eq!(trade.ts_event, ts_init);
        assert_eq!(trade.ts_init, ts_init);
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

    #[rstest]
    fn test_parse_depth_diff_sets_delete_actions_and_last_flag_on_final_ask() {
        let instrument = sample_instrument();
        let msg = BinanceSpotDepthDiffMsg {
            event_type: "depthUpdate".to_string(),
            event_time: 1_700_000_000_000,
            symbol: Ustr::from("ETHUSDT"),
            first_update_id: 10,
            final_update_id: 12,
            bids: vec![
                ["100.00000000".to_string(), "1.00000000".to_string()],
                ["99.00000000".to_string(), "0.00000000".to_string()],
            ],
            asks: vec![
                ["101.00000000".to_string(), "2.00000000".to_string()],
                ["102.00000000".to_string(), "0.00000000".to_string()],
            ],
        };

        let deltas = parse_depth_diff(&msg, &instrument, UnixNanos::from(1))
            .unwrap()
            .expect("depth diff should produce deltas");

        assert_eq!(deltas.sequence, 12);
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy.into());
        assert_eq!(deltas.deltas[0].flags, 0);
        assert_eq!(deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy.into());
        assert_eq!(deltas.deltas[1].order.size.as_decimal(), Decimal::ZERO);
        assert_eq!(deltas.deltas[1].flags, 0);
        assert_eq!(deltas.deltas[2].action, BookAction::Update);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell.into());
        assert_eq!(deltas.deltas[2].flags, 0);
        assert_eq!(deltas.deltas[3].action, BookAction::Delete);
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell.into());
        assert_eq!(deltas.deltas[3].order.size.as_decimal(), Decimal::ZERO);
        assert_eq!(deltas.deltas[3].flags, RecordFlag::F_LAST as u8);
    }

    #[rstest]
    fn test_parse_closed_one_second_kline_preserves_extended_fields() {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(1_700_000_001_234_567_890_u64);
        let msg = BinanceSpotKlineMsg {
            event_type: "kline".to_string(),
            event_time: 1_700_000_000_999,
            symbol: Ustr::from("ETHUSDT"),
            kline: super::super::messages::BinanceSpotKlineData {
                start_time: 1_700_000_000_000,
                close_time: 1_700_000_000_999,
                symbol: Ustr::from("ETHUSDT"),
                interval: BinanceKlineInterval::Second1,
                first_trade_id: 201,
                last_trade_id: 207,
                open: "123.45678901".to_string(),
                close: "124.56789012".to_string(),
                high: "125.67890123".to_string(),
                low: "122.34567890".to_string(),
                volume: "7.65432109".to_string(),
                num_trades: 7,
                is_closed: true,
                quote_volume: "951.35792468".to_string(),
                taker_buy_base_volume: "3.21098765".to_string(),
                taker_buy_quote_volume: "399.86420864".to_string(),
            },
        };

        let bar = parse_kline(&msg, &instrument, ts_init).unwrap().unwrap();

        assert_eq!(
            bar.bar_type,
            BarType::from("ETHUSDT.BINANCE-1-SECOND-LAST-EXTERNAL")
        );
        assert_eq!(bar.open, Price::from("123.45678901"));
        assert_eq!(bar.high, Price::from("125.67890123"));
        assert_eq!(bar.low, Price::from("122.34567890"));
        assert_eq!(bar.close, Price::from("124.56789012"));
        assert_eq!(bar.volume, Quantity::from("7.65432109"));
        assert_eq!(bar.quote_volume, dec!(951.35792468));
        assert_eq!(bar.count, 7);
        assert_eq!(bar.taker_buy_base_volume, dec!(3.21098765));
        assert_eq!(bar.taker_buy_quote_volume, dec!(399.86420864));
        assert_eq!(bar.ts_event, UnixNanos::from(1_700_000_000_999_000_000_u64));
        assert_eq!(bar.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_open_kline_returns_none() {
        let instrument = sample_instrument();
        let msg: BinanceSpotKlineMsg = serde_json::from_value(serde_json::json!({
            "e": "kline",
            "E": 1700000000999_i64,
            "s": "ETHUSDT",
            "k": {
                "t": 1700000000000_i64,
                "T": 1700000000999_i64,
                "s": "ETHUSDT",
                "i": "1s",
                "f": 201,
                "L": 207,
                "o": "123.45678901",
                "c": "124.56789012",
                "h": "125.67890123",
                "l": "122.34567890",
                "v": "7.65432109",
                "n": 7,
                "x": false,
                "q": "951.35792468",
                "V": "3.21098765",
                "Q": "399.86420864"
            }
        }))
        .unwrap();

        assert!(
            parse_kline(&msg, &instrument, UnixNanos::from(1))
                .unwrap()
                .is_none()
        );
    }

    #[rstest]
    fn test_parse_spot_ticker_preserves_all_fields() {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(1_700_000_001_234_567_890_u64);
        let msg = BinanceSpotTickerMsg {
            event_time: 1_700_000_000_999,
            symbol: Ustr::from("ETHUSDT"),
            price_change: "1.00000001".to_string(),
            price_change_percent: "2.00000002".to_string(),
            weighted_avg_price: "3.00000003".to_string(),
            prev_close_price: "4.00000004".to_string(),
            last_price: "5.00000005".to_string(),
            last_qty: "6.00000006".to_string(),
            bid_price: "7.00000007".to_string(),
            bid_qty: "8.00000008".to_string(),
            ask_price: "9.00000009".to_string(),
            ask_qty: "10.00000010".to_string(),
            open_price: "11.00000011".to_string(),
            high_price: "12.00000012".to_string(),
            low_price: "13.00000013".to_string(),
            volume: "14.00000014".to_string(),
            quote_volume: "15.00000015".to_string(),
            open_time: 1_699_913_600_999,
            close_time: 1_700_000_000_998,
            first_trade_id: 301,
            last_trade_id: 399,
            num_trades: 99,
        };

        let ticker = parse_ticker(&msg, &instrument, ts_init).unwrap();

        assert_eq!(ticker.instrument_id, instrument.id());
        assert_eq!(ticker.price_change, dec!(1.00000001));
        assert_eq!(ticker.price_change_percent, dec!(2.00000002));
        assert_eq!(ticker.weighted_avg_price, dec!(3.00000003));
        assert_eq!(ticker.prev_close_price, dec!(4.00000004));
        assert_eq!(ticker.last_price, dec!(5.00000005));
        assert_eq!(ticker.last_qty, dec!(6.00000006));
        assert_eq!(ticker.bid_price, dec!(7.00000007));
        assert_eq!(ticker.bid_qty, dec!(8.00000008));
        assert_eq!(ticker.ask_price, dec!(9.00000009));
        assert_eq!(ticker.ask_qty, dec!(10.00000010));
        assert_eq!(ticker.open_price, dec!(11.00000011));
        assert_eq!(ticker.high_price, dec!(12.00000012));
        assert_eq!(ticker.low_price, dec!(13.00000013));
        assert_eq!(ticker.volume, dec!(14.00000014));
        assert_eq!(ticker.quote_volume, dec!(15.00000015));
        assert_eq!(
            ticker.open_time,
            UnixNanos::from(1_699_913_600_999_000_000_u64)
        );
        assert_eq!(
            ticker.close_time,
            UnixNanos::from(1_700_000_000_998_000_000_u64)
        );
        assert_eq!(ticker.first_trade_id, 301);
        assert_eq!(ticker.last_trade_id, 399);
        assert_eq!(ticker.num_trades, 99);
        assert_eq!(
            ticker.ts_event,
            UnixNanos::from(1_700_000_000_999_000_000_u64)
        );
        assert_eq!(ticker.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_spot_ticker_rejects_invalid_decimal() {
        let instrument = sample_instrument();
        let mut msg: BinanceSpotTickerMsg = serde_json::from_value(serde_json::json!({
            "E": 1700000000999_i64,
            "s": "ETHUSDT",
            "p": "1.1",
            "P": "2.2",
            "w": "3.3",
            "x": "4.4",
            "c": "5.5",
            "Q": "6.6",
            "b": "7.7",
            "B": "8.8",
            "a": "9.9",
            "A": "10.1",
            "o": "11.1",
            "h": "12.1",
            "l": "13.1",
            "v": "14.1",
            "q": "15.1",
            "O": 1699913600999_i64,
            "C": 1700000000998_i64,
            "F": 301,
            "L": 399,
            "n": 99
        }))
        .unwrap();
        msg.quote_volume = "invalid".to_string();

        let error = parse_ticker(&msg, &instrument, UnixNanos::from(1)).unwrap_err();

        assert!(error.to_string().contains("invalid quote volume `invalid`"));
    }
}

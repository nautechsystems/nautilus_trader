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

//! Parsers for Derive public WebSocket subscription payloads.

use anyhow::Context;
use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, greeks::OptionGreekValues,
        option_chain::OptionGreeks,
    },
    enums::{AggressorSide, BarAggregation, BookAction, GreeksConvention, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use rust_decimal::prelude::ToPrimitive;
use ustr::Ustr;

use super::messages::{
    DeriveOrderbookData, DeriveOrderbookLevel, DeriveOrderbookMsg, DerivePublicWsData,
    DeriveTickerData, DeriveTickerMsg, DeriveTradesMsg, WsSubscriptionPayload,
};
use crate::{
    common::{enums::DeriveOrderSide, parse::format_instrument_id},
    http::models::{
        DerivePublicCandle, DerivePublicFundingRate, DerivePublicTrade, DeriveTickerSnapshot,
    },
};

/// Parses a Derive public subscription payload into a typed market data update.
///
/// # Errors
///
/// Returns an error when the channel is unsupported or `params.data` does not
/// match the channel payload shape.
pub fn parse_public_ws_data(payload: &WsSubscriptionPayload) -> anyhow::Result<DerivePublicWsData> {
    let channel = payload.channel.as_str();

    if channel.starts_with("orderbook.") {
        return parse_orderbook_msg(payload).map(DerivePublicWsData::Orderbook);
    }

    if channel.starts_with("trades.") {
        return parse_trades_msg(payload).map(DerivePublicWsData::Trades);
    }

    if channel.starts_with("ticker_slim.") || channel.starts_with("ticker.") {
        return parse_ticker_msg(payload).map(|msg| DerivePublicWsData::Ticker(Box::new(msg)));
    }

    anyhow::bail!("unsupported Derive public WS channel `{}`", payload.channel)
}

/// Parses an order book subscription payload.
///
/// # Errors
///
/// Returns an error when `params.data` is not a Derive order book snapshot.
pub fn parse_orderbook_msg(payload: &WsSubscriptionPayload) -> anyhow::Result<DeriveOrderbookMsg> {
    let data = serde_json::from_value::<DeriveOrderbookData>(payload.data.clone())
        .context("failed to decode Derive orderbook data")?;
    Ok(DeriveOrderbookMsg {
        channel: Ustr::from(payload.channel.as_str()),
        data,
    })
}

/// Parses a public trades subscription payload.
///
/// # Errors
///
/// Returns an error when `params.data` is not a list of Derive public trades.
pub fn parse_trades_msg(payload: &WsSubscriptionPayload) -> anyhow::Result<DeriveTradesMsg> {
    let trades = serde_json::from_value::<Vec<DerivePublicTrade>>(payload.data.clone())
        .context("failed to decode Derive trades data")?;
    Ok(DeriveTradesMsg {
        channel: Ustr::from(payload.channel.as_str()),
        trades,
    })
}

/// Parses a ticker subscription payload.
///
/// # Errors
///
/// Returns an error when `params.data` is not a Derive ticker payload.
pub fn parse_ticker_msg(payload: &WsSubscriptionPayload) -> anyhow::Result<DeriveTickerMsg> {
    let mut data = serde_json::from_value::<DeriveTickerData>(payload.data.clone())
        .context("failed to decode Derive ticker data")?;
    data.apply_channel_context(payload.channel.as_str())
        .map_err(anyhow::Error::msg)?;
    Ok(DeriveTickerMsg {
        channel: Ustr::from(payload.channel.as_str()),
        data,
    })
}

/// Parses an order book snapshot message into Nautilus snapshot deltas.
///
/// Derive's grouped order book stream sends a full depth snapshot for the
/// requested grouping and depth, so the output starts with a clear delta and
/// marks the last add with `F_LAST`. The payload does not include a change ID,
/// so this uses the feed timestamp as the snapshot sequence.
///
/// Pass price and size precision from the instrument definition rather than
/// inferring them from the wire values, since Derive may trim trailing zeroes.
///
/// # Errors
///
/// Returns an error when a price, size, or timestamp cannot be converted.
pub fn parse_orderbook_deltas(
    msg: &DeriveOrderbookMsg,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = msg.data.instrument_id();
    let timestamp =
        u64::try_from(msg.data.timestamp).context("negative Derive orderbook timestamp")?;
    let ts_event = timestamp_millis_to_nanos(timestamp, "timestamp")?;
    let sequence = timestamp;
    let context = BookDeltaContext {
        instrument_id,
        sequence,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
    };

    let mut deltas = Vec::with_capacity(1 + msg.data.bids.len() + msg.data.asks.len());
    let clear_flags = if msg.data.bids.is_empty() && msg.data.asks.is_empty() {
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
    } else {
        RecordFlag::F_SNAPSHOT as u8
    };
    deltas.push(OrderBookDelta::new_checked(
        context.instrument_id,
        BookAction::Clear,
        BookOrder::default(),
        clear_flags,
        context.sequence,
        context.ts_event,
        context.ts_init,
    )?);

    for (idx, level) in msg.data.bids.iter().enumerate() {
        push_level_delta(&mut deltas, &context, OrderSide::Buy, level, idx as u64)?;
    }

    let bid_count = msg.data.bids.len();
    for (idx, level) in msg.data.asks.iter().enumerate() {
        push_level_delta(
            &mut deltas,
            &context,
            OrderSide::Sell,
            level,
            (bid_count + idx) as u64,
        )?;
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(context.instrument_id, deltas)
}

/// Parses a public trade message into a Nautilus trade tick.
///
/// Pass price and size precision from the instrument definition rather than
/// inferring them from the wire values, since Derive may trim trailing zeroes.
///
/// # Errors
///
/// Returns an error when price, size, or timestamp conversion fails.
pub fn parse_trade_tick(
    trade: &DerivePublicTrade,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = format_instrument_id(trade.instrument_name.as_str());
    let price = Price::from_decimal_dp(trade.trade_price, price_precision)
        .with_context(|| format!("invalid trade price for {}", trade.instrument_name))?;
    let size = Quantity::from_decimal_dp(trade.trade_amount, size_precision)
        .with_context(|| format!("invalid trade amount for {}", trade.instrument_name))?;
    let aggressor_side = match trade.direction {
        DeriveOrderSide::Buy => AggressorSide::Buyer,
        DeriveOrderSide::Sell => AggressorSide::Seller,
    };
    let trade_id = TradeId::new(&trade.trade_id);
    let timestamp = u64::try_from(trade.timestamp).context("negative Derive trade timestamp")?;
    let ts_event = timestamp_millis_to_nanos(timestamp, "timestamp")?;

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a ticker message into a Nautilus top-of-book quote.
///
/// Pass price and size precision from the instrument definition rather than
/// inferring them from the wire values, since Derive may trim trailing zeroes.
///
/// # Errors
///
/// Returns an error when price, size, or timestamp conversion fails.
pub fn parse_ticker_quote(
    msg: &DeriveTickerMsg,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = msg.data.instrument_id();
    let instrument_name = msg.data.instrument_name().as_str();
    let bid_price = Price::from_decimal_dp(msg.data.best_bid_price(), price_precision)
        .with_context(|| format!("invalid bid price for {instrument_name}"))?;
    let ask_price = Price::from_decimal_dp(msg.data.best_ask_price(), price_precision)
        .with_context(|| format!("invalid ask price for {instrument_name}"))?;
    let bid_size = Quantity::from_decimal_dp(msg.data.best_bid_amount(), size_precision)
        .with_context(|| format!("invalid bid amount for {instrument_name}"))?;
    let ask_size = Quantity::from_decimal_dp(msg.data.best_ask_amount(), size_precision)
        .with_context(|| format!("invalid ask amount for {instrument_name}"))?;
    let timestamp =
        u64::try_from(msg.data.timestamp()).context("negative Derive ticker timestamp")?;
    let ts_event = timestamp_millis_to_nanos(timestamp, "timestamp")?;

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

/// Parses a REST `public/get_tickers` snapshot into a Nautilus top-of-book quote.
///
/// # Errors
///
/// Returns an error when price, size, or timestamp conversion fails.
pub fn parse_ticker_quote_from_rest(
    ticker: &DeriveTickerSnapshot,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = format_instrument_id(ticker.instrument_name.as_str());
    let instrument_name = ticker.instrument_name.as_str();
    let bid_price = Price::from_decimal_dp(ticker.best_bid_price, price_precision)
        .with_context(|| format!("invalid bid price for {instrument_name}"))?;
    let ask_price = Price::from_decimal_dp(ticker.best_ask_price, price_precision)
        .with_context(|| format!("invalid ask price for {instrument_name}"))?;
    let bid_size = Quantity::from_decimal_dp(ticker.best_bid_amount, size_precision)
        .with_context(|| format!("invalid bid amount for {instrument_name}"))?;
    let ask_size = Quantity::from_decimal_dp(ticker.best_ask_amount, size_precision)
        .with_context(|| format!("invalid ask amount for {instrument_name}"))?;
    let timestamp = u64::try_from(ticker.timestamp).context("negative Derive ticker timestamp")?;
    let ts_event = timestamp_millis_to_nanos(timestamp, "timestamp")?;

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[derive(Debug, Clone, Copy)]
struct BookDeltaContext {
    instrument_id: InstrumentId,
    sequence: u64,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

fn push_level_delta(
    deltas: &mut Vec<OrderBookDelta>,
    context: &BookDeltaContext,
    side: OrderSide,
    level: &DeriveOrderbookLevel,
    order_id: u64,
) -> anyhow::Result<()> {
    if level.amount().is_zero() {
        return Ok(());
    }

    let price = Price::from_decimal_dp(level.price(), context.price_precision)
        .context("invalid Derive orderbook price")?;
    let size = Quantity::from_decimal_dp(level.amount(), context.size_precision)
        .context("invalid Derive orderbook amount")?;
    let order = BookOrder::new(side, price, size, order_id);
    deltas.push(OrderBookDelta::new_checked(
        context.instrument_id,
        BookAction::Add,
        order,
        RecordFlag::F_SNAPSHOT as u8,
        context.sequence,
        context.ts_event,
        context.ts_init,
    )?);
    Ok(())
}

fn timestamp_millis_to_nanos(value: u64, field: &str) -> anyhow::Result<UnixNanos> {
    let nanos = value
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .with_context(|| format!("Derive {field} overflows nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

fn ticker_ts_event(timestamp_ms: i64) -> anyhow::Result<UnixNanos> {
    let timestamp = u64::try_from(timestamp_ms).context("negative Derive ticker timestamp")?;
    timestamp_millis_to_nanos(timestamp, "timestamp")
}

/// Parses a ticker payload into a [`MarkPriceUpdate`].
///
/// # Errors
///
/// Returns an error when the ticker timestamp is negative or overflows.
pub fn parse_mark_price(
    msg: &DeriveTickerMsg,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<MarkPriceUpdate>> {
    let instrument_id = msg.data.instrument_id();
    let value = Price::from_decimal_dp(msg.data.mark_price(), price_precision)
        .with_context(|| format!("invalid Derive mark price for {instrument_id}"))?;
    let ts_event = ticker_ts_event(msg.data.timestamp())?;
    Ok(Some(MarkPriceUpdate::new(
        instrument_id,
        value,
        ts_event,
        ts_init,
    )))
}

/// Parses a ticker payload into an [`IndexPriceUpdate`].
///
/// # Errors
///
/// Returns an error when the ticker timestamp is negative or overflows.
pub fn parse_index_price(
    msg: &DeriveTickerMsg,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<IndexPriceUpdate>> {
    let instrument_id = msg.data.instrument_id();
    let value = Price::from_decimal_dp(msg.data.index_price(), price_precision)
        .with_context(|| format!("invalid Derive index price for {instrument_id}"))?;
    let ts_event = ticker_ts_event(msg.data.timestamp())?;
    Ok(Some(IndexPriceUpdate::new(
        instrument_id,
        value,
        ts_event,
        ts_init,
    )))
}

/// Parses a perpetual ticker payload into a [`FundingRateUpdate`].
///
/// Returns `Ok(None)` when the ticker does not carry funding.
///
/// # Errors
///
/// Returns an error when the ticker timestamp is negative or overflows.
pub fn parse_funding_rate(
    msg: &DeriveTickerMsg,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<FundingRateUpdate>> {
    let Some(rate) = msg.data.funding_rate() else {
        return Ok(None);
    };
    let instrument_id = msg.data.instrument_id();
    let ts_event = ticker_ts_event(msg.data.timestamp())?;
    Ok(Some(FundingRateUpdate::new(
        instrument_id,
        rate,
        None,
        None,
        ts_event,
        ts_init,
    )))
}

/// Parses a `public/get_funding_rate_history` record into a [`FundingRateUpdate`].
///
/// # Errors
///
/// Returns an error when the record timestamp is negative or overflows.
pub fn parse_funding_rate_history_record(
    record: &DerivePublicFundingRate,
    instrument_id: InstrumentId,
    interval: Option<u16>,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    let ts_event = ticker_ts_event(record.timestamp)?;
    Ok(FundingRateUpdate::new(
        instrument_id,
        record.funding_rate,
        interval,
        None,
        ts_event,
        ts_init,
    ))
}

/// Parses a `public/get_tradingview_chart_data` record into a Nautilus [`Bar`].
///
/// Pass price and size precision from the instrument definition rather than
/// inferring them from the wire values. The Derive `timestamp_bucket` is the
/// bucket start in UNIX seconds; the returned bar's `ts_event` marks that
/// start (not the close).
///
/// # Errors
///
/// Returns an error when price, size, or timestamp conversion fails.
pub fn parse_candle_record(
    record: &DerivePublicCandle,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = Price::from_decimal_dp(record.open_price, price_precision)
        .context("invalid Derive candle open price")?;
    let high = Price::from_decimal_dp(record.high_price, price_precision)
        .context("invalid Derive candle high price")?;
    let low = Price::from_decimal_dp(record.low_price, price_precision)
        .context("invalid Derive candle low price")?;
    let close = Price::from_decimal_dp(record.close_price, price_precision)
        .context("invalid Derive candle close price")?;
    let volume = Quantity::from_decimal_dp(record.volume_contracts, size_precision)
        .context("invalid Derive candle volume")?;
    let timestamp =
        u64::try_from(record.timestamp_bucket).context("negative Derive candle timestamp")?;
    let ts_event = timestamp_seconds_to_nanos(timestamp, "candle timestamp_bucket")?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Derive candle record")
}

/// Maps a Nautilus bar aggregation and step to the Derive `period` enum value
/// (bucket size in seconds).
///
/// Derive supports the following bucket sizes: 60, 300, 900, 1800, 3600,
/// 14400, 28800, 86400, 604800.
///
/// # Errors
///
/// Returns an error if the aggregation or step has no Derive equivalent.
pub fn bar_spec_to_derive_period(aggregation: BarAggregation, step: u64) -> anyhow::Result<u32> {
    match aggregation {
        BarAggregation::Minute => match step {
            1 => Ok(60),
            5 => Ok(300),
            15 => Ok(900),
            30 => Ok(1800),
            _ => anyhow::bail!(
                "Derive only supports minute intervals 1, 5, 15, 30 (use HOUR for >= 60)"
            ),
        },
        BarAggregation::Hour => match step {
            1 => Ok(3600),
            4 => Ok(14400),
            8 => Ok(28800),
            _ => anyhow::bail!("Derive only supports hour intervals 1, 4, 8"),
        },
        BarAggregation::Day => {
            if step != 1 {
                anyhow::bail!("Derive only supports 1 DAY interval bars");
            }
            Ok(86400)
        }
        BarAggregation::Week => {
            if step != 1 {
                anyhow::bail!("Derive only supports 1 WEEK interval bars");
            }
            Ok(604800)
        }
        _ => anyhow::bail!("Derive does not support {aggregation:?} bars"),
    }
}

fn timestamp_seconds_to_nanos(value: u64, field: &str) -> anyhow::Result<UnixNanos> {
    let nanos = value
        .checked_mul(NANOSECONDS_IN_SECOND)
        .with_context(|| format!("Derive {field} overflows nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

/// Parses an option ticker payload into [`OptionGreeks`].
///
/// Returns `Ok(None)` when the ticker does not carry option pricing.
///
/// # Errors
///
/// Returns an error when the ticker timestamp is negative or overflows.
pub fn parse_option_greeks(
    msg: &DeriveTickerMsg,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OptionGreeks>> {
    let Some(pricing) = msg.data.option_pricing() else {
        return Ok(None);
    };
    let instrument_id = msg.data.instrument_id();
    let ts_event = ticker_ts_event(msg.data.timestamp())?;
    let to_f64 = |label: &str, value: rust_decimal::Decimal| {
        value
            .to_f64()
            .ok_or_else(|| anyhow::anyhow!("Derive {label} cannot be represented as f64"))
    };

    Ok(Some(OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: to_f64("delta", pricing.delta)?,
            gamma: to_f64("gamma", pricing.gamma)?,
            vega: to_f64("vega", pricing.vega)?,
            theta: to_f64("theta", pricing.theta)?,
            rho: to_f64("rho", pricing.rho)?,
        },
        mark_iv: Some(to_f64("iv", pricing.iv)?),
        bid_iv: Some(to_f64("bid_iv", pricing.bid_iv)?),
        ask_iv: Some(to_f64("ask_iv", pricing.ask_iv)?),
        underlying_price: Some(to_f64("forward_price", pricing.forward_price)?),
        open_interest: msg
            .data
            .stats()
            .map(|s| to_f64("open_interest", s.open_interest))
            .transpose()?,
        ts_event,
        ts_init,
    }))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use nautilus_model::{
        enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};

    use super::*;
    use crate::websocket::messages::DeriveWsFrame;

    const PRICE_PRECISION: u8 = 2;
    const SIZE_PRECISION: u8 = 3;
    const INVALID_PRECISION: u8 = u8::MAX;

    fn data_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    fn load_json(filename: &str) -> Value {
        let content = std::fs::read_to_string(data_path().join(filename))
            .unwrap_or_else(|_| panic!("failed to read {filename}"));
        serde_json::from_str(&content).expect("invalid json")
    }

    fn subscription_payload(frame: &Value) -> WsSubscriptionPayload {
        match DeriveWsFrame::parse(&frame.to_string()).unwrap() {
            DeriveWsFrame::Subscription(payload) => payload,
            other => panic!("expected subscription frame, was {other:?}"),
        }
    }

    fn subscription_data_payload(channel: &str, data: &Value) -> WsSubscriptionPayload {
        subscription_payload(&json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": channel,
                "data": data
            }
        }))
    }

    fn orderbook_json(timestamp: i64, bids: &Value, asks: &Value) -> Value {
        let mut value = load_json("perps/ws_orderbook_eth.json");
        value["timestamp"] = json!(timestamp);
        value["bids"] = bids.clone();
        value["asks"] = asks.clone();
        value
    }

    fn trade_json(timestamp: i64, direction: &str) -> Value {
        trade_json_with_values(timestamp, direction, "3500.2", "0.25")
    }

    fn trade_json_with_values(
        timestamp: i64,
        direction: &str,
        trade_price: &str,
        trade_amount: &str,
    ) -> Value {
        let mut value = load_json("perps/ws_trade_eth.json");
        value["direction"] = json!(direction);
        value["timestamp"] = json!(timestamp);
        value["trade_amount"] = json!(trade_amount);
        value["trade_id"] = json!("trade-1");
        value["trade_price"] = json!(trade_price);
        value
    }

    fn ticker_json_with_timestamp(timestamp: i64) -> Value {
        let mut value = load_json("perps/ws_ticker_eth.json");
        value["best_ask_amount"] = json!("1.20");
        value["best_ask_price"] = json!("3501.00");
        value["best_bid_amount"] = json!("0.80");
        value["best_bid_price"] = json!("3499.50");
        value["timestamp"] = json!(timestamp);
        value
    }

    fn ticker_json() -> Value {
        ticker_json_with_timestamp(1_700_000_000_000)
    }

    fn price(value: &str) -> Price {
        Price::from_decimal_dp(Decimal::from_str(value).unwrap(), PRICE_PRECISION).unwrap()
    }

    fn quantity(value: &str) -> Quantity {
        Quantity::from_decimal_dp(Decimal::from_str(value).unwrap(), SIZE_PRECISION).unwrap()
    }

    #[rstest]
    fn test_parse_public_orderbook_frame() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(
                1_700_000_000_000,
                &json!([["3499.50", "1.20"], ["3499.00", "0.40"]]),
                &json!([["3501.00", "0.80"]]),
            ),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let deltas =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .unwrap();

        assert_eq!(msg.channel.as_str(), "orderbook.ETH-PERP.1.10");
        assert_eq!(
            msg.data.instrument_id(),
            InstrumentId::from("ETH-PERP.DERIVE")
        );
        assert_eq!(msg.data.bids[0].price().to_string(), "3499.50");
        assert_eq!(deltas.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, price("3499.50"));
        assert_eq!(deltas.deltas[1].order.size, quantity("1.20"));
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
        assert_eq!(
            deltas.deltas[3].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_public_trades_frame() {
        let payload = subscription_data_payload(
            "trades.perp.ETH",
            &json!([trade_json(1_700_000_000_001, "buy")]),
        );

        let msg = parse_trades_msg(&payload).unwrap();
        let tick = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .unwrap();

        assert_eq!(msg.channel.as_str(), "trades.perp.ETH");
        assert_eq!(msg.trades.len(), 1);
        assert_eq!(
            format_instrument_id(msg.trades[0].instrument_name.as_str()),
            InstrumentId::from("ETH-PERP.DERIVE")
        );
        assert_eq!(tick.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(tick.price, price("3500.2"));
        assert_eq!(tick.size, quantity("0.25"));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.trade_id, TradeId::from("trade-1"));
        assert_eq!(tick.ts_event, UnixNanos::from(1_700_000_000_001_000_000));
    }

    #[rstest]
    fn test_parse_public_ticker_frame() {
        let payload = subscription_data_payload(
            "ticker_slim.ETH-PERP.1000",
            &load_json("perps/ws_ticker_slim_eth.json"),
        );

        let msg = parse_ticker_msg(&payload).unwrap();
        let quote = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(789))
            .unwrap();

        assert_eq!(msg.channel.as_str(), "ticker_slim.ETH-PERP.1000");
        assert_eq!(
            msg.data.instrument_id(),
            InstrumentId::from("ETH-PERP.DERIVE")
        );
        assert_eq!(msg.data.timestamp(), 1_779_953_796_714);
        assert_eq!(quote.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(quote.bid_price, price("1992.36"));
        assert_eq!(quote.ask_price, price("1992.37"));
        assert_eq!(quote.bid_size, quantity("1.505"));
        assert_eq!(quote.ask_size, quantity("1.505"));
        assert_eq!(quote.ts_event, UnixNanos::from(1_779_953_796_714_000_000));
    }

    #[rstest]
    fn test_parse_spot_orderbook_frame() {
        let mut data = load_json("spot/ws_orderbook_eth.json");
        data["bids"] = json!([["2050.0", "1.20"], ["2049.5", "0.40"]]);
        data["asks"] = json!([["2051.0", "0.80"]]);
        let payload = subscription_data_payload("orderbook.ETH-USDC.1.10", &data);

        let msg = parse_orderbook_msg(&payload).unwrap();
        let deltas =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .unwrap();

        assert_eq!(msg.channel.as_str(), "orderbook.ETH-USDC.1.10");
        assert_eq!(
            msg.data.instrument_id(),
            InstrumentId::from("ETH-USDC.DERIVE")
        );
        assert_eq!(deltas.instrument_id, InstrumentId::from("ETH-USDC.DERIVE"));
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, price("2050.0"));
        assert_eq!(deltas.deltas[1].order.size, quantity("1.20"));
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
        assert_eq!(
            deltas.deltas[3].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_spot_trades_frame() {
        let payload = subscription_data_payload(
            "trades.erc20.ETH",
            &json!([load_json("spot/ws_trade_eth.json")]),
        );

        let msg = parse_trades_msg(&payload).unwrap();
        let tick = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .unwrap();

        assert_eq!(msg.channel.as_str(), "trades.erc20.ETH");
        assert_eq!(msg.trades.len(), 1);
        assert_eq!(tick.instrument_id, InstrumentId::from("ETH-USDC.DERIVE"));
        assert_eq!(tick.price, price("2050"));
        assert_eq!(tick.size, quantity("0.1"));
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(
            tick.trade_id,
            TradeId::from("0445f96a-10fb-4fdc-a0f9-eed94a2f32e1")
        );
    }

    #[rstest]
    fn test_parse_spot_ticker_slim_frame_handles_null_funding() {
        let payload = subscription_data_payload(
            "ticker_slim.ETH-USDC.1000",
            &load_json("spot/ws_ticker_slim_eth.json"),
        );

        let msg = parse_ticker_msg(&payload).unwrap();
        let quote = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(789))
            .unwrap();

        assert_eq!(msg.channel.as_str(), "ticker_slim.ETH-USDC.1000");
        assert_eq!(
            msg.data.instrument_id(),
            InstrumentId::from("ETH-USDC.DERIVE")
        );
        assert_eq!(quote.instrument_id, InstrumentId::from("ETH-USDC.DERIVE"));

        assert!(
            parse_funding_rate(&msg, UnixNanos::from(789))
                .unwrap()
                .is_none()
        );
        let mark = parse_mark_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .unwrap()
            .expect("spot slim ticker carries mark price");
        let index = parse_index_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .unwrap()
            .expect("spot slim ticker carries index price");
        assert_eq!(mark.instrument_id, InstrumentId::from("ETH-USDC.DERIVE"));
        assert_eq!(index.instrument_id, InstrumentId::from("ETH-USDC.DERIVE"));
    }

    #[rstest]
    fn test_parse_public_ticker_direct_payload() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &ticker_json_with_timestamp(1_700_000_000_011),
        );

        let msg = parse_ticker_msg(&payload).unwrap();
        let quote = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(790))
            .unwrap();

        assert_eq!(msg.channel.as_str(), "ticker.ETH-PERP.1000");
        assert_eq!(msg.data.timestamp(), 1_700_000_000_011);
        assert_eq!(
            msg.data.instrument_id(),
            InstrumentId::from("ETH-PERP.DERIVE")
        );
        assert_eq!(quote.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(quote.ts_event, UnixNanos::from(1_700_000_000_011_000_000));
    }

    #[rstest]
    fn test_parse_ticker_quote_uses_supplied_precision_when_wire_scale_varies() {
        let mut ticker = ticker_json_with_timestamp(1_700_000_000_012);
        ticker["best_bid_price"] = json!("3500");
        ticker["best_ask_price"] = json!("3501");
        ticker["best_bid_amount"] = json!("1");
        ticker["best_ask_amount"] = json!("2");
        let payload = subscription_data_payload("ticker.ETH-PERP.1000", &ticker);

        let msg = parse_ticker_msg(&payload).unwrap();
        let quote = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(790))
            .unwrap();

        assert_eq!(quote.bid_price, price("3500"));
        assert_eq!(quote.ask_price, price("3501"));
        assert_eq!(quote.bid_size, quantity("1"));
        assert_eq!(quote.ask_size, quantity("2"));
        assert_eq!(quote.bid_price.precision, PRICE_PRECISION);
        assert_eq!(quote.bid_size.precision, SIZE_PRECISION);
    }

    #[rstest]
    fn test_parse_ticker_quote_from_rest_emits_quote() {
        let ticker: DeriveTickerSnapshot =
            serde_json::from_value(ticker_json_with_timestamp(1_700_000_000_013)).unwrap();

        let quote = parse_ticker_quote_from_rest(
            &ticker,
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(791),
        )
        .unwrap();

        assert_eq!(quote.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(quote.bid_price, price("3499.50"));
        assert_eq!(quote.ask_price, price("3501.00"));
        assert_eq!(quote.bid_size, quantity("0.80"));
        assert_eq!(quote.ask_size, quantity("1.20"));
        assert_eq!(quote.ts_event, UnixNanos::from(1_700_000_000_013_000_000));
    }

    #[rstest]
    fn test_parse_ticker_quote_from_rest_rejects_negative_timestamp() {
        let mut value = ticker_json_with_timestamp(1_700_000_000_013);
        value["timestamp"] = json!(-1_i64);
        let ticker: DeriveTickerSnapshot = serde_json::from_value(value).unwrap();

        let err = parse_ticker_quote_from_rest(
            &ticker,
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(791),
        )
        .expect_err("must reject negative timestamp");
        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_orderbook_deltas_empty_book_marks_clear_last() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(1_700_000_000_000, &json!([]), &json!([])),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let deltas =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .unwrap();

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas.deltas[0].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_orderbook_deltas_skips_zero_size_levels() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(
                1_700_000_000_000,
                &json!([["3499.50", "0"], ["3499.00", "0.40"]]),
                &json!([["3501.00", "0"]]),
            ),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let deltas =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, price("3499.00"));
        assert_eq!(deltas.deltas[1].order.size, quantity("0.40"));
        assert_eq!(deltas.deltas[1].order.order_id, 1);
        assert_eq!(
            deltas.deltas[1].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_orderbook_deltas_uses_supplied_precision_when_wire_scale_varies() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(
                1_700_000_000_000,
                &json!([["3500", "1"]]),
                &json!([["3501", "2"]]),
            ),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let deltas =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .unwrap();

        assert_eq!(deltas.deltas[1].order.price, price("3500"));
        assert_eq!(deltas.deltas[1].order.size, quantity("1"));
        assert_eq!(deltas.deltas[2].order.price, price("3501"));
        assert_eq!(deltas.deltas[2].order.size, quantity("2"));
        assert_eq!(deltas.deltas[1].order.price.precision, PRICE_PRECISION);
        assert_eq!(deltas.deltas[1].order.size.precision, SIZE_PRECISION);
    }

    #[rstest]
    fn test_parse_trade_tick_maps_sell_direction() {
        let payload = subscription_data_payload(
            "trades.perp.ETH",
            &json!([trade_json(1_700_000_000_001, "sell")]),
        );

        let msg = parse_trades_msg(&payload).unwrap();
        let tick = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .unwrap();

        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
    }

    #[rstest]
    fn test_parse_trade_tick_uses_supplied_precision_when_wire_scale_varies() {
        let payload = subscription_data_payload(
            "trades.perp.ETH",
            &json!([trade_json_with_values(
                1_700_000_000_001,
                "buy",
                "3500",
                "1"
            )]),
        );

        let msg = parse_trades_msg(&payload).unwrap();
        let tick = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .unwrap();

        assert_eq!(tick.price, price("3500"));
        assert_eq!(tick.size, quantity("1"));
        assert_eq!(tick.price.precision, PRICE_PRECISION);
        assert_eq!(tick.size.precision, SIZE_PRECISION);
    }

    #[rstest]
    fn test_parse_public_ws_data_dispatches_orderbook_channel() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(1_700_000_000_000, &json!([]), &json!([])),
        );

        let parsed = parse_public_ws_data(&payload).unwrap();

        match parsed {
            DerivePublicWsData::Orderbook(msg) => {
                assert_eq!(msg.channel.as_str(), "orderbook.ETH-PERP.1.10");
                assert_eq!(
                    msg.data.instrument_id(),
                    InstrumentId::from("ETH-PERP.DERIVE")
                );
            }
            other => panic!("expected orderbook data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_public_ws_data_dispatches_trades_channel() {
        let payload = subscription_data_payload("trades.perp.ETH", &json!([]));

        let parsed = parse_public_ws_data(&payload).unwrap();

        match parsed {
            DerivePublicWsData::Trades(msg) => assert!(msg.trades.is_empty()),
            other => panic!("expected trades data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_public_ws_data_dispatches_ticker_channel() {
        let payload = subscription_data_payload(
            "ticker_slim.ETH-PERP.1000",
            &load_json("perps/ws_ticker_slim_eth.json"),
        );

        let parsed = parse_public_ws_data(&payload).unwrap();

        match parsed {
            DerivePublicWsData::Ticker(msg) => {
                assert_eq!(msg.channel.as_str(), "ticker_slim.ETH-PERP.1000");
                assert_eq!(
                    msg.data.instrument_id(),
                    InstrumentId::from("ETH-PERP.DERIVE")
                );
            }
            other => panic!("expected ticker data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_orderbook_msg_rejects_malformed_payload() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &json!({
                "instrument_name": "ETH-PERP",
                "timestamp": 1_700_000_000_000_i64,
                "bids": []
            }),
        );

        let err = parse_orderbook_msg(&payload).expect_err("must reject malformed orderbook");

        assert!(
            err.to_string()
                .contains("failed to decode Derive orderbook data")
        );
    }

    #[rstest]
    fn test_parse_trades_msg_rejects_malformed_payload() {
        let payload = subscription_data_payload("trades.perp.ETH", &json!({}));

        let err = parse_trades_msg(&payload).expect_err("must reject malformed trades");

        assert!(
            err.to_string()
                .contains("failed to decode Derive trades data")
        );
    }

    #[rstest]
    fn test_parse_ticker_msg_rejects_malformed_payload() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": 1_700_000_000_010_i64
            }),
        );

        let err = parse_ticker_msg(&payload).expect_err("must reject malformed ticker");

        assert!(
            err.to_string()
                .contains("failed to decode Derive ticker data")
        );
    }

    #[rstest]
    #[case("ticker_slim.ETH-PERP")]
    #[case("ticker_slim..1000")]
    fn test_parse_ticker_msg_rejects_malformed_slim_channel(#[case] channel: &str) {
        let payload =
            subscription_data_payload(channel, &load_json("perps/ws_ticker_slim_eth.json"));

        let err = parse_ticker_msg(&payload).expect_err("must reject malformed slim channel");

        assert!(err.to_string().contains("invalid Derive ticker channel"));
    }

    #[rstest]
    fn test_parse_orderbook_deltas_rejects_negative_timestamp() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(-1, &json!([]), &json!([])),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let err =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .expect_err("must reject negative orderbook timestamp");

        assert!(
            err.to_string()
                .contains("negative Derive orderbook timestamp")
        );
    }

    #[rstest]
    fn test_parse_orderbook_deltas_rejects_timestamp_overflow() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(i64::MAX, &json!([]), &json!([])),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let err =
            parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(123))
                .expect_err("must reject overflowing orderbook timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_orderbook_deltas_rejects_invalid_size_precision() {
        let payload = subscription_data_payload(
            "orderbook.ETH-PERP.1.10",
            &orderbook_json(
                1_700_000_000_000,
                &json!([["3500", "1"]]),
                &json!([["3501", "2"]]),
            ),
        );

        let msg = parse_orderbook_msg(&payload).unwrap();
        let err = parse_orderbook_deltas(
            &msg,
            PRICE_PRECISION,
            INVALID_PRECISION,
            UnixNanos::from(123),
        )
        .expect_err("must reject invalid orderbook size precision");

        assert!(err.to_string().contains("invalid Derive orderbook amount"));
    }

    #[rstest]
    fn test_parse_trade_tick_rejects_negative_timestamp() {
        let payload = subscription_data_payload("trades.perp.ETH", &json!([trade_json(-1, "buy")]));

        let msg = parse_trades_msg(&payload).unwrap();
        let err = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .expect_err("must reject negative trade timestamp");

        assert!(err.to_string().contains("negative Derive trade timestamp"));
    }

    #[rstest]
    fn test_parse_trade_tick_rejects_timestamp_overflow() {
        let payload =
            subscription_data_payload("trades.perp.ETH", &json!([trade_json(i64::MAX, "buy")]));

        let msg = parse_trades_msg(&payload).unwrap();
        let err = parse_trade_tick(
            &msg.trades[0],
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .expect_err("must reject overflowing trade timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_trade_tick_rejects_invalid_price_precision() {
        let payload = subscription_data_payload(
            "trades.perp.ETH",
            &json!([trade_json(1_700_000_000_001, "buy")]),
        );

        let msg = parse_trades_msg(&payload).unwrap();
        let err = parse_trade_tick(
            &msg.trades[0],
            INVALID_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(456),
        )
        .expect_err("must reject invalid trade price precision");

        assert!(err.to_string().contains("invalid trade price for ETH-PERP"));
    }

    #[rstest]
    fn test_parse_ticker_quote_rejects_negative_timestamp() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": -1_i64,
                "instrument_ticker": ticker_json()
            }),
        );

        let msg = parse_ticker_msg(&payload).unwrap();
        let err = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject negative ticker timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_ticker_quote_rejects_timestamp_overflow() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": i64::MAX,
                "instrument_ticker": ticker_json()
            }),
        );

        let msg = parse_ticker_msg(&payload).unwrap();
        let err = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject overflowing ticker timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_public_ws_data_rejects_unknown_channel() {
        let payload = WsSubscriptionPayload {
            channel: Ustr::from("wallet.ETH"),
            data: json!({}),
        };

        let err = parse_public_ws_data(&payload).expect_err("must reject unknown channel");

        assert!(
            err.to_string()
                .contains("unsupported Derive public WS channel")
        );
    }

    fn option_ticker_json(timestamp: i64) -> Value {
        let mut value = load_json("options/http_ticker_eth_snapshot.json");
        value["timestamp"] = json!(timestamp);
        value
    }

    fn perp_envelope_payload(timestamp: i64) -> WsSubscriptionPayload {
        subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": timestamp,
                "instrument_ticker": ticker_json_with_timestamp(timestamp),
            }),
        )
    }

    fn option_envelope_payload(timestamp: i64) -> WsSubscriptionPayload {
        let mut option_data = option_ticker_json(timestamp);
        option_data["instrument_name"] = json!("ETH-20260627-3500-C");
        subscription_data_payload(
            "ticker.ETH-20260627-3500-C.1000",
            &json!({
                "timestamp": timestamp,
                "instrument_ticker": option_data,
            }),
        )
    }

    fn slim_payload() -> WsSubscriptionPayload {
        subscription_data_payload(
            "ticker_slim.ETH-PERP.1000",
            &load_json("perps/ws_ticker_slim_eth.json"),
        )
    }

    #[rstest]
    fn test_parse_mark_price_maps_slim_variant() {
        let msg = parse_ticker_msg(&slim_payload()).unwrap();

        let update = parse_mark_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .unwrap()
            .expect("slim ticker carries mark price");

        assert_eq!(update.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(update.value, price("1992.49"));
        assert_eq!(update.ts_event, UnixNanos::from(1_779_953_796_714_000_000));
        assert_eq!(update.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_index_price_maps_slim_variant() {
        let msg = parse_ticker_msg(&slim_payload()).unwrap();

        let update = parse_index_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .unwrap()
            .expect("slim ticker carries index price");

        assert_eq!(update.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(update.value, price("1991.79"));
        assert_eq!(update.ts_event, UnixNanos::from(1_779_953_796_714_000_000));
        assert_eq!(update.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_funding_rate_maps_slim_variant() {
        let msg = parse_ticker_msg(&slim_payload()).unwrap();

        let update = parse_funding_rate(&msg, UnixNanos::from(789))
            .unwrap()
            .expect("slim ticker carries perp funding");

        assert_eq!(update.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(update.rate, Decimal::from_str("0.000012500").unwrap());
        assert_eq!(update.ts_event, UnixNanos::from(1_779_953_796_714_000_000));
        assert_eq!(update.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_option_greeks_returns_none_for_slim_variant_without_option_pricing() {
        let msg = parse_ticker_msg(&slim_payload()).unwrap();

        let result = parse_option_greeks(&msg, UnixNanos::from(789)).unwrap();

        assert!(result.is_none());
    }

    fn option_slim_payload(filename: &str, instrument_name: &str) -> WsSubscriptionPayload {
        subscription_data_payload(
            &format!("ticker_slim.{instrument_name}.1000"),
            &load_json(filename),
        )
    }

    #[rstest]
    fn test_parse_option_greeks_maps_slim_variant() {
        let msg = parse_ticker_msg(&option_slim_payload(
            "options/ws_ticker_slim_eth_call.json",
            "ETH-20260612-1600-C",
        ))
        .unwrap();

        let greeks = parse_option_greeks(&msg, UnixNanos::from(789))
            .unwrap()
            .expect("slim ticker carries option pricing");

        assert_eq!(
            greeks.instrument_id,
            InstrumentId::from("ETH-20260612-1600-C.DERIVE")
        );
        assert_eq!(greeks.convention, GreeksConvention::BlackScholes);
        assert!((greeks.greeks.delta - 0.95222).abs() < 1e-9);
        assert!((greeks.greeks.gamma - 0.00036344).abs() < 1e-9);
        assert_eq!(greeks.mark_iv, Some(0.67698));
        assert_eq!(greeks.bid_iv, Some(0.0));
        assert_eq!(greeks.ask_iv, Some(0.88815));
        assert_eq!(greeks.underlying_price, Some(1992.6));
        assert_eq!(greeks.open_interest, Some(0.0));
        assert_eq!(greeks.ts_event, UnixNanos::from(1_779_953_796_231_000_000));
        assert_eq!(greeks.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_option_greeks_maps_slim_put_variant() {
        let msg = parse_ticker_msg(&option_slim_payload(
            "options/ws_ticker_slim_eth_put.json",
            "ETH-20260612-1900-P",
        ))
        .unwrap();

        let greeks = parse_option_greeks(&msg, UnixNanos::from(789))
            .unwrap()
            .expect("slim ticker carries put option pricing");

        assert_eq!(
            greeks.instrument_id,
            InstrumentId::from("ETH-20260612-1900-P.DERIVE")
        );
        assert!((greeks.greeks.delta + 0.30438).abs() < 1e-9);
        assert!((greeks.greeks.gamma - 0.00169741).abs() < 1e-9);
        assert_eq!(greeks.mark_iv, Some(0.51012));
        assert_eq!(greeks.bid_iv, Some(0.48229));
        assert_eq!(greeks.ask_iv, Some(0.52063));
        assert_eq!(greeks.underlying_price, Some(1992.6));
        assert_eq!(greeks.open_interest, Some(42.13));
        assert_eq!(greeks.ts_event, UnixNanos::from(1_779_953_797_040_000_000));
        assert_eq!(greeks.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_funding_rate_returns_none_for_option_payload() {
        let msg = parse_ticker_msg(&option_envelope_payload(1_700_000_000_010)).unwrap();

        let result = parse_funding_rate(&msg, UnixNanos::from(789)).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_option_greeks_returns_none_for_perp_payload() {
        let msg = parse_ticker_msg(&perp_envelope_payload(1_700_000_000_010)).unwrap();

        let result = parse_option_greeks(&msg, UnixNanos::from(789)).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_option_greeks_open_interest_none_when_stats_absent() {
        // Legacy full ticker payloads may omit `stats`. When the WS path
        // receives one without stats, `open_interest` must degrade to
        // None while the remaining greek fields still populate normally.
        let timestamp = 1_700_000_000_010_i64;
        let mut option_data = option_ticker_json(timestamp);
        option_data["instrument_name"] = json!("ETH-20260627-3500-C");
        option_data["stats"] = json!(null);
        let payload = subscription_data_payload(
            "ticker.ETH-20260627-3500-C.1000",
            &json!({
                "timestamp": timestamp,
                "instrument_ticker": option_data,
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let greeks = parse_option_greeks(&msg, UnixNanos::from(789))
            .unwrap()
            .expect("option greeks present when option_pricing is set");
        assert!(greeks.open_interest.is_none());
        // The other greek fields must still be populated from option_pricing
        assert!((greeks.greeks.delta - 0.55).abs() < 1e-9);
        assert!(greeks.mark_iv.is_some());
        assert!(greeks.underlying_price.is_some());
    }

    #[rstest]
    fn test_parse_mark_price_rejects_negative_timestamp() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": -1_i64,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_mark_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject negative ticker timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_mark_price_rejects_timestamp_overflow() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": i64::MAX,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_mark_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject overflowing ticker timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_index_price_rejects_negative_timestamp() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": -1_i64,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_index_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject negative ticker timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_index_price_rejects_timestamp_overflow() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": i64::MAX,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_index_price(&msg, PRICE_PRECISION, UnixNanos::from(789))
            .expect_err("must reject overflowing ticker timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_funding_rate_rejects_negative_timestamp() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": -1_i64,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_funding_rate(&msg, UnixNanos::from(789))
            .expect_err("must reject negative ticker timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_funding_rate_rejects_timestamp_overflow() {
        let payload = subscription_data_payload(
            "ticker.ETH-PERP.1000",
            &json!({
                "timestamp": i64::MAX,
                "instrument_ticker": ticker_json(),
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_funding_rate(&msg, UnixNanos::from(789))
            .expect_err("must reject overflowing ticker timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }

    #[rstest]
    fn test_parse_funding_rate_history_record_maps_fields() {
        let record = DerivePublicFundingRate {
            funding_rate: Decimal::from_str("0.00015").unwrap(),
            timestamp: 1_700_000_000_000,
        };
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

        let update = parse_funding_rate_history_record(
            &record,
            instrument_id,
            Some(60),
            UnixNanos::from(789),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument_id);
        assert_eq!(update.rate, Decimal::from_str("0.00015").unwrap());
        assert_eq!(update.interval, Some(60));
        assert!(update.next_funding_ns.is_none());
        assert_eq!(update.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(update.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_funding_rate_history_record_rejects_negative_timestamp() {
        let record = DerivePublicFundingRate {
            funding_rate: Decimal::from_str("0.0001").unwrap(),
            timestamp: -1,
        };
        let err = parse_funding_rate_history_record(
            &record,
            InstrumentId::from("ETH-PERP.DERIVE"),
            None,
            UnixNanos::from(789),
        )
        .expect_err("must reject negative timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_candle_record_maps_fields() {
        // `timestamp` and `timestamp_bucket` differ so a swap from `timestamp_bucket`
        // to `timestamp` in the parser would shift ts_event and fail the assertion.
        let record = DerivePublicCandle {
            open_price: Decimal::from_str("3500.0").unwrap(),
            high_price: Decimal::from_str("3501.5").unwrap(),
            low_price: Decimal::from_str("3499.0").unwrap(),
            close_price: Decimal::from_str("3501.0").unwrap(),
            volume_usd: Decimal::from_str("12345.6").unwrap(),
            volume_contracts: Decimal::from_str("3.527").unwrap(),
            timestamp: 1_700_000_007,
            timestamp_bucket: 1_700_000_000,
        };
        let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

        let bar = parse_candle_record(
            &record,
            bar_type,
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(789),
        )
        .unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from_str("3500.00").unwrap());
        assert_eq!(bar.high, Price::from_str("3501.50").unwrap());
        assert_eq!(bar.low, Price::from_str("3499.00").unwrap());
        assert_eq!(bar.close, Price::from_str("3501.00").unwrap());
        assert_eq!(bar.volume, Quantity::from_str("3.527").unwrap());
        assert_eq!(bar.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(bar.ts_init, UnixNanos::from(789));
    }

    #[rstest]
    fn test_parse_candle_record_rejects_negative_timestamp() {
        let record = DerivePublicCandle {
            open_price: Decimal::from_str("1").unwrap(),
            high_price: Decimal::from_str("1").unwrap(),
            low_price: Decimal::from_str("1").unwrap(),
            close_price: Decimal::from_str("1").unwrap(),
            volume_usd: Decimal::ZERO,
            volume_contracts: Decimal::ZERO,
            timestamp: 1_700_000_000,
            timestamp_bucket: -1,
        };
        let err = parse_candle_record(
            &record,
            BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL"),
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(789),
        )
        .expect_err("must reject negative timestamp");

        assert!(err.to_string().contains("negative Derive candle timestamp"));
    }

    #[rstest]
    fn test_parse_candle_record_rejects_timestamp_overflow() {
        let record = DerivePublicCandle {
            open_price: Decimal::from_str("1").unwrap(),
            high_price: Decimal::from_str("1").unwrap(),
            low_price: Decimal::from_str("1").unwrap(),
            close_price: Decimal::from_str("1").unwrap(),
            volume_usd: Decimal::ZERO,
            volume_contracts: Decimal::ZERO,
            timestamp: 1_700_000_000,
            timestamp_bucket: i64::MAX,
        };
        let err = parse_candle_record(
            &record,
            BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL"),
            PRICE_PRECISION,
            SIZE_PRECISION,
            UnixNanos::from(789),
        )
        .expect_err("must reject overflowing timestamp");

        assert!(
            err.to_string()
                .contains("Derive candle timestamp_bucket overflows nanoseconds"),
            "{err}",
        );
    }

    #[rstest]
    #[case(BarAggregation::Minute, 1, 60)]
    #[case(BarAggregation::Minute, 5, 300)]
    #[case(BarAggregation::Minute, 15, 900)]
    #[case(BarAggregation::Minute, 30, 1800)]
    #[case(BarAggregation::Hour, 1, 3600)]
    #[case(BarAggregation::Hour, 4, 14400)]
    #[case(BarAggregation::Hour, 8, 28800)]
    #[case(BarAggregation::Day, 1, 86400)]
    #[case(BarAggregation::Week, 1, 604800)]
    fn test_bar_spec_to_derive_period_maps_supported_intervals(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
        #[case] expected: u32,
    ) {
        assert_eq!(
            bar_spec_to_derive_period(aggregation, step).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case(BarAggregation::Minute, 2, "minute intervals")]
    #[case(BarAggregation::Hour, 2, "hour intervals")]
    #[case(BarAggregation::Day, 7, "1 DAY interval")]
    #[case(BarAggregation::Week, 2, "1 WEEK interval")]
    #[case(BarAggregation::Second, 1, "does not support")]
    fn test_bar_spec_to_derive_period_rejects_unsupported(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
        #[case] expected_msg: &str,
    ) {
        let err =
            bar_spec_to_derive_period(aggregation, step).expect_err("must reject unsupported spec");
        assert!(
            err.to_string().contains(expected_msg),
            "expected {expected_msg:?}, was {err}",
        );
    }

    #[rstest]
    fn test_parse_option_greeks_rejects_negative_timestamp() {
        let mut option_data = option_ticker_json(1_700_000_000_000);
        option_data["instrument_name"] = json!("ETH-20260627-3500-C");
        let payload = subscription_data_payload(
            "ticker.ETH-20260627-3500-C.1000",
            &json!({
                "timestamp": -1_i64,
                "instrument_ticker": option_data,
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_option_greeks(&msg, UnixNanos::from(789))
            .expect_err("must reject negative ticker timestamp");

        assert!(err.to_string().contains("negative Derive ticker timestamp"));
    }

    #[rstest]
    fn test_parse_option_greeks_rejects_timestamp_overflow() {
        let mut option_data = option_ticker_json(1_700_000_000_000);
        option_data["instrument_name"] = json!("ETH-20260627-3500-C");
        let payload = subscription_data_payload(
            "ticker.ETH-20260627-3500-C.1000",
            &json!({
                "timestamp": i64::MAX,
                "instrument_ticker": option_data,
            }),
        );
        let msg = parse_ticker_msg(&payload).unwrap();

        let err = parse_option_greeks(&msg, UnixNanos::from(789))
            .expect_err("must reject overflowing ticker timestamp");

        assert!(
            err.to_string()
                .contains("Derive timestamp overflows nanoseconds")
        );
    }
}

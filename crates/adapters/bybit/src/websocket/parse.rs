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

//! Parsing helpers for Bybit WebSocket payloads.

use std::convert::TryFrom;

use anyhow::Context;
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{
        AccountType, AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus, OrderType,
        PositionSideSpecified, RecordFlag, TimeInForce,
    },
    events::account::state::AccountState,
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::messages::{
    BybitWsAccountExecution, BybitWsAccountOrder, BybitWsAccountPosition, BybitWsAccountWallet,
    BybitWsKline, BybitWsOrderbookDepthMsg, BybitWsTickerLinearMsg, BybitWsTickerOptionMsg,
    BybitWsTrade,
};
use crate::common::{
    enums::{BybitOrderStatus, BybitOrderType, BybitTimeInForce},
    parse::{parse_millis_timestamp, parse_price_with_precision, parse_quantity_with_precision},
};

/// Parses a Bybit WebSocket topic string into its components.
///
/// # Errors
///
/// Returns an error if the topic format is invalid.
pub fn parse_topic(topic: &str) -> anyhow::Result<Vec<&str>> {
    let parts: Vec<&str> = topic.split('.').collect();
    if parts.is_empty() {
        anyhow::bail!("Invalid topic format: empty topic");
    }
    Ok(parts)
}

/// Parses a Bybit kline topic into (interval, symbol).
///
/// Topic format: "kline.{interval}.{symbol}" (e.g., "kline.5.BTCUSDT")
///
/// # Errors
///
/// Returns an error if the topic format is invalid.
pub fn parse_kline_topic(topic: &str) -> anyhow::Result<(&str, &str)> {
    let parts = parse_topic(topic)?;
    if parts.len() != 3 || parts[0] != "kline" {
        anyhow::bail!(
            "Invalid kline topic format: expected 'kline.{{interval}}.{{symbol}}', got '{topic}'"
        );
    }
    Ok((parts[1], parts[2]))
}

/// Parses a WebSocket trade frame into a [`TradeTick`].
pub fn parse_ws_trade_tick(
    trade: &BybitWsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price_with_precision(&trade.p, instrument.price_precision(), "trade.p")?;
    let size = parse_quantity_with_precision(&trade.v, instrument.size_precision(), "trade.v")?;
    let aggressor: AggressorSide = trade.taker_side.into();
    let trade_id = TradeId::new_checked(trade.i.as_str())
        .context("invalid trade identifier in Bybit trade message")?;
    let ts_event = parse_millis_i64(trade.t, "trade.T")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Bybit trade message")
}

/// Parses an order book depth message into [`OrderBookDeltas`].
pub fn parse_orderbook_deltas(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let is_snapshot = msg.msg_type.eq_ignore_ascii_case("snapshot");
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    let depth = &msg.data;
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let update_id = u64::try_from(depth.u)
        .context("received negative update id in Bybit order book message")?;
    let sequence = u64::try_from(depth.seq)
        .context("received negative sequence in Bybit order book message")?;

    let mut deltas = Vec::new();

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let total_levels = depth.b.len() + depth.a.len();
    let mut processed = 0_usize;

    let mut push_level = |values: &[String], side: OrderSide| -> anyhow::Result<()> {
        let (price, size) = parse_book_level(values, price_precision, size_precision, "orderbook")?;
        let action = if size.is_zero() {
            BookAction::Delete
        } else if is_snapshot {
            BookAction::Add
        } else {
            BookAction::Update
        };

        processed += 1;
        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, update_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct OrderBookDelta from Bybit book level")?;
        deltas.push(delta);
        Ok(())
    };

    for level in &depth.b {
        push_level(level, OrderSide::Buy)?;
    }
    for level in &depth.a {
        push_level(level, OrderSide::Sell)?;
    }

    if total_levels == 0
        && let Some(last) = deltas.last_mut()
    {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("failed to assemble OrderBookDeltas from Bybit message")
}

/// Parses an order book snapshot or delta into a [`QuoteTick`].
pub fn parse_orderbook_quote(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    last_quote: Option<&QuoteTick>,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let get_best =
        |levels: &[Vec<String>], label: &str| -> anyhow::Result<Option<(Price, Quantity)>> {
            if let Some(values) = levels.first() {
                parse_book_level(values, price_precision, size_precision, label).map(Some)
            } else {
                Ok(None)
            }
        };

    let bids = get_best(&msg.data.b, "bid")?;
    let asks = get_best(&msg.data.a, "ask")?;

    let (bid_price, bid_size) = match (bids, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.bid_price, prev.bid_size),
        (None, None) => {
            anyhow::bail!(
                "Bybit order book update missing bid levels and no previous quote provided"
            );
        }
    };

    let (ask_price, ask_size) = match (asks, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.ask_price, prev.ask_size),
        (None, None) => {
            anyhow::bail!(
                "Bybit order book update missing ask levels and no previous quote provided"
            );
        }
    };

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit order book message")
}

/// Parses a linear or inverse ticker payload into a [`QuoteTick`].
pub fn parse_ticker_linear_quote(
    msg: &BybitWsTickerLinearMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "ticker.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let data = &msg.data;
    let bid_price = data
        .bid1_price
        .as_ref()
        .context("Bybit ticker message missing bid1Price")?
        .as_str();
    let ask_price = data
        .ask1_price
        .as_ref()
        .context("Bybit ticker message missing ask1Price")?
        .as_str();

    let bid_price = parse_price_with_precision(bid_price, price_precision, "ticker.bid1Price")?;
    let ask_price = parse_price_with_precision(ask_price, price_precision, "ticker.ask1Price")?;

    let bid_size_str = data.bid1_size.as_deref().unwrap_or("0");
    let ask_size_str = data.ask1_size.as_deref().unwrap_or("0");

    let bid_size = parse_quantity_with_precision(bid_size_str, size_precision, "ticker.bid1Size")?;
    let ask_size = parse_quantity_with_precision(ask_size_str, size_precision, "ticker.ask1Size")?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit linear ticker message")
}

/// Parses an option ticker payload into a [`QuoteTick`].
pub fn parse_ticker_option_quote(
    msg: &BybitWsTickerOptionMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "ticker.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let data = &msg.data;
    let bid_price =
        parse_price_with_precision(&data.bid_price, price_precision, "ticker.bidPrice")?;
    let ask_price =
        parse_price_with_precision(&data.ask_price, price_precision, "ticker.askPrice")?;
    let bid_size = parse_quantity_with_precision(&data.bid_size, size_precision, "ticker.bidSize")?;
    let ask_size = parse_quantity_with_precision(&data.ask_size, size_precision, "ticker.askSize")?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit option ticker message")
}

pub(crate) fn parse_millis_i64(value: i64, field: &str) -> anyhow::Result<UnixNanos> {
    if value < 0 {
        Err(anyhow::anyhow!("{field} must be non-negative, was {value}"))
    } else {
        parse_millis_timestamp(&value.to_string(), field)
    }
}

fn parse_book_level(
    level: &[String],
    price_precision: u8,
    size_precision: u8,
    label: &str,
) -> anyhow::Result<(Price, Quantity)> {
    let price_str = level
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing price component in {label} level"))?;
    let size_str = level
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("missing size component in {label} level"))?;
    let price = parse_price_with_precision(price_str, price_precision, label)?;
    let size = parse_quantity_with_precision(size_str, size_precision, label)?;
    Ok((price, size))
}

/// Parses a WebSocket kline payload into a [`Bar`].
///
/// # Errors
///
/// Returns an error if price or volume fields cannot be parsed or if the bar cannot be constructed.
pub fn parse_ws_kline_bar(
    kline: &BybitWsKline,
    instrument: &InstrumentAny,
    bar_type: BarType,
    timestamp_on_close: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&kline.open, price_precision, "kline.open")?;
    let high = parse_price_with_precision(&kline.high, price_precision, "kline.high")?;
    let low = parse_price_with_precision(&kline.low, price_precision, "kline.low")?;
    let close = parse_price_with_precision(&kline.close, price_precision, "kline.close")?;
    let volume = parse_quantity_with_precision(&kline.volume, size_precision, "kline.volume")?;

    let mut ts_event = parse_millis_i64(kline.start, "kline.start")?;
    if timestamp_on_close {
        let interval_ns = bar_type
            .spec()
            .timedelta()
            .num_nanoseconds()
            .context("bar specification produced non-integer interval")?;
        let interval_ns = u64::try_from(interval_ns)
            .context("bar interval overflowed the u64 range for nanoseconds")?;
        let updated = ts_event
            .as_u64()
            .checked_add(interval_ns)
            .context("bar timestamp overflowed when adjusting to close time")?;
        ts_event = UnixNanos::from(updated);
    }
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Bybit WebSocket kline")
}

/// Parses a WebSocket account order payload into an [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if price or quantity fields cannot be parsed or timestamps are invalid.
pub fn parse_ws_order_status_report(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());
    let order_side: OrderSide = order.side.into();

    let order_type: OrderType = match order.order_type {
        BybitOrderType::Market => OrderType::Market,
        BybitOrderType::Limit => OrderType::Limit,
        BybitOrderType::Unknown => OrderType::Limit,
    };

    let time_in_force: TimeInForce = match order.time_in_force {
        BybitTimeInForce::Gtc => TimeInForce::Gtc,
        BybitTimeInForce::Ioc => TimeInForce::Ioc,
        BybitTimeInForce::Fok => TimeInForce::Fok,
        BybitTimeInForce::PostOnly => TimeInForce::Gtc,
    };

    let quantity =
        parse_quantity_with_precision(&order.qty, instrument.size_precision(), "order.qty")?;

    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )?;

    // Map Bybit order status to Nautilus order status
    // Special case: if Bybit reports "Rejected" but the order has fills, treat it as Canceled.
    // This handles the case where the exchange partially fills an order then rejects the
    // remaining quantity (e.g., due to margin, risk limits, or liquidity constraints).
    // The state machine does not allow PARTIALLY_FILLED -> REJECTED transitions.
    let order_status: OrderStatus = match order.order_status {
        BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
            OrderStatus::Accepted
        }
        BybitOrderStatus::Rejected => {
            if filled_qty.is_positive() {
                OrderStatus::Canceled
            } else {
                OrderStatus::Rejected
            }
        }
        BybitOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BybitOrderStatus::Filled => OrderStatus::Filled,
        BybitOrderStatus::Canceled | BybitOrderStatus::PartiallyFilledCanceled => {
            OrderStatus::Canceled
        }
        BybitOrderStatus::Triggered => OrderStatus::Triggered,
        BybitOrderStatus::Deactivated => OrderStatus::Canceled,
    };

    let ts_accepted = parse_millis_timestamp(&order.created_time, "order.createdTime")?;
    let ts_last = parse_millis_timestamp(&order.updated_time, "order.updatedTime")?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(UUID4::new()),
    );

    if !order.order_link_id.is_empty() {
        report = report.with_client_order_id(ClientOrderId::new(order.order_link_id.as_str()));
    }

    if !order.price.is_empty() && order.price != "0" {
        let price =
            parse_price_with_precision(&order.price, instrument.price_precision(), "order.price")?;
        report = report.with_price(price);
    }

    if !order.avg_price.is_empty() && order.avg_price != "0" {
        let avg_px = order
            .avg_price
            .parse::<f64>()
            .with_context(|| format!("Failed to parse avg_price='{}' as f64", order.avg_price))?;
        report = report.with_avg_px(avg_px);
    }

    if !order.trigger_price.is_empty() && order.trigger_price != "0" {
        let trigger_price = parse_price_with_precision(
            &order.trigger_price,
            instrument.price_precision(),
            "order.triggerPrice",
        )?;
        report = report.with_trigger_price(trigger_price);
    }

    if order.reduce_only {
        report = report.with_reduce_only(true);
    }

    if order.time_in_force == BybitTimeInForce::PostOnly {
        report = report.with_post_only(true);
    }

    if !order.reject_reason.is_empty() {
        report = report.with_cancel_reason(order.reject_reason.to_string());
    }

    Ok(report)
}

/// Parses a WebSocket account execution payload into a [`FillReport`].
///
/// # Errors
///
/// Returns an error if price or quantity fields cannot be parsed or timestamps are invalid.
pub fn parse_ws_fill_report(
    execution: &BybitWsAccountExecution,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(execution.order_id.as_str());
    let trade_id = TradeId::new_checked(execution.exec_id.as_str())
        .context("invalid execId in Bybit WebSocket execution payload")?;

    let order_side: OrderSide = execution.side.into();
    let last_qty = parse_quantity_with_precision(
        &execution.exec_qty,
        instrument.size_precision(),
        "execution.execQty",
    )?;
    let last_px = parse_price_with_precision(
        &execution.exec_price,
        instrument.price_precision(),
        "execution.execPrice",
    )?;

    let liquidity_side = if execution.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let commission_str = execution.exec_fee.trim_start_matches('-');
    let commission_amount = commission_str
        .parse::<f64>()
        .with_context(|| format!("Failed to parse execFee='{}' as f64", execution.exec_fee))?
        .abs();

    // Use instrument quote currency for commission
    let commission_currency = instrument.quote_currency();
    let commission = Money::new(commission_amount, commission_currency);
    let ts_event = parse_millis_timestamp(&execution.exec_time, "execution.execTime")?;

    let client_order_id = if !execution.order_link_id.is_empty() {
        Some(ClientOrderId::new(execution.order_link_id.as_str()))
    } else {
        None
    };

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None, // venue_position_id
        ts_event,
        ts_init,
        None, // report_id
    ))
}

/// Parses a WebSocket account position payload into a [`PositionStatusReport`].
///
/// # Errors
///
/// Returns an error if position size or prices cannot be parsed.
pub fn parse_ws_position_status_report(
    position: &BybitWsAccountPosition,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Parse absolute size as unsigned Quantity
    let quantity = parse_quantity_with_precision(
        &position.size,
        instrument.size_precision(),
        "position.size",
    )?;

    // Derive position side from the side field
    let position_side = if position.side.eq_ignore_ascii_case("buy") {
        PositionSideSpecified::Long
    } else if position.side.eq_ignore_ascii_case("sell") {
        PositionSideSpecified::Short
    } else {
        PositionSideSpecified::Flat
    };

    let avg_px_open = if let Some(ref avg_price) = position.avg_price {
        if !avg_price.is_empty() && avg_price != "0" {
            avg_price
                .parse::<f64>()
                .with_context(|| format!("Failed to parse avgPrice='{}' as f64", avg_price))?
        } else {
            0.0
        }
    } else {
        0.0
    };

    let _unrealized_pnl = position.unrealised_pnl.parse::<f64>().with_context(|| {
        format!(
            "Failed to parse unrealisedPnl='{}' as f64",
            position.unrealised_pnl
        )
    })?;

    let _realized_pnl = position.cum_realised_pnl.parse::<f64>().with_context(|| {
        format!(
            "Failed to parse cumRealisedPnl='{}' as f64",
            position.cum_realised_pnl
        )
    })?;

    let ts_last = parse_millis_timestamp(&position.updated_time, "position.updatedTime")?;

    let avg_px_open_decimal = if avg_px_open != 0.0 {
        Some(Decimal::try_from(avg_px_open).context("Failed to convert avg_px_open to Decimal")?)
    } else {
        None
    };

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None, // report_id
        None, // venue_position_id
        avg_px_open_decimal,
    ))
}

/// Parses a WebSocket account wallet payload into an [`AccountState`].
///
/// # Errors
///
/// Returns an error if balance fields cannot be parsed.
pub fn parse_ws_account_state(
    wallet: &BybitWsAccountWallet,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();

    for coin_data in &wallet.coin {
        let currency = Currency::from(coin_data.coin.as_str());

        let wallet_balance_amount = coin_data.wallet_balance.parse::<f64>().with_context(|| {
            format!(
                "Failed to parse walletBalance='{}' as f64",
                coin_data.wallet_balance
            )
        })?;

        let spot_borrow_amount = if let Some(ref spot_borrow) = coin_data.spot_borrow {
            if spot_borrow.is_empty() {
                0.0
            } else {
                spot_borrow.parse::<f64>().with_context(|| {
                    format!("Failed to parse spotBorrow='{}' as f64", spot_borrow)
                })?
            }
        } else {
            0.0
        };

        let total_amount = wallet_balance_amount - spot_borrow_amount;

        let free_amount = if coin_data.available_to_withdraw.is_empty() {
            0.0
        } else {
            coin_data
                .available_to_withdraw
                .parse::<f64>()
                .with_context(|| {
                    format!(
                        "Failed to parse availableToWithdraw='{}' as f64",
                        coin_data.available_to_withdraw
                    )
                })?
        };

        let locked_amount = total_amount - free_amount;

        let total = Money::new(total_amount, currency);
        let locked = Money::new(locked_amount, currency);
        let free = Money::new(free_amount, currency);

        let balance = AccountBalance::new_checked(total, locked, free)
            .context("Failed to create AccountBalance from wallet data")?;
        balances.push(balance);
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Margin, // Bybit unified account
        balances,
        vec![], // margins - Bybit doesn't provide per-instrument margin in wallet updates
        true,   // is_reported
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PositionSide, PriceType},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{
            parse::{parse_linear_instrument, parse_option_instrument},
            testing::load_test_json,
        },
        http::models::{BybitInstrumentLinearResponse, BybitInstrumentOptionResponse},
        websocket::messages::{
            BybitWsOrderbookDepthMsg, BybitWsTickerLinearMsg, BybitWsTickerOptionMsg,
            BybitWsTradeMsg,
        },
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    use ustr::Ustr;

    use crate::http::models::BybitFeeRate;

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap()
    }

    fn option_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        parse_option_instrument(instrument, TS, TS).unwrap()
    }

    #[rstest]
    fn parse_ws_trade_into_trade_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let trade = &msg.data[0];

        let tick = parse_ws_trade_tick(trade, &instrument, TS).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(27451.00));
        assert_eq!(tick.size, instrument.make_qty(0.010, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(
            tick.trade_id.to_string(),
            "9dc75fca-4bdd-4773-9f78-6f5d7ab2a110"
        );
        assert_eq!(tick.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_orderbook_snapshot_into_deltas() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas.deltas[1].order.price,
            instrument.make_price(27450.00)
        );
        assert_eq!(
            deltas.deltas[1].order.size,
            instrument.make_qty(0.500, None)
        );
        let last = deltas.deltas.last().unwrap();
        assert_eq!(last.order.side, OrderSide::Sell);
        assert_eq!(last.order.price, instrument.make_price(27451.50));
        assert_eq!(
            last.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_delta_marks_actions() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_delta.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        let bid = &deltas.deltas[0];
        assert_eq!(bid.action, BookAction::Update);
        assert_eq!(bid.order.side, OrderSide::Buy);
        assert_eq!(bid.order.size, instrument.make_qty(0.400, None));

        let ask = &deltas.deltas[1];
        assert_eq!(ask.action, BookAction::Delete);
        assert_eq!(ask.order.side, OrderSide::Sell);
        assert_eq!(ask.order.size, instrument.make_qty(0.0, None));
        assert_eq!(
            ask.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_quote_produces_top_of_book() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_orderbook_quote(&msg, &instrument, None, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(27450.00));
        assert_eq!(quote.bid_size, instrument.make_qty(0.500, None));
        assert_eq!(quote.ask_price, instrument.make_price(27451.00));
        assert_eq!(quote.ask_size, instrument.make_qty(0.750, None));
    }

    #[rstest]
    fn parse_orderbook_quote_with_delta_updates_sizes() {
        let instrument = linear_instrument();
        let snapshot: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json")).unwrap();
        let base_quote = parse_orderbook_quote(&snapshot, &instrument, None, TS).unwrap();

        let delta: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_delta.json")).unwrap();
        let updated = parse_orderbook_quote(&delta, &instrument, Some(&base_quote), TS).unwrap();

        assert_eq!(updated.bid_price, instrument.make_price(27450.00));
        assert_eq!(updated.bid_size, instrument.make_qty(0.400, None));
        assert_eq!(updated.ask_price, instrument.make_price(27451.00));
        assert_eq!(updated.ask_size, instrument.make_qty(0.0, None));
    }

    #[rstest]
    fn parse_linear_ticker_quote_to_quote_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_ticker_linear.json");
        let msg: BybitWsTickerLinearMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_ticker_linear_quote(&msg, &instrument, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(17215.50));
        assert_eq!(quote.ask_price, instrument.make_price(17216.00));
        assert_eq!(quote.bid_size, instrument.make_qty(84.489, None));
        assert_eq!(quote.ask_size, instrument.make_qty(83.020, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_673_272_861_686_000_000));
        assert_eq!(quote.ts_init, TS);
    }

    #[rstest]
    fn parse_option_ticker_quote_to_quote_tick() {
        let instrument = option_instrument();
        let json = load_test_json("ws_ticker_option.json");
        let msg: BybitWsTickerOptionMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_ticker_option_quote(&msg, &instrument, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(0.0));
        assert_eq!(quote.ask_price, instrument.make_price(10.0));
        assert_eq!(quote.bid_size, instrument.make_qty(0.0, None));
        assert_eq!(quote.ask_size, instrument.make_qty(5.1, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_672_917_511_074_000_000));
        assert_eq!(quote.ts_init, TS);
    }

    #[rstest]
    fn parse_ws_kline_into_bar() {
        use std::num::NonZero;

        let instrument = linear_instrument();
        let json = load_test_json("ws_kline.json");
        let msg: crate::websocket::messages::BybitWsKlineMsg = serde_json::from_str(&json).unwrap();
        let kline = &msg.data[0];

        let bar_spec = BarSpecification {
            step: NonZero::new(5).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

        let bar = parse_ws_kline_bar(kline, &instrument, bar_type, false, TS).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, instrument.make_price(16649.5));
        assert_eq!(bar.high, instrument.make_price(16677.0));
        assert_eq!(bar.low, instrument.make_price(16608.0));
        assert_eq!(bar.close, instrument.make_price(16677.0));
        assert_eq!(bar.volume, instrument.make_qty(2.081, None));
        assert_eq!(bar.ts_event, UnixNanos::new(1_672_324_800_000_000_000));
        assert_eq!(bar.ts_init, TS);
    }

    #[rstest]
    fn parse_ws_order_into_order_status_report() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_order_filled.json");
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();
        let order = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_ws_order_status_report(order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(report.quantity, instrument.make_qty(0.100, None));
        assert_eq!(report.filled_qty, instrument.make_qty(0.100, None));
        assert_eq!(report.price, Some(instrument.make_price(30000.50)));
        assert_eq!(report.avg_px, Some(30000.50));
        assert_eq!(
            report.client_order_id.as_ref().unwrap().to_string(),
            "test-client-order-001"
        );
        assert_eq!(
            report.ts_accepted,
            UnixNanos::new(1_672_364_262_444_000_000)
        );
        assert_eq!(report.ts_last, UnixNanos::new(1_672_364_262_457_000_000));
    }

    #[rstest]
    fn parse_ws_order_partially_filled_rejected_maps_to_canceled() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_order_partially_filled_rejected.json");
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();
        let order = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_ws_order_status_report(order, &instrument, account_id, TS).unwrap();

        // Verify that Bybit "Rejected" status with fills is mapped to Canceled, not Rejected
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, instrument.make_qty(50.0, None));
        assert_eq!(
            report.client_order_id.as_ref().unwrap().to_string(),
            "O-20251001-164609-APEX-000-49"
        );
        assert_eq!(report.cancel_reason, Some("UNKNOWN".to_string()));
    }

    #[rstest]
    fn parse_ws_execution_into_fill_report() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();
        let execution = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_ws_fill_report(execution, account_id, &instrument, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.venue_order_id.to_string(),
            "9aac161b-8ed6-450d-9cab-c5cc67c21784"
        );
        assert_eq!(
            report.trade_id.to_string(),
            "0ab1bdf7-4219-438b-b30a-32ec863018f7"
        );
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty, instrument.make_qty(0.5, None));
        assert_eq!(report.last_px, instrument.make_price(95900.1));
        assert_eq!(report.commission.as_f64(), 26.3725275);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(
            report.client_order_id.as_ref().unwrap().to_string(),
            "test-order-link-001"
        );
        assert_eq!(report.ts_event, UnixNanos::new(1_746_270_400_353_000_000));
    }

    #[rstest]
    fn parse_ws_position_into_position_status_report() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_position.json");
        let msg: crate::websocket::messages::BybitWsAccountPositionMsg =
            serde_json::from_str(&json).unwrap();
        let position = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");

        let report =
            parse_ws_position_status_report(position, account_id, &instrument, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.position_side.as_position_side(), PositionSide::Long);
        assert_eq!(report.quantity, instrument.make_qty(0.15, None));
        assert_eq!(
            report.avg_px_open,
            Some(Decimal::try_from(28500.50).unwrap())
        );
        assert_eq!(report.ts_last, UnixNanos::new(1_697_682_317_038_000_000));
        assert_eq!(report.ts_init, TS);
    }

    #[rstest]
    fn parse_ws_position_short_into_position_status_report() {
        // Create ETHUSDT instrument
        let instruments_json = load_test_json("http_get_instruments_linear.json");
        let instruments_response: crate::http::models::BybitInstrumentLinearResponse =
            serde_json::from_str(&instruments_json).unwrap();
        let eth_def = &instruments_response.result.list[1]; // ETHUSDT is second in the list
        let fee_rate = crate::http::models::BybitFeeRate {
            symbol: ustr::Ustr::from("ETHUSDT"),
            taker_fee_rate: "0.00055".to_string(),
            maker_fee_rate: "0.0001".to_string(),
            base_coin: Some(ustr::Ustr::from("ETH")),
        };
        let instrument =
            crate::common::parse::parse_linear_instrument(eth_def, &fee_rate, TS, TS).unwrap();

        let json = load_test_json("ws_account_position_short.json");
        let msg: crate::websocket::messages::BybitWsAccountPositionMsg =
            serde_json::from_str(&json).unwrap();
        let position = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");

        let report =
            parse_ws_position_status_report(position, account_id, &instrument, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id.symbol.as_str(), "ETHUSDT-LINEAR");
        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity, instrument.make_qty(2.5, None));
        assert_eq!(
            report.avg_px_open,
            Some(Decimal::try_from(2450.75).unwrap())
        );
        assert_eq!(report.ts_last, UnixNanos::new(1_697_682_417_038_000_000));
        assert_eq!(report.ts_init, TS);
    }

    #[rstest]
    fn parse_ws_wallet_into_account_state() {
        let json = load_test_json("ws_account_wallet.json");
        let msg: crate::websocket::messages::BybitWsAccountWalletMsg =
            serde_json::from_str(&json).unwrap();
        let wallet = &msg.data[0];
        let account_id = AccountId::new("BYBIT-001");
        let ts_event = UnixNanos::new(1_700_034_722_104_000_000);

        let state = parse_ws_account_state(wallet, account_id, ts_event, TS).unwrap();

        assert_eq!(state.account_id, account_id);
        assert_eq!(state.account_type, AccountType::Margin);
        assert_eq!(state.balances.len(), 2);
        assert!(state.is_reported);

        // Check BTC balance
        let btc_balance = &state.balances[0];
        assert_eq!(btc_balance.currency.code.as_str(), "BTC");
        assert!((btc_balance.total.as_f64() - 0.00102964).abs() < 1e-8);
        assert!((btc_balance.free.as_f64() - 0.00092964).abs() < 1e-8);
        assert!((btc_balance.locked.as_f64() - 0.0001).abs() < 1e-8);

        // Check USDT balance
        let usdt_balance = &state.balances[1];
        assert_eq!(usdt_balance.currency.code.as_str(), "USDT");
        assert!((usdt_balance.total.as_f64() - 9647.75537647).abs() < 1e-6);
        assert!((usdt_balance.free.as_f64() - 9519.89806037).abs() < 1e-6);
        assert!((usdt_balance.locked.as_f64() - 127.8573161).abs() < 1e-6);

        assert_eq!(state.ts_event, ts_event);
        assert_eq!(state.ts_init, TS);
    }
}

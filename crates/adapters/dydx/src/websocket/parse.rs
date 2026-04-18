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

//! Parsing utilities for dYdX WebSocket messages.
//!
//! Converts WebSocket-specific message formats into Nautilus domain types
//! by transforming them into HTTP-equivalent structures and delegating to
//! the HTTP parser for consistency.

use std::str::FromStr;

use anyhow::Context;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, Data, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, OrderStatus, RecordFlag},
    identifiers::{AccountId, InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::{DydxWsError, DydxWsResult};
use crate::{
    common::{
        enums::{DydxOrderStatus, DydxTickerType},
        instrument_cache::InstrumentCache,
    },
    execution::{encoder::ClientOrderIdEncoder, types::OrderContext},
    http::{
        models::{Fill, Order, PerpetualPosition},
        parse::{parse_fill_report, parse_order_status_report, parse_position_status_report},
    },
    websocket::messages::{
        DydxCandle, DydxOrderbookContents, DydxOrderbookSnapshotContents, DydxPerpetualPosition,
        DydxTradeContents, DydxWsFillSubaccountMessageContents,
        DydxWsOrderSubaccountMessageContents,
    },
};

/// Parses a WebSocket order update into an OrderStatusReport.
///
/// Converts the WebSocket order format to the HTTP Order format, then delegates
/// to the existing HTTP parser for consistency.
///
/// # Arguments
///
/// * `ws_order` - The WebSocket order message to parse
/// * `instrument_cache` - Cache for looking up instruments by clob_pair_id
/// * `order_contexts` - Map of dYdX u32 client IDs to order contexts
/// * `encoder` - Bidirectional encoder for ClientOrderId ↔ u32 mapping
/// * `account_id` - Account ID for the report
/// * `ts_init` - Timestamp for initialization
///
/// # Errors
///
/// Returns an error if:
/// - clob_pair_id cannot be parsed from string.
/// - Instrument lookup fails for the clob_pair_id.
/// - Field parsing fails (price, size, etc.).
/// - HTTP parser fails.
pub fn parse_ws_order_report(
    ws_order: &DydxWsOrderSubaccountMessageContents,
    instrument_cache: &InstrumentCache,
    order_contexts: &DashMap<u32, OrderContext>,
    encoder: &ClientOrderIdEncoder,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let clob_pair_id: u32 = ws_order.clob_pair_id.parse().context(format!(
        "Failed to parse clob_pair_id '{}'",
        ws_order.clob_pair_id
    ))?;

    let instrument = instrument_cache
        .get_by_clob_id(clob_pair_id)
        .ok_or_else(|| {
            instrument_cache.log_missing_clob_pair_id(clob_pair_id);
            anyhow::anyhow!("No instrument cached for clob_pair_id {clob_pair_id}")
        })?;

    let http_order = convert_ws_order_to_http(ws_order)?;
    let mut report = parse_order_status_report(&http_order, &instrument, account_id, ts_init)?;

    let dydx_client_id = ws_order.client_id.parse::<u32>().ok();
    let dydx_client_metadata = ws_order
        .client_metadata
        .as_ref()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);

    log::debug!(
        "[WS_ORDER_RECV] dYdX client_id='{}' meta={:#x} (parsed u32={:?}) | status={:?} | clob_pair={} | side={:?} | size={} | filled={}",
        ws_order.client_id,
        dydx_client_metadata,
        dydx_client_id,
        ws_order.status,
        ws_order.clob_pair_id,
        ws_order.side,
        ws_order.size,
        ws_order.total_filled.as_deref().unwrap_or("?")
    );

    // Look up the original Nautilus client_order_id from the order context first,
    // then fall back to encoder.decode_if_known() if not found in context
    if let Some(client_id) = dydx_client_id {
        if let Some(ctx) = order_contexts.get(&client_id) {
            log::debug!(
                "[WS_ORDER_RECV] DECODE via order_contexts: dYdX u32={} -> Nautilus '{}'",
                client_id,
                ctx.client_order_id
            );
            report.client_order_id = Some(ctx.client_order_id);
        } else if let Some(client_order_id) =
            encoder.decode_if_known(client_id, dydx_client_metadata)
        {
            log::debug!(
                "[WS_ORDER_RECV] DECODE via encoder fallback: dYdX u32={client_id} meta={dydx_client_metadata:#x} -> Nautilus '{client_order_id}'"
            );
            report.client_order_id = Some(client_order_id);
        } else {
            log::debug!(
                "[WS_ORDER_RECV] Unknown order: dYdX u32={client_id} meta={dydx_client_metadata:#x} (external or previous session)"
            );
        }
    } else {
        log::warn!(
            "[WS_ORDER_RECV] Could not parse client_id '{}' as u32",
            ws_order.client_id
        );
    }

    // For untriggered conditional orders with an explicit trigger price we
    // surface `PendingUpdate` to match Nautilus semantics and existing dYdX
    // enum mapping.
    if matches!(ws_order.status, DydxOrderStatus::Untriggered) && ws_order.trigger_price.is_some() {
        report.order_status = OrderStatus::PendingUpdate;
    }

    Ok(report)
}

/// Converts a WebSocket order message to HTTP Order format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_order_to_http(
    ws_order: &DydxWsOrderSubaccountMessageContents,
) -> anyhow::Result<Order> {
    let clob_pair_id: u32 = ws_order
        .clob_pair_id
        .parse()
        .context("Failed to parse clob_pair_id")?;

    let size: Decimal = ws_order.size.parse().context("Failed to parse size")?;

    let total_filled: Decimal = ws_order
        .total_filled
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .context("Failed to parse total_filled")?
        .unwrap_or(Decimal::ZERO);

    // Saturate to zero if total_filled exceeds size (edge case: rounding or partial fills)
    let remaining_size = (size - total_filled).max(Decimal::ZERO);

    let price: Decimal = ws_order.price.parse().context("Failed to parse price")?;

    let created_at_height: u64 = ws_order
        .created_at_height
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .context("Failed to parse created_at_height")?
        .unwrap_or(0);

    let client_metadata: u32 = ws_order
        .client_metadata
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing required field: client_metadata"))?
        .parse()
        .context("Failed to parse client_metadata")?;

    let order_flags: u32 = ws_order
        .order_flags
        .parse()
        .context("Failed to parse order_flags")?;

    let good_til_block = ws_order
        .good_til_block
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok());

    let good_til_block_time = ws_order
        .good_til_block_time
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let trigger_price = ws_order
        .trigger_price
        .as_ref()
        .and_then(|s| Decimal::from_str(s).ok());

    // Parse updated_at (optional for BEST_EFFORT_OPENED orders)
    let updated_at = ws_order
        .updated_at
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Parse updated_at_height (optional for BEST_EFFORT_OPENED orders)
    let updated_at_height = ws_order
        .updated_at_height
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok());

    let total_filled = size.checked_sub(remaining_size).unwrap_or(Decimal::ZERO);

    Ok(Order {
        id: ws_order.id.clone(),
        subaccount_id: ws_order.subaccount_id.clone(),
        client_id: ws_order.client_id.clone(),
        clob_pair_id,
        side: ws_order.side,
        size,
        total_filled,
        price,
        status: ws_order.status,
        order_type: ws_order.order_type,
        time_in_force: ws_order.time_in_force,
        reduce_only: ws_order.reduce_only,
        post_only: ws_order.post_only,
        order_flags,
        good_til_block,
        good_til_block_time,
        created_at_height: Some(created_at_height),
        client_metadata,
        trigger_price,
        condition_type: None, // Not provided in WebSocket messages
        conditional_order_trigger_subticks: None, // Not provided in WebSocket messages
        execution: None,      // Inferred from post_only flag by HTTP parser
        updated_at,
        updated_at_height,
        ticker: None,               // Not provided in WebSocket messages
        subaccount_number: 0,       // Default to 0 for WebSocket messages
        order_router_address: None, // Not provided in WebSocket messages
    })
}

/// Parses a WebSocket fill update into a FillReport.
///
/// Converts the WebSocket fill format to the HTTP Fill format, then delegates
/// to the existing HTTP parser for consistency. Correlates the fill back to the
/// originating order using the `order_id_map` (built from WS order updates).
///
/// # Errors
///
/// Returns an error if:
/// - Instrument lookup fails for the market symbol.
/// - Field parsing fails (price, size, fee, etc.).
/// - HTTP parser fails.
pub fn parse_ws_fill_report(
    ws_fill: &DydxWsFillSubaccountMessageContents,
    instrument_cache: &InstrumentCache,
    order_id_map: &DashMap<String, (u32, u32)>,
    order_contexts: &DashMap<u32, OrderContext>,
    encoder: &ClientOrderIdEncoder,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument = instrument_cache
        .get_by_market(&ws_fill.market)
        .ok_or_else(|| {
            let available: Vec<String> = instrument_cache
                .all_instruments()
                .into_iter()
                .map(|inst| inst.id().symbol.to_string())
                .collect();
            anyhow::anyhow!(
                "No instrument cached for market '{}'. Available: {:?}",
                ws_fill.market,
                available
            )
        })?;

    let http_fill = convert_ws_fill_to_http(ws_fill)?;
    let mut report = parse_fill_report(&http_fill, &instrument, account_id, ts_init)?;

    // Correlate fill to order via order_id → (client_id, client_metadata) → client_order_id
    if let Some(ref order_id) = ws_fill.order_id {
        if let Some(entry) = order_id_map.get(order_id) {
            let (client_id, client_metadata) = *entry.value();
            if let Some(ctx) = order_contexts.get(&client_id) {
                report.client_order_id = Some(ctx.client_order_id);
            } else if let Some(client_order_id) =
                encoder.decode_if_known(client_id, client_metadata)
            {
                report.client_order_id = Some(client_order_id);
            } else {
                log::debug!(
                    "[WS_FILL_RECV] Unknown order: order_id={order_id} -> client_id={client_id} meta={client_metadata:#x} (external or previous session)",
                );
            }
        } else {
            log::warn!(
                "[WS_FILL_RECV] No order_id mapping for '{order_id}', fill cannot be correlated",
            );
        }
    }

    Ok(report)
}

/// Converts a WebSocket fill message to HTTP Fill format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_fill_to_http(ws_fill: &DydxWsFillSubaccountMessageContents) -> anyhow::Result<Fill> {
    let price: Decimal = ws_fill.price.parse().context("Failed to parse price")?;
    let size: Decimal = ws_fill.size.parse().context("Failed to parse size")?;
    let fee: Decimal = ws_fill.fee.parse().context("Failed to parse fee")?;

    let created_at_height: u64 = ws_fill
        .created_at_height
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .context("Failed to parse created_at_height")?
        .unwrap_or(0);

    let client_metadata: u32 = ws_fill
        .client_metadata
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing required field: client_metadata"))?
        .parse()
        .context("Failed to parse client_metadata")?;

    let order_id = ws_fill
        .order_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing required field: order_id"))?;

    let created_at = DateTime::parse_from_rfc3339(&ws_fill.created_at)
        .context("Failed to parse created_at")?
        .with_timezone(&Utc);

    Ok(Fill {
        id: ws_fill.id.clone(),
        side: ws_fill.side,
        liquidity: ws_fill.liquidity,
        fill_type: ws_fill.fill_type,
        market: ws_fill.market,
        market_type: ws_fill.market_type.unwrap_or(DydxTickerType::Perpetual),
        price,
        size,
        fee,
        created_at,
        created_at_height,
        order_id,
        client_metadata,
    })
}

/// Parses a WebSocket position into a PositionStatusReport.
///
/// Converts the WebSocket position format to the HTTP PerpetualPosition format,
/// then delegates to the existing HTTP parser for consistency.
///
/// # Errors
///
/// Returns an error if:
/// - Instrument lookup fails for the market symbol.
/// - Field parsing fails (size, prices, etc.).
/// - HTTP parser fails.
pub fn parse_ws_position_report(
    ws_position: &DydxPerpetualPosition,
    instrument_cache: &InstrumentCache,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument = instrument_cache
        .get_by_market(&ws_position.market)
        .ok_or_else(|| {
            let available: Vec<String> = instrument_cache
                .all_instruments()
                .into_iter()
                .map(|inst| inst.id().symbol.to_string())
                .collect();
            anyhow::anyhow!(
                "No instrument cached for market '{}'. Available: {:?}",
                ws_position.market,
                available
            )
        })?;

    let http_position = convert_ws_position_to_http(ws_position)?;
    parse_position_status_report(&http_position, &instrument, account_id, ts_init)
}

/// Converts a WebSocket position to HTTP PerpetualPosition format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_position_to_http(
    ws_position: &DydxPerpetualPosition,
) -> anyhow::Result<PerpetualPosition> {
    let size: Decimal = ws_position.size.parse().context("Failed to parse size")?;

    let max_size: Decimal = ws_position
        .max_size
        .parse()
        .context("Failed to parse max_size")?;

    let entry_price: Decimal = ws_position
        .entry_price
        .parse()
        .context("Failed to parse entry_price")?;

    let exit_price: Option<Decimal> = ws_position
        .exit_price
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .context("Failed to parse exit_price")?;

    let realized_pnl: Decimal = ws_position
        .realized_pnl
        .parse()
        .context("Failed to parse realized_pnl")?;

    let unrealized_pnl: Decimal = ws_position
        .unrealized_pnl
        .parse()
        .context("Failed to parse unrealized_pnl")?;

    let sum_open: Decimal = ws_position
        .sum_open
        .parse()
        .context("Failed to parse sum_open")?;

    let sum_close: Decimal = ws_position
        .sum_close
        .parse()
        .context("Failed to parse sum_close")?;

    let net_funding: Decimal = ws_position
        .net_funding
        .parse()
        .context("Failed to parse net_funding")?;

    let created_at = DateTime::parse_from_rfc3339(&ws_position.created_at)
        .context("Failed to parse created_at")?
        .with_timezone(&Utc);

    let closed_at = ws_position
        .closed_at
        .as_ref()
        .map(|s| DateTime::parse_from_rfc3339(s))
        .transpose()
        .context("Failed to parse closed_at")?
        .map(|dt| dt.with_timezone(&Utc));

    // Preserve the venue-supplied side; only derive from size sign when side is absent
    // (the WS schema always provides it, but this keeps the behavior explicit).
    let side = ws_position.side;

    Ok(PerpetualPosition {
        market: ws_position.market,
        status: ws_position.status,
        side,
        size,
        max_size,
        entry_price,
        exit_price,
        realized_pnl,
        created_at_height: 0, // Not provided in WebSocket messages
        created_at,
        sum_open,
        sum_close,
        net_funding,
        unrealized_pnl,
        closed_at,
    })
}

// ---------------------------------------------------------------------------
//  Market data parsing functions
// ---------------------------------------------------------------------------

/// Parses an orderbook snapshot into [`OrderBookDeltas`].
///
/// # Errors
///
/// Returns an error if price/size parsing fails.
pub fn parse_orderbook_snapshot(
    instrument_id: &InstrumentId,
    contents: &DydxOrderbookSnapshotContents,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> DydxWsResult<OrderBookDeltas> {
    let bids = contents.bids.as_deref().unwrap_or(&[]);
    let asks = contents.asks.as_deref().unwrap_or(&[]);

    let mut deltas = Vec::with_capacity(1 + bids.len() + asks.len());
    let snapshot_flag = RecordFlag::F_SNAPSHOT as u8;

    // Empty book snapshot: Clear alone must carry F_SNAPSHOT | F_LAST
    if bids.is_empty() && asks.is_empty() {
        let clear_flags = snapshot_flag | RecordFlag::F_LAST as u8;
        let mut clear_delta = OrderBookDelta::clear(*instrument_id, 0, ts_init, ts_init);
        clear_delta.flags = clear_flags;
        deltas.push(clear_delta);
        return Ok(OrderBookDeltas::new(*instrument_id, deltas));
    }

    // Non-empty: Clear carries F_SNAPSHOT (not last)
    let mut clear_delta = OrderBookDelta::clear(*instrument_id, 0, ts_init, ts_init);
    clear_delta.flags = snapshot_flag;
    deltas.push(clear_delta);

    let bids_len = bids.len();
    let asks_len = asks.len();

    for (idx, bid) in bids.iter().enumerate() {
        let is_last = idx == bids_len - 1 && asks_len == 0;
        let flags = if is_last {
            snapshot_flag | RecordFlag::F_LAST as u8
        } else {
            snapshot_flag
        };

        let price = Decimal::from_str(&bid.price)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

        let size = Decimal::from_str(&bid.size)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from_decimal_dp(price, price_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
            })?,
            Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
            })?,
            0,
        );

        deltas.push(OrderBookDelta::new(
            *instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    for (idx, ask) in asks.iter().enumerate() {
        let is_last = idx == asks_len - 1;
        let flags = if is_last {
            snapshot_flag | RecordFlag::F_LAST as u8
        } else {
            snapshot_flag
        };

        let price = Decimal::from_str(&ask.price)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

        let size = Decimal::from_str(&ask.size)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from_decimal_dp(price, price_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
            })?,
            Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
            })?,
            0,
        );

        deltas.push(OrderBookDelta::new(
            *instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    Ok(OrderBookDeltas::new(*instrument_id, deltas))
}

/// Parses orderbook deltas (marks as last message by default).
///
/// # Errors
///
/// Returns an error if price/size parsing fails.
pub fn parse_orderbook_deltas(
    instrument_id: &InstrumentId,
    contents: &DydxOrderbookContents,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> DydxWsResult<OrderBookDeltas> {
    let deltas = parse_orderbook_deltas_with_flag(
        instrument_id,
        contents,
        price_precision,
        size_precision,
        ts_init,
        true,
    )?;
    Ok(OrderBookDeltas::new(*instrument_id, deltas))
}

/// Parses orderbook deltas with explicit last-message flag for batch processing.
///
/// # Errors
///
/// Returns an error if price/size parsing fails.
pub fn parse_orderbook_deltas_with_flag(
    instrument_id: &InstrumentId,
    contents: &DydxOrderbookContents,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
    is_last_message: bool,
) -> DydxWsResult<Vec<OrderBookDelta>> {
    let mut deltas = Vec::new();

    let bids = contents.bids.as_deref().unwrap_or(&[]);
    let asks = contents.asks.as_deref().unwrap_or(&[]);

    let bids_len = bids.len();
    let asks_len = asks.len();

    for (idx, (price_str, size_str)) in bids.iter().enumerate() {
        let is_last = is_last_message && idx == bids_len - 1 && asks_len == 0;
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let price = Decimal::from_str(price_str)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

        let size = Decimal::from_str(size_str)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

        let qty = Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
        })?;
        let action = if qty.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from_decimal_dp(price, price_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
            })?,
            qty,
            0,
        );

        deltas.push(OrderBookDelta::new(
            *instrument_id,
            action,
            order,
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    for (idx, (price_str, size_str)) in asks.iter().enumerate() {
        let is_last = is_last_message && idx == asks_len - 1;
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let price = Decimal::from_str(price_str)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

        let size = Decimal::from_str(size_str)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

        let qty = Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
        })?;
        let action = if qty.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from_decimal_dp(price, price_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
            })?,
            qty,
            0,
        );

        deltas.push(OrderBookDelta::new(
            *instrument_id,
            action,
            order,
            flags,
            0,
            ts_init,
            ts_init,
        ));
    }

    Ok(deltas)
}

/// Parses trade ticks from trade contents.
///
/// # Errors
///
/// Returns an error if price/size/timestamp parsing fails.
pub fn parse_trade_ticks(
    instrument_id: InstrumentId,
    instrument: &InstrumentAny,
    contents: &DydxTradeContents,
    ts_init: UnixNanos,
) -> DydxWsResult<Vec<Data>> {
    let mut ticks = Vec::new();

    for trade in &contents.trades {
        let aggressor_side = match trade.side {
            OrderSide::Buy => AggressorSide::Buyer,
            OrderSide::Sell => AggressorSide::Seller,
            _ => continue,
        };

        let price = Decimal::from_str(&trade.price)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade price: {e}")))?;

        let size = Decimal::from_str(&trade.size)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade size: {e}")))?;

        let trade_ts = trade.created_at.timestamp_nanos_opt().ok_or_else(|| {
            DydxWsError::Parse(format!("Timestamp out of range for trade {}", trade.id))
        })?;

        let tick = TradeTick::new(
            instrument_id,
            Price::from_decimal_dp(price, instrument.price_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
            })?,
            Quantity::from_decimal_dp(size, instrument.size_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
            })?,
            aggressor_side,
            TradeId::new(&trade.id),
            UnixNanos::from(trade_ts as u64),
            ts_init,
        );
        ticks.push(Data::Trade(tick));
    }

    Ok(ticks)
}

/// Parses a single candle into a [`Bar`].
///
/// When `timestamp_on_close` is true, `ts_event` is set to bar close time
/// (started_at + interval). When false, uses the venue-native open time.
///
/// # Errors
///
/// Returns an error if OHLCV/timestamp parsing fails.
pub fn parse_candle_bar(
    bar_type: BarType,
    instrument: &InstrumentAny,
    candle: &DydxCandle,
    timestamp_on_close: bool,
    ts_init: UnixNanos,
) -> DydxWsResult<Bar> {
    let open = Decimal::from_str(&candle.open)
        .map_err(|e| DydxWsError::Parse(format!("Failed to parse open: {e}")))?;
    let high = Decimal::from_str(&candle.high)
        .map_err(|e| DydxWsError::Parse(format!("Failed to parse high: {e}")))?;
    let low = Decimal::from_str(&candle.low)
        .map_err(|e| DydxWsError::Parse(format!("Failed to parse low: {e}")))?;
    let close = Decimal::from_str(&candle.close)
        .map_err(|e| DydxWsError::Parse(format!("Failed to parse close: {e}")))?;
    let volume = candle
        .base_token_volume
        .as_deref()
        .map(Decimal::from_str)
        .transpose()
        .map_err(|e| DydxWsError::Parse(format!("Failed to parse volume: {e}")))?
        .unwrap_or(Decimal::ZERO);

    let started_at_nanos = candle.started_at.timestamp_nanos_opt().ok_or_else(|| {
        DydxWsError::Parse(format!(
            "Timestamp out of range for candle at {}",
            candle.started_at
        ))
    })?;
    let mut ts_event = UnixNanos::from(started_at_nanos as u64);

    if timestamp_on_close {
        let interval_ns = bar_type
            .spec()
            .timedelta()
            .num_nanoseconds()
            .ok_or_else(|| DydxWsError::Parse("Bar interval overflow".to_string()))?;
        let updated = (started_at_nanos as u64)
            .checked_add(interval_ns as u64)
            .ok_or_else(|| {
                DydxWsError::Parse("Bar timestamp overflowed adjusting to close time".to_string())
            })?;
        ts_event = UnixNanos::from(updated);
    }

    let bar = Bar::new(
        bar_type,
        Price::from_decimal_dp(open, instrument.price_precision()).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create open Price from decimal: {e}"))
        })?,
        Price::from_decimal_dp(high, instrument.price_precision()).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create high Price from decimal: {e}"))
        })?,
        Price::from_decimal_dp(low, instrument.price_precision()).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create low Price from decimal: {e}"))
        })?,
        Price::from_decimal_dp(close, instrument.price_precision()).map_err(|e| {
            DydxWsError::Parse(format!("Failed to create close Price from decimal: {e}"))
        })?,
        Quantity::from_decimal_dp(volume, instrument.size_precision()).map_err(|e| {
            DydxWsError::Parse(format!(
                "Failed to create volume Quantity from decimal: {e}"
            ))
        })?,
        ts_event,
        ts_init,
    );

    Ok(bar)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        data::{BarType, Data},
        enums::{
            AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus, OrderType,
            PositionSideSpecified,
        },
        identifiers::{AccountId, InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            enums::{
                DydxFillType, DydxLiquidity, DydxMarketStatus, DydxOrderStatus, DydxOrderType,
                DydxPositionSide, DydxPositionStatus, DydxTickerType, DydxTimeInForce,
            },
            testing::load_json_fixture,
        },
        http::models::PerpetualMarket,
        websocket::messages::{DydxPerpetualPosition, DydxWsFillSubaccountMessageContents},
    };

    /// Creates a test market with BTC-USD ticker and specified clob_pair_id.
    fn create_test_market(ticker: &str, clob_pair_id: u32) -> PerpetualMarket {
        PerpetualMarket {
            clob_pair_id,
            ticker: Ustr::from(ticker),
            status: DydxMarketStatus::Active,
            base_asset: Some(Ustr::from("BTC")),
            quote_asset: Some(Ustr::from("USD")),
            step_size: dec!(0.001),
            tick_size: dec!(0.01),
            index_price: Some(dec!(50000)),
            oracle_price: Some(dec!(50000)),
            price_change_24h: dec!(0),
            next_funding_rate: dec!(0),
            next_funding_at: None,
            min_order_size: Some(dec!(0.001)),
            market_type: None,
            initial_margin_fraction: dec!(0.05),
            maintenance_margin_fraction: dec!(0.03),
            base_position_notional: None,
            incremental_position_size: None,
            incremental_initial_margin_fraction: None,
            max_position_size: None,
            open_interest: dec!(1000),
            atomic_resolution: -10,
            quantum_conversion_exponent: -9,
            subticks_per_tick: 1000000,
            step_base_quantums: 1000000,
            is_reduce_only: false,
        }
    }

    /// Creates an InstrumentCache populated with the test instrument.
    fn create_test_instrument_cache() -> InstrumentCache {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument();
        let market = create_test_market("BTC-USD", 1);
        cache.insert(instrument, market);
        cache
    }

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("BTC-USD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,
            8,
            Price::new(0.01, 2),
            Quantity::new(0.001, 8),
            Some(Quantity::new(1.0, 0)),
            Some(Quantity::new(0.001, 8)),
            Some(Quantity::new(100000.0, 8)),
            Some(Quantity::new(0.001, 8)),
            None,
            None,
            Some(Price::new(1000000.0, 2)),
            Some(Price::new(0.01, 2)),
            Some(rust_decimal_macros::dec!(0.05)),
            Some(rust_decimal_macros::dec!(0.03)),
            Some(rust_decimal_macros::dec!(0.0002)),
            Some(rust_decimal_macros::dec!(0.0005)),
            None, // info: Option<Params>
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_convert_ws_order_to_http_basic() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order123".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "12345".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "1.5".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::PartiallyFilled,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("900".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some("0.5".to_string()),
            updated_at: Some("2024-11-14T10:00:00Z".to_string()),
            updated_at_height: Some("950".to_string()),
        };

        let result = convert_ws_order_to_http(&ws_order);
        assert!(result.is_ok());

        let http_order = result.unwrap();
        assert_eq!(http_order.id, "order123");
        assert_eq!(http_order.clob_pair_id, 1);
        assert_eq!(http_order.size.to_string(), "1.5");
        assert_eq!(http_order.total_filled, rust_decimal_macros::dec!(0.5)); // 0.5 filled
        assert_eq!(http_order.status, DydxOrderStatus::PartiallyFilled);
    }

    #[rstest]
    fn test_parse_ws_order_report_success() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order456".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "67890".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Sell,
            size: "2.0".to_string(),
            price: "51000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: true,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("2000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("1800".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some("0.0".to_string()),
            updated_at: None,
            updated_at_height: None,
        };

        let instrument_cache = create_test_instrument_cache();
        let encoder = ClientOrderIdEncoder::new();

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.order_side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_ws_order_report_missing_instrument() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order789".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "11111".to_string(),
            clob_pair_id: "99".to_string(), // Non-existent
            side: OrderSide::Buy,
            size: "1.0".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Market,
            time_in_force: DydxTimeInForce::Ioc,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("900".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some("0.0".to_string()),
            updated_at: None,
            updated_at_height: None,
        };

        let instrument_cache = InstrumentCache::new(); // Empty cache
        let encoder = ClientOrderIdEncoder::new();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached")
        );
    }

    #[rstest]
    fn test_convert_ws_fill_to_http() {
        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill123".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD".into(),
            market_type: Some(DydxTickerType::Perpetual),
            price: "50000.5".to_string(),
            size: "0.1".to_string(),
            fee: "-2.5".to_string(), // Negative for maker rebate
            created_at: "2024-01-15T10:30:00Z".to_string(),
            created_at_height: Some("12345".to_string()),
            order_id: Some("order456".to_string()),
            client_metadata: Some("999".to_string()),
        };

        let result = convert_ws_fill_to_http(&ws_fill);
        assert!(result.is_ok());

        let http_fill = result.unwrap();
        assert_eq!(http_fill.id, "fill123");
        assert_eq!(http_fill.side, OrderSide::Buy);
        assert_eq!(http_fill.liquidity, DydxLiquidity::Maker);
        assert_eq!(http_fill.price, rust_decimal_macros::dec!(50000.5));
        assert_eq!(http_fill.size, rust_decimal_macros::dec!(0.1));
        assert_eq!(http_fill.fee, rust_decimal_macros::dec!(-2.5));
        assert_eq!(http_fill.created_at_height, 12345);
        assert_eq!(http_fill.order_id, "order456");
        assert_eq!(http_fill.client_metadata, 999);
    }

    #[rstest]
    fn test_parse_ws_fill_report_success() {
        let instrument_cache = create_test_instrument_cache();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        // dYdX WS fills use market format "BTC-USD" (not "BTC-USD-PERP")
        // but the instrument symbol is "BTC-USD-PERP"
        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill789".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Sell,
            liquidity: DydxLiquidity::Taker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD".into(),
            market_type: Some(DydxTickerType::Perpetual),
            price: "49500.0".to_string(),
            size: "0.5".to_string(),
            fee: "12.375".to_string(), // Positive for taker fee
            created_at: "2024-01-15T11:00:00Z".to_string(),
            created_at_height: Some("12400".to_string()),
            order_id: Some("order999".to_string()),
            client_metadata: Some("888".to_string()),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_id_map = DashMap::new();
        let order_contexts = DashMap::new();
        let encoder = ClientOrderIdEncoder::new();

        let result = parse_ws_fill_report(
            &ws_fill,
            &instrument_cache,
            &order_id_map,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.instrument_id, instrument_id);
        assert_eq!(fill_report.venue_order_id.as_str(), "order999");
        assert_eq!(fill_report.last_qty.as_f64(), 0.5);
        assert_eq!(fill_report.last_px.as_f64(), 49500.0);
        assert_eq!(fill_report.commission.as_decimal(), dec!(12.38));
    }

    #[rstest]
    fn test_parse_ws_fill_report_missing_instrument() {
        let instrument_cache = InstrumentCache::new(); // Empty - no instruments cached

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill000".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "ETH-USD-PERP".into(),
            market_type: Some(DydxTickerType::Perpetual),
            price: "3000.0".to_string(),
            size: "1.0".to_string(),
            fee: "-1.5".to_string(),
            created_at: "2024-01-15T12:00:00Z".to_string(),
            created_at_height: Some("12500".to_string()),
            order_id: Some("order111".to_string()),
            client_metadata: Some("777".to_string()),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_id_map = DashMap::new();
        let order_contexts = DashMap::new();
        let encoder = ClientOrderIdEncoder::new();

        let result = parse_ws_fill_report(
            &ws_fill,
            &instrument_cache,
            &order_id_map,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached for market")
        );
    }

    #[rstest]
    fn test_convert_ws_position_to_http() {
        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: DydxPositionSide::Long,
            size: "1.5".to_string(),
            max_size: "2.0".to_string(),
            entry_price: "50000.0".to_string(),
            exit_price: None,
            realized_pnl: "100.0".to_string(),
            unrealized_pnl: "250.5".to_string(),
            created_at: "2024-01-15T10:00:00Z".to_string(),
            closed_at: None,
            sum_open: "5.0".to_string(),
            sum_close: "3.5".to_string(),
            net_funding: "-10.25".to_string(),
        };

        let result = convert_ws_position_to_http(&ws_position);
        assert!(result.is_ok());

        let http_position = result.unwrap();
        assert_eq!(http_position.market, "BTC-USD");
        assert_eq!(http_position.status, DydxPositionStatus::Open);
        assert_eq!(http_position.side, DydxPositionSide::Long); // Positive size = Long
        assert_eq!(http_position.size, rust_decimal_macros::dec!(1.5));
        assert_eq!(http_position.max_size, rust_decimal_macros::dec!(2.0));
        assert_eq!(
            http_position.entry_price,
            rust_decimal_macros::dec!(50000.0)
        );
        assert_eq!(http_position.exit_price, None);
        assert_eq!(http_position.realized_pnl, rust_decimal_macros::dec!(100.0));
        assert_eq!(
            http_position.unrealized_pnl,
            rust_decimal_macros::dec!(250.5)
        );
        assert_eq!(http_position.sum_open, rust_decimal_macros::dec!(5.0));
        assert_eq!(http_position.sum_close, rust_decimal_macros::dec!(3.5));
        assert_eq!(http_position.net_funding, rust_decimal_macros::dec!(-10.25));
    }

    /// The converter must preserve the venue-supplied `side`, not re-derive it from
    /// the sign of `size`. A zero-size position reported as `Long` must stay `Long`,
    /// and a mismatched (Long, negative-size) payload must retain the venue side.
    #[rstest]
    #[case::long_positive(DydxPositionSide::Long, "1.0", DydxPositionSide::Long)]
    #[case::short_negative(DydxPositionSide::Short, "-1.0", DydxPositionSide::Short)]
    #[case::long_zero(DydxPositionSide::Long, "0.0", DydxPositionSide::Long)]
    #[case::short_zero(DydxPositionSide::Short, "0.0", DydxPositionSide::Short)]
    #[case::long_with_negative_size(DydxPositionSide::Long, "-1.0", DydxPositionSide::Long)]
    #[case::short_with_positive_size(DydxPositionSide::Short, "1.0", DydxPositionSide::Short)]
    fn test_convert_ws_position_preserves_venue_side(
        #[case] venue_side: DydxPositionSide,
        #[case] size: &str,
        #[case] expected_side: DydxPositionSide,
    ) {
        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: venue_side,
            size: size.to_string(),
            max_size: "1.0".to_string(),
            entry_price: "50000.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "0.0".to_string(),
            created_at: "2024-01-15T10:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.0".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "0.0".to_string(),
        };

        let http_position =
            convert_ws_position_to_http(&ws_position).expect("conversion should succeed");
        assert_eq!(http_position.side, expected_side);
    }

    /// End-to-end verification that the venue-supplied side flows through to the
    /// emitted `PositionStatusReport`. The previous implementation re-derived side
    /// from `size.is_sign_positive()` inside `parse_position_status_report`, which
    /// silently overrode the venue side for the mismatched case below.
    #[rstest]
    fn test_ws_position_report_emits_venue_side_for_mismatched_size() {
        use nautilus_model::enums::PositionSideSpecified;

        let instrument_cache = create_test_instrument_cache();
        // Venue reports a Short position but the `size` field would round to
        // positive via the legacy sign check. The report must show Short.
        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: DydxPositionSide::Short,
            size: "1.0".to_string(),
            max_size: "1.0".to_string(),
            entry_price: "50000.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "0.0".to_string(),
            created_at: "2024-01-15T10:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.0".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "0.0".to_string(),
        };

        let report = parse_ws_position_report(
            &ws_position,
            &instrument_cache,
            AccountId::new("DYDX-001"),
            UnixNanos::default(),
        )
        .expect("parse should succeed");
        assert_eq!(report.position_side, PositionSideSpecified::Short);
    }

    #[rstest]
    fn test_parse_ws_position_report_success() {
        let instrument_cache = create_test_instrument_cache();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: DydxPositionSide::Long,
            size: "0.5".to_string(),
            max_size: "1.0".to_string(),
            entry_price: "49500.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "125.0".to_string(),
            created_at: "2024-01-15T09:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.5".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "-2.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instrument_cache, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        assert_eq!(position_report.position_side, PositionSideSpecified::Long);
        assert_eq!(position_report.quantity.as_f64(), 0.5);
        // avg_px_open should be entry_price
        assert!(position_report.avg_px_open.is_some());
    }

    #[rstest]
    fn test_parse_ws_position_report_short() {
        let instrument_cache = create_test_instrument_cache();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: DydxPositionSide::Short,
            size: "-0.25".to_string(), // Negative for short
            max_size: "0.5".to_string(),
            entry_price: "51000.0".to_string(),
            exit_price: None,
            realized_pnl: "50.0".to_string(),
            unrealized_pnl: "-75.25".to_string(),
            created_at: "2024-01-15T08:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.25".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "1.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instrument_cache, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        assert_eq!(position_report.position_side, PositionSideSpecified::Short);
        assert_eq!(position_report.quantity.as_f64(), 0.25); // Quantity is always positive
    }

    #[rstest]
    fn test_parse_ws_position_report_missing_instrument() {
        let instrument_cache = InstrumentCache::new(); // Empty - no instruments cached

        let ws_position = DydxPerpetualPosition {
            market: "ETH-USD-PERP".into(),
            status: DydxPositionStatus::Open,
            side: DydxPositionSide::Long,
            size: "5.0".to_string(),
            max_size: "10.0".to_string(),
            entry_price: "3000.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "500.0".to_string(),
            created_at: "2024-01-15T07:00:00Z".to_string(),
            closed_at: None,
            sum_open: "5.0".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "-5.0".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instrument_cache, account_id, ts_init);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached for market")
        );
    }

    #[rstest]
    #[case(DydxOrderStatus::Filled, "2.0")]
    #[case(DydxOrderStatus::Canceled, "0.0")]
    #[case(DydxOrderStatus::BestEffortCanceled, "0.5")]
    #[case(DydxOrderStatus::BestEffortOpened, "0.0")]
    #[case(DydxOrderStatus::Untriggered, "0.0")]
    fn test_parse_ws_order_various_statuses(
        #[case] status: DydxOrderStatus,
        #[case] total_filled: &str,
    ) {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: format!("order_{status:?}"),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "99999".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "2.0".to_string(),
            price: "50000.0".to_string(),
            status,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("900".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some(total_filled.to_string()),
            updated_at: Some("2024-11-14T10:00:00Z".to_string()),
            updated_at_height: Some("950".to_string()),
        };

        let instrument_cache = create_test_instrument_cache();
        let encoder = ClientOrderIdEncoder::new();

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(
            result.is_ok(),
            "Failed to parse order with status {status:?}"
        );
        let report = result.unwrap();

        // Verify status conversion
        let expected_status = match status {
            DydxOrderStatus::Open
            | DydxOrderStatus::BestEffortOpened
            | DydxOrderStatus::Untriggered => OrderStatus::Accepted,
            DydxOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
            DydxOrderStatus::Filled => OrderStatus::Filled,
            DydxOrderStatus::Canceled | DydxOrderStatus::BestEffortCanceled => {
                OrderStatus::Canceled
            }
        };
        assert_eq!(report.order_status, expected_status);
    }

    #[rstest]
    fn test_parse_ws_order_with_trigger_price() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "conditional_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "88888".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Sell,
            size: "1.0".to_string(),
            price: "52000.0".to_string(),
            status: DydxOrderStatus::Untriggered,
            order_type: DydxOrderType::StopLimit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: true,
            order_flags: "32".to_string(),
            good_til_block: None,
            good_til_block_time: Some("2024-12-31T23:59:59Z".to_string()),
            created_at_height: Some("1000".to_string()),
            client_metadata: Some("100".to_string()),
            trigger_price: Some("51500.0".to_string()),
            total_filled: Some("0.0".to_string()),
            updated_at: Some("2024-11-14T11:00:00Z".to_string()),
            updated_at_height: Some("1050".to_string()),
        };

        let instrument_cache = create_test_instrument_cache();
        let encoder = ClientOrderIdEncoder::new();

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.order_status, OrderStatus::PendingUpdate);
        // Trigger price should be parsed and available in the report
        assert!(report.trigger_price.is_some());
    }

    #[rstest]
    fn test_parse_ws_order_market_type() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "market_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "77777".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "0.5".to_string(),
            price: "50000.0".to_string(), // Market orders still have a price
            status: DydxOrderStatus::Filled,
            order_type: DydxOrderType::Market,
            time_in_force: DydxTimeInForce::Ioc,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("900".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some("0.5".to_string()),
            updated_at: Some("2024-11-14T10:01:00Z".to_string()),
            updated_at_height: Some("901".to_string()),
        };

        let instrument_cache = create_test_instrument_cache();
        let encoder = ClientOrderIdEncoder::new();

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.order_type, OrderType::Market);
        assert_eq!(report.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_ws_order_invalid_clob_pair_id() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "bad_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "12345".to_string(),
            clob_pair_id: "not_a_number".to_string(), // Invalid
            side: OrderSide::Buy,
            size: "1.0".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: Some("900".to_string()),
            client_metadata: Some("0".to_string()),
            trigger_price: None,
            total_filled: Some("0.0".to_string()),
            updated_at: None,
            updated_at_height: None,
        };

        let instrument_cache = InstrumentCache::new(); // Empty cache
        let encoder = ClientOrderIdEncoder::new();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_contexts: DashMap<u32, OrderContext> = DashMap::new();

        let result = parse_ws_order_report(
            &ws_order,
            &instrument_cache,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to parse clob_pair_id")
        );
    }

    #[rstest]
    fn test_parse_ws_position_closed() {
        let instrument_cache = create_test_instrument_cache();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Closed,
            side: DydxPositionSide::Long,
            size: "0.0".to_string(), // Closed = zero size
            max_size: "2.0".to_string(),
            entry_price: "48000.0".to_string(),
            exit_price: Some("52000.0".to_string()),
            realized_pnl: "2000.0".to_string(),
            unrealized_pnl: "0.0".to_string(),
            created_at: "2024-01-10T09:00:00Z".to_string(),
            closed_at: Some("2024-01-15T14:00:00Z".to_string()),
            sum_open: "5.0".to_string(),
            sum_close: "5.0".to_string(), // Fully closed
            net_funding: "-25.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instrument_cache, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        // Closed position should have zero quantity
        assert_eq!(position_report.quantity.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_ws_fill_with_maker_rebate() {
        let instrument_cache = create_test_instrument_cache();

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill_rebate".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD".into(),
            market_type: Some(DydxTickerType::Perpetual),
            price: "50000.0".to_string(),
            size: "1.0".to_string(),
            fee: "-15.0".to_string(), // Negative fee = rebate
            created_at: "2024-01-15T13:00:00Z".to_string(),
            created_at_height: Some("13000".to_string()),
            order_id: Some("order_maker".to_string()),
            client_metadata: Some("200".to_string()),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_id_map = DashMap::new();
        let order_contexts = DashMap::new();
        let encoder = ClientOrderIdEncoder::new();

        let result = parse_ws_fill_report(
            &ws_fill,
            &instrument_cache,
            &order_id_map,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
        assert!(fill_report.commission.as_decimal() < dec!(0));
    }

    #[rstest]
    fn test_parse_ws_fill_taker_with_fee() {
        let instrument_cache = create_test_instrument_cache();

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill_taker".to_string(),
            subaccount_id: "sub2".to_string(),
            side: OrderSide::Sell,
            liquidity: DydxLiquidity::Taker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD".into(),
            market_type: Some(DydxTickerType::Perpetual),
            price: "49800.0".to_string(),
            size: "0.75".to_string(),
            fee: "18.675".to_string(), // Positive fee for taker
            created_at: "2024-01-15T14:00:00Z".to_string(),
            created_at_height: Some("14000".to_string()),
            order_id: Some("order_taker".to_string()),
            client_metadata: Some("300".to_string()),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();
        let order_id_map = DashMap::new();
        let order_contexts = DashMap::new();
        let encoder = ClientOrderIdEncoder::new();

        let result = parse_ws_fill_report(
            &ws_fill,
            &instrument_cache,
            &order_id_map,
            &order_contexts,
            &encoder,
            account_id,
            ts_init,
        );
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill_report.order_side, OrderSide::Sell);
        assert!(fill_report.commission.as_decimal() > dec!(0));
    }

    #[rstest]
    fn test_parse_orderbook_snapshot() {
        let json = load_json_fixture("ws_orderbook_subscribed.json");
        let contents: DydxOrderbookSnapshotContents =
            serde_json::from_value(json["contents"].clone())
                .expect("Failed to parse orderbook snapshot contents");

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_orderbook_snapshot(&instrument_id, &contents, 2, 8, ts_init)
            .expect("Failed to parse orderbook snapshot");

        // 1 clear + 3 bids + 3 asks = 7 deltas
        assert_eq!(deltas.deltas.len(), 7);

        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price.to_string(), "43240.00");
        assert_eq!(deltas.deltas[1].order.size.to_string(), "1.50000000");

        assert_eq!(deltas.deltas[4].action, BookAction::Add);
        assert_eq!(deltas.deltas[4].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[4].order.price.to_string(), "43250.00");
        assert_eq!(deltas.deltas[4].order.size.to_string(), "1.20000000");

        // Every snapshot delta must carry F_SNAPSHOT. The Clear carries F_SNAPSHOT only
        // (not last); every intermediate delta carries F_SNAPSHOT only; the terminator
        // carries F_SNAPSHOT | F_LAST.
        let snapshot = RecordFlag::F_SNAPSHOT as u8;
        let last_flag = RecordFlag::F_LAST as u8;

        assert_eq!(deltas.deltas[0].flags, snapshot, "Clear missing F_SNAPSHOT");
        for (idx, delta) in deltas.deltas.iter().enumerate().skip(1) {
            let expected = if idx == deltas.deltas.len() - 1 {
                snapshot | last_flag
            } else {
                snapshot
            };
            assert_eq!(
                delta.flags, expected,
                "delta at index {idx} has wrong flags: got {:#010b}, expected {expected:#010b}",
                delta.flags,
            );
        }
    }

    #[rstest]
    #[case::empty_book(vec![], vec![], 1)]
    #[case::bids_only(vec![("100.0", "1.0")], vec![], 2)]
    #[case::asks_only(vec![], vec![("101.0", "2.0")], 2)]
    fn test_parse_orderbook_snapshot_flag_shapes(
        #[case] bids: Vec<(&str, &str)>,
        #[case] asks: Vec<(&str, &str)>,
        #[case] expected_len: usize,
    ) {
        use crate::websocket::messages::DydxPriceLevel;
        let contents = DydxOrderbookSnapshotContents {
            bids: if bids.is_empty() {
                None
            } else {
                Some(
                    bids.into_iter()
                        .map(|(p, s)| DydxPriceLevel {
                            price: p.to_string(),
                            size: s.to_string(),
                        })
                        .collect(),
                )
            },
            asks: if asks.is_empty() {
                None
            } else {
                Some(
                    asks.into_iter()
                        .map(|(p, s)| DydxPriceLevel {
                            price: p.to_string(),
                            size: s.to_string(),
                        })
                        .collect(),
                )
            },
        };
        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_orderbook_snapshot(&instrument_id, &contents, 2, 8, ts_init)
            .expect("Failed to parse orderbook snapshot");

        let snapshot = RecordFlag::F_SNAPSHOT as u8;
        let last_flag = RecordFlag::F_LAST as u8;

        assert_eq!(deltas.deltas.len(), expected_len);

        if expected_len == 1 {
            // Empty book: Clear alone must carry F_SNAPSHOT | F_LAST so buffered
            // subscribers flush when the book is empty.
            assert_eq!(deltas.deltas[0].action, BookAction::Clear);
            assert_eq!(deltas.deltas[0].flags, snapshot | last_flag);
        } else {
            // Non-empty: Clear carries F_SNAPSHOT only; terminator carries both.
            assert_eq!(deltas.deltas[0].flags, snapshot);
            let terminator = deltas.deltas.last().unwrap();
            assert_eq!(terminator.flags, snapshot | last_flag);
        }
    }

    #[rstest]
    fn test_parse_orderbook_deltas_update() {
        let json = load_json_fixture("ws_orderbook_update.json");
        let contents: DydxOrderbookContents = serde_json::from_value(json["contents"].clone())
            .expect("Failed to parse orderbook update contents");

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_orderbook_deltas(&instrument_id, &contents, 2, 8, ts_init)
            .expect("Failed to parse orderbook deltas");

        // 2 bids + 2 asks = 4 deltas
        assert_eq!(deltas.deltas.len(), 4);

        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[0].order.price.to_string(), "43240.00");

        // First ask with size 0.0 should be a Delete
        assert_eq!(deltas.deltas[2].action, BookAction::Delete);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[2].order.price.to_string(), "43250.00");

        assert_eq!(deltas.deltas[3].action, BookAction::Update);
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_trade_ticks_ws() {
        let json = load_json_fixture("ws_trades_subscribed.json");
        let contents: DydxTradeContents = serde_json::from_value(json["contents"].clone())
            .expect("Failed to parse trade contents");

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let ticks = parse_trade_ticks(instrument_id, &instrument, &contents, ts_init)
            .expect("Failed to parse trade ticks");

        assert_eq!(ticks.len(), 1);
        if let Data::Trade(tick) = &ticks[0] {
            assert_eq!(tick.instrument_id, instrument_id);
            assert_eq!(tick.price.to_string(), "43250.00");
            assert_eq!(tick.size.to_string(), "0.50000000");
            assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
            assert_eq!(tick.trade_id.to_string(), "trade-001");
        } else {
            panic!("Expected Trade data");
        }
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn test_parse_candle_bar_timestamp_on_close(#[case] timestamp_on_close: bool) {
        let json = load_json_fixture("ws_candles_subscribed.json");
        let candles_value = &json["contents"]["candles"];
        let candles: Vec<DydxCandle> =
            serde_json::from_value(candles_value.clone()).expect("Failed to parse candle array");

        let instrument = create_test_instrument();
        let bar_type = BarType::from_str("BTC-USD-PERP.DYDX-1-MINUTE-LAST-EXTERNAL")
            .expect("Failed to parse bar type");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let bar = parse_candle_bar(
            bar_type,
            &instrument,
            &candles[0],
            timestamp_on_close,
            ts_init,
        )
        .expect("Failed to parse candle bar");

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open.to_string(), "43100.00");
        assert_eq!(bar.high.to_string(), "43500.00");
        assert_eq!(bar.low.to_string(), "43000.00");
        assert_eq!(bar.close.to_string(), "43400.00");
        assert_eq!(bar.volume.to_string(), "12.34500000");

        // 2024-01-01T00:00:00.000Z = 1_704_067_200_000_000_000 ns
        let started_at_ns = 1_704_067_200_000_000_000u64;
        let one_min_ns = 60_000_000_000u64;

        if timestamp_on_close {
            assert_eq!(bar.ts_event.as_u64(), started_at_ns + one_min_ns);
        } else {
            assert_eq!(bar.ts_event.as_u64(), started_at_ns);
        }
    }

    #[rstest]
    fn test_deserialize_market_trading_update_with_status() {
        let json = load_json_fixture("ws_markets_status_update.json");
        let contents: super::super::messages::DydxMarketsContents =
            serde_json::from_value(json["contents"].clone())
                .expect("Failed to deserialize markets contents");

        let trading = contents.trading.expect("Expected trading data");
        assert_eq!(trading.len(), 2);

        let btc = trading.get("BTC-USD").expect("Expected BTC-USD");
        assert_eq!(btc.status, Some(DydxMarketStatus::Paused));
        assert_eq!(btc.next_funding_rate, Some("0.0001".to_string()));

        let eth = trading.get("ETH-USD").expect("Expected ETH-USD");
        assert_eq!(eth.status, Some(DydxMarketStatus::Active));
    }

    #[rstest]
    #[case("ACTIVE", DydxMarketStatus::Active)]
    #[case("PAUSED", DydxMarketStatus::Paused)]
    #[case("CANCEL_ONLY", DydxMarketStatus::CancelOnly)]
    #[case("POST_ONLY", DydxMarketStatus::PostOnly)]
    #[case("INITIALIZING", DydxMarketStatus::Initializing)]
    #[case("FINAL_SETTLEMENT", DydxMarketStatus::FinalSettlement)]
    fn test_deserialize_market_status_variants(
        #[case] status_str: &str,
        #[case] expected: DydxMarketStatus,
    ) {
        let json_str = format!(r#"{{"status": "{status_str}"}}"#);
        let update: super::super::messages::DydxMarketTradingUpdate =
            serde_json::from_str(&json_str).expect("Failed to deserialize");
        assert_eq!(update.status, Some(expected));
    }

    #[rstest]
    fn test_deserialize_market_trading_update_without_status() {
        let json_str = r#"{"nextFundingRate": "0.0001"}"#;
        let update: super::super::messages::DydxMarketTradingUpdate =
            serde_json::from_str(json_str).expect("Failed to deserialize");
        assert_eq!(update.status, None);
        assert_eq!(update.next_funding_rate, Some("0.0001".to_string()));
    }
}

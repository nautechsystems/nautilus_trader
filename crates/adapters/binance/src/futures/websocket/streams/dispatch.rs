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

//! User data stream dispatch for the Binance Futures adapter.
//!
//! Translates WebSocket stream messages into either proper order events (for
//! tracked orders submitted through this client) or execution reports (for
//! external / untracked orders). Exchange-generated fills (liquidation, ADL,
//! settlement) are routed through the reports path regardless of tracking.

use std::sync::Arc;

use anyhow::Context;
use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{AtomicSet, UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::LiquiditySide,
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{
    messages::{
        BinanceExecutionType, BinanceFuturesAlgoUpdateMsg, BinanceFuturesOrderUpdateMsg,
        BinanceFuturesTradeLiteMsg, BinanceFuturesWsStreamsMessage, OrderUpdateData,
    },
    parse_exec::{
        decode_algo_client_id, decode_order_client_id, parse_futures_account_update,
        parse_futures_algo_update_to_order_status, parse_futures_order_update_to_fill,
        parse_futures_order_update_to_order_status,
    },
};
use crate::{
    common::{
        consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        dispatch::{OrderIdentity, WsDispatchState, ensure_accepted_emitted},
        encoder::decode_client_order_id,
        enums::{BinancePositionSide, BinanceProductType},
        parse::{
            parse_millis_or_init, parse_price_at_precision, parse_quantity_at_precision,
            parse_required_decimal, parse_required_price_at_precision,
            parse_required_quantity_at_precision, price_at_precision, quantity_at_precision,
        },
        symbol::format_instrument_id,
    },
    futures::{
        conversions::normalize_futures_asset,
        http::client::{BinanceFuturesHttpClient, BinanceFuturesInstrument},
    },
};

/// Shared state required by the user data stream dispatch task.
pub(crate) struct DispatchCtx {
    pub emitter: ExecutionEventEmitter,
    pub http_client: BinanceFuturesHttpClient,
    pub account_id: AccountId,
    pub product_type: BinanceProductType,
    pub clock: &'static AtomicTime,
    pub dispatch_state: Arc<WsDispatchState>,
    pub triggered_algo_ids: Arc<AtomicSet<ClientOrderId>>,
    pub algo_client_ids: Arc<AtomicSet<ClientOrderId>>,
    pub use_position_ids: bool,
    pub default_taker_fee: Decimal,
    pub bnfcr_currency: Currency,
    pub treat_expired_as_canceled: bool,
    pub use_trade_lite: bool,
    pub seen_trade_ids: Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
    pub cancellation_token: CancellationToken,
}

pub(crate) async fn run_user_stream_dispatch<S, F>(
    stream: S,
    ctx: Arc<DispatchCtx>,
    recovery_tx: tokio::sync::mpsc::UnboundedSender<()>,
    dispatch_fn: F,
) where
    S: Stream<Item = BinanceFuturesWsStreamsMessage> + Send + 'static,
    F: Fn(BinanceFuturesWsStreamsMessage, &DispatchCtx, &tokio::sync::mpsc::UnboundedSender<()>)
        + Send
        + Sync
        + 'static,
{
    let cancel = ctx.cancellation_token.clone();

    pin_mut!(stream);

    loop {
        tokio::select! {
            msg = stream.next() => {
                // Break on stream end so the task exits once the WebSocket
                // client has drained its out_rx queue. The recovery path
                // relies on this to flush events queued on the old stream
                // before the new dispatcher takes over.
                match msg {
                    Some(message) => dispatch_fn(message, ctx.as_ref(), &recovery_tx),
                    None => {
                        log::debug!("WS dispatch stream ended");
                        break;
                    }
                }
            }
            () = cancel.cancelled() => {
                log::debug!("WS dispatch task cancelled");
                break;
            }
        }
    }
}

/// Adapter between [`DispatchCtx`] and the free-function [`dispatch_ws_message`].
pub(crate) fn dispatch_user_stream_message(
    message: BinanceFuturesWsStreamsMessage,
    ctx: &DispatchCtx,
    recovery_tx: &tokio::sync::mpsc::UnboundedSender<()>,
) {
    dispatch_ws_message(
        message,
        &ctx.emitter,
        &ctx.http_client,
        ctx.account_id,
        ctx.product_type,
        ctx.clock,
        &ctx.dispatch_state,
        &ctx.triggered_algo_ids,
        &ctx.algo_client_ids,
        ctx.use_position_ids,
        ctx.default_taker_fee,
        ctx.bnfcr_currency,
        ctx.treat_expired_as_canceled,
        ctx.use_trade_lite,
        &ctx.seen_trade_ids,
        recovery_tx,
    );
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn dispatch_ws_message(
    msg: BinanceFuturesWsStreamsMessage,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    triggered_algo_ids: &Arc<AtomicSet<ClientOrderId>>,
    algo_client_ids: &Arc<AtomicSet<ClientOrderId>>,
    use_position_ids: bool,
    default_taker_fee: Decimal,
    bnfcr_currency: Currency,
    treat_expired_as_canceled: bool,
    use_trade_lite: bool,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
    recovery_tx: &tokio::sync::mpsc::UnboundedSender<()>,
) {
    match msg {
        BinanceFuturesWsStreamsMessage::OrderUpdate(update) => {
            dispatch_order_update(
                &update,
                emitter,
                http_client,
                account_id,
                product_type,
                clock,
                dispatch_state,
                use_position_ids,
                default_taker_fee,
                bnfcr_currency,
                treat_expired_as_canceled,
                use_trade_lite,
                seen_trade_ids,
            );
        }
        BinanceFuturesWsStreamsMessage::TradeLite(msg) => {
            if use_trade_lite {
                dispatch_trade_lite(
                    &msg,
                    emitter,
                    http_client,
                    account_id,
                    product_type,
                    clock,
                    dispatch_state,
                );
            }
        }
        BinanceFuturesWsStreamsMessage::AlgoUpdate(update) => {
            dispatch_algo_update(
                &update,
                emitter,
                http_client,
                account_id,
                product_type,
                clock,
                dispatch_state,
                triggered_algo_ids,
                algo_client_ids,
                use_position_ids,
            );
        }
        BinanceFuturesWsStreamsMessage::AccountUpdate(update) => {
            let ts_init = clock.get_time_ns();

            if let Some(state) =
                parse_futures_account_update(&update, account_id, bnfcr_currency, ts_init)
            {
                emitter.send_account_state(state);
            }
        }
        BinanceFuturesWsStreamsMessage::MarginCall(mc) => {
            log::warn!(
                "Margin call: cross_wallet_balance={}, positions_at_risk={}",
                mc.cross_wallet_balance,
                mc.positions.len()
            );
        }
        BinanceFuturesWsStreamsMessage::AccountConfigUpdate(cfg) => {
            if let Some(ref lc) = cfg.leverage_config {
                log::info!(
                    "Account config update: symbol={}, leverage={}",
                    lc.symbol,
                    lc.leverage
                );
            }
        }
        BinanceFuturesWsStreamsMessage::ListenKeyExpired => {
            log::warn!("Listen key expired, triggering recovery");

            if recovery_tx.send(()).is_err() {
                log::warn!("Recovery channel closed, cannot trigger listen key rotation");
            }
        }
        BinanceFuturesWsStreamsMessage::Reconnected => {
            // A transport-level reconnect (not a listenKey expiry) still loses
            // any events that arrived during the outage. Trigger recovery to
            // rotate the key and replay the current venue state.
            log::warn!("User data stream reconnected, triggering recovery");

            if recovery_tx.send(()).is_err() {
                log::warn!("Recovery channel closed, cannot trigger recovery");
            }
        }
        BinanceFuturesWsStreamsMessage::Error(err) => {
            log::warn!(
                "User data stream WebSocket error: code={}, msg={}",
                err.code,
                err.msg
            );
        }
        // Market data messages ignored by execution client
        BinanceFuturesWsStreamsMessage::AggTrade(_)
        | BinanceFuturesWsStreamsMessage::Trade(_)
        | BinanceFuturesWsStreamsMessage::BookTicker(_)
        | BinanceFuturesWsStreamsMessage::DepthUpdate(_)
        | BinanceFuturesWsStreamsMessage::MarkPrice(_)
        | BinanceFuturesWsStreamsMessage::Kline(_)
        | BinanceFuturesWsStreamsMessage::ForceOrder(_)
        | BinanceFuturesWsStreamsMessage::Ticker(_) => {}
    }
}

/// Dispatches a Futures order update with tracked/untracked routing.
///
/// Tracked orders produce proper order events. Untracked orders fall back
/// to execution reports for reconciliation.
#[expect(clippy::too_many_arguments)]
pub(crate) fn dispatch_order_update(
    msg: &BinanceFuturesOrderUpdateMsg,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    use_position_ids: bool,
    default_taker_fee: Decimal,
    bnfcr_currency: Currency,
    treat_expired_as_canceled: bool,
    use_trade_lite: bool,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let symbol_ustr = order.symbol;
    let ts_init = clock.get_time_ns();
    let ts_event =
        parse_millis_or_init(msg.event_time, "Futures order dispatch event time", ts_init);

    let Some((cached_instrument, price_precision, size_precision)) =
        resolve_instrument_metadata(http_client, symbol_ustr, product_type, "order update")
    else {
        return;
    };
    let instrument_id = cached_instrument.id();

    let client_order_id = match decode_order_client_id(order) {
        Ok(client_order_id) => client_order_id,
        Err(e) => {
            log::warn!("Skipping Futures order update with invalid client order ID: {e}");
            return;
        }
    };
    let venue_position_id =
        match make_venue_position_id(use_position_ids, instrument_id, Some(order.position_side)) {
            Ok(venue_position_id) => venue_position_id,
            Err(e) => {
                log::warn!("Skipping Futures order update with invalid position side: {e}");
                return;
            }
        };

    // Exchange-generated orders (liquidation/ADL/settlement) are routed through
    // reconciliation reports regardless of tracked/untracked state, because
    // they have no locally submitted identity
    if order.is_exchange_generated() {
        let is_linear = matches!(cached_instrument, BinanceFuturesInstrument::UsdM(_));
        let quote_currency = cached_instrument.quote_currency();

        let taker_fee = if is_linear {
            Some(default_taker_fee)
        } else {
            None
        };

        dispatch_exchange_generated_fill(
            msg,
            emitter,
            instrument_id,
            price_precision,
            size_precision,
            account_id,
            ts_init,
            taker_fee,
            quote_currency,
            bnfcr_currency,
            venue_position_id,
            seen_trade_ids,
        );
        return;
    }

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());

    if let Some(identity) = identity {
        let venue_order_id = VenueOrderId::new(order.order_id.to_string());
        if dispatch_state.promote_algo_order_id(client_order_id, venue_order_id) == Some(true) {
            emit_venue_order_id_update(
                &identity,
                client_order_id,
                venue_order_id,
                account_id,
                ts_event,
                ts_init,
                emitter,
            );
        }

        match order.execution_type {
            BinanceExecutionType::New => {
                if dispatch_state.has_filled(&client_order_id) {
                    log::debug!("Skipping late New for filled order {client_order_id}");
                    return;
                }

                if dispatch_state.has_emitted_accepted(&client_order_id) {
                    log::debug!("Skipping duplicate Accepted for {client_order_id}");
                } else {
                    dispatch_state.insert_accepted(client_order_id);
                    let accepted = OrderAccepted::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        venue_order_id,
                        account_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                    );
                    emitter.send_order_event(OrderEventAny::Accepted(accepted));
                }

                emit_order_delta_if_changed(
                    order,
                    &identity,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                    emitter,
                    dispatch_state,
                );
            }
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock();
                let is_duplicate = guard.contains(&dedup_key);
                guard.add(dedup_key);
                drop(guard);

                if is_duplicate && !use_trade_lite {
                    log::debug!(
                        "Duplicate trade_id={} for {}, skipping",
                        order.trade_id,
                        order.symbol
                    );
                    return;
                }

                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );

                // Reconcile any venue-side qty/price adjustment before emitting
                // the fill: fast-fill paths can deliver TRADE without a prior NEW.
                emit_order_delta_if_changed(
                    order,
                    &identity,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                    emitter,
                    dispatch_state,
                );

                // When use_trade_lite is on, the TRADE_LITE handler owns the
                // fill emission. This arm still runs so the terminal-state
                // cleanup below fires (it needs `z` from ORDER_TRADE_UPDATE,
                // which TRADE_LITE does not carry).
                if !use_trade_lite && !is_duplicate {
                    match parse_order_fill_event_fields(
                        order,
                        price_precision,
                        size_precision,
                        bnfcr_currency,
                    ) {
                        Ok((last_qty, last_px, commission_currency, commission)) => {
                            let liquidity_side = if order.is_maker {
                                LiquiditySide::Maker
                            } else {
                                LiquiditySide::Taker
                            };

                            let filled = OrderFilled::new(
                                emitter.trader_id(),
                                identity.strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                account_id,
                                TradeId::new(order.trade_id.to_string()),
                                identity.order_side,
                                identity.order_type,
                                last_qty,
                                last_px,
                                commission_currency,
                                liquidity_side,
                                UUID4::new(),
                                ts_event,
                                ts_init,
                                false,
                                venue_position_id,
                                commission,
                                None,
                            );

                            dispatch_state.insert_filled(client_order_id);
                            emitter.send_order_event(OrderEventAny::Filled(filled));
                        }
                        Err(e) => log::error!("Failed to parse order fill event: {e}"),
                    }
                }

                match parse_order_terminal_quantities(order) {
                    Ok((orig_qty, cum_qty)) => {
                        if orig_qty <= cum_qty {
                            dispatch_state.cleanup_terminal(client_order_id);
                        }
                    }
                    Err(e) => log::error!("Failed to parse order terminal quantities: {e}"),
                }
            }
            BinanceExecutionType::Canceled => {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );
                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    None,
                );
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            }
            BinanceExecutionType::Expired => {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );
                dispatch_state.cleanup_terminal(client_order_id);

                if treat_expired_as_canceled {
                    let canceled = OrderCanceled::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                        None,
                    );
                    emitter.send_order_event(OrderEventAny::Canceled(canceled));
                } else {
                    let expired = OrderExpired::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::Expired(expired));
                }
            }
            BinanceExecutionType::Amendment => {
                let quantity = match parse_required_quantity_at_precision(
                    &order.original_qty,
                    size_precision,
                    "original_qty",
                ) {
                    Ok(quantity) => quantity,
                    Err(e) => {
                        log::error!("Failed to parse amendment quantity: {e}");
                        return;
                    }
                };
                let price = match parse_required_price_at_precision(
                    &order.original_price,
                    price_precision,
                    "original_price",
                ) {
                    Ok(price) => price,
                    Err(e) => {
                        log::error!("Failed to parse amendment price: {e}");
                        return;
                    }
                };

                let updated = OrderUpdated::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    quantity,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    Some(price),
                    None,
                    None,
                    false, // is_quote_quantity
                );
                emitter.send_order_event(OrderEventAny::Updated(updated));
            }
            BinanceExecutionType::Calculated => {
                log::warn!(
                    "CALCULATED for non-exchange-generated order: symbol={}, client_order_id={}",
                    order.symbol,
                    order.client_order_id,
                );
            }
        }
    } else {
        // Untracked: fall back to reports for reconciliation.
        match order.execution_type {
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock();
                let is_duplicate = guard.contains(&dedup_key);
                guard.add(dedup_key);
                drop(guard);

                if is_duplicate {
                    log::debug!(
                        "Duplicate trade_id={} for {}, skipping",
                        order.trade_id,
                        order.symbol
                    );
                    return;
                }

                let fill = match parse_futures_order_update_to_fill(
                    msg,
                    account_id,
                    instrument_id,
                    price_precision,
                    size_precision,
                    None,
                    None,
                    bnfcr_currency,
                    venue_position_id,
                    ts_init,
                ) {
                    Ok(fill) => Some(fill),
                    Err(e) => {
                        log::error!("Failed to parse fill report: {e}");
                        None
                    }
                };

                let status = match parse_futures_order_update_to_order_status(
                    msg,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(status) => Some(with_venue_position_id(status, venue_position_id)),
                    Err(e) => {
                        log::error!("Failed to parse order status report: {e}");
                        None
                    }
                };

                emit_bundled_or_individual(emitter, status, fill);
            }
            BinanceExecutionType::New
            | BinanceExecutionType::Canceled
            | BinanceExecutionType::Expired
            | BinanceExecutionType::Amendment => {
                match parse_futures_order_update_to_order_status(
                    msg,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(status) => emitter.send_order_status_report(with_venue_position_id(
                        status,
                        venue_position_id,
                    )),
                    Err(e) => log::error!("Failed to parse order status report: {e}"),
                }
            }
            BinanceExecutionType::Calculated => {
                log::warn!(
                    "CALCULATED for non-exchange-generated order: symbol={}, client_order_id={}",
                    order.symbol,
                    order.client_order_id,
                );
            }
        }
    }
}

fn parse_order_fill_event_fields(
    order: &OrderUpdateData,
    price_precision: u8,
    size_precision: u8,
    bnfcr_currency: Currency,
) -> anyhow::Result<(Quantity, Price, Currency, Option<Money>)> {
    let last_qty = parse_required_quantity_at_precision(
        &order.last_filled_qty,
        size_precision,
        "last_filled_qty",
    )?;
    let last_px = parse_required_price_at_precision(
        &order.last_filled_price,
        price_precision,
        "last_filled_price",
    )?;
    let (commission_currency, commission) = match parse_order_commission(order, bnfcr_currency) {
        Ok(commission) => commission,
        Err(e) => {
            log::error!("Failed to parse order commission: {e}");
            (order_commission_currency(order, bnfcr_currency), None)
        }
    };

    Ok((last_qty, last_px, commission_currency, commission))
}

fn parse_order_commission(
    order: &OrderUpdateData,
    bnfcr_currency: Currency,
) -> anyhow::Result<(Currency, Option<Money>)> {
    let currency = order_commission_currency(order, bnfcr_currency);

    let Some(raw_commission) = order.commission.as_deref() else {
        return Ok((currency, None));
    };

    let amount = parse_required_decimal(raw_commission, "commission")?;
    let commission = Money::from_decimal(amount, currency)
        .map_err(|e| anyhow::anyhow!("invalid commission='{raw_commission}': {e}"))?;

    Ok((currency, Some(commission)))
}

fn order_commission_currency(order: &OrderUpdateData, bnfcr_currency: Currency) -> Currency {
    order
        .commission_asset
        .as_ref()
        .map_or(bnfcr_currency, |asset| {
            normalize_futures_asset(asset.as_str(), bnfcr_currency)
        })
}

fn parse_order_terminal_quantities(order: &OrderUpdateData) -> anyhow::Result<(Decimal, Decimal)> {
    let original_qty = parse_required_decimal(&order.original_qty, "original_qty")?;
    let cumulative_filled_qty =
        parse_required_decimal(&order.cumulative_filled_qty, "cumulative_filled_qty")?;

    Ok((original_qty, cumulative_filled_qty))
}

/// Emits an `OrderUpdated` if the venue's reported quantity or price differs
/// from the submitted identity, then refreshes the identity to suppress further
/// emissions on subsequent messages for the same order.
#[expect(clippy::too_many_arguments)]
fn emit_order_delta_if_changed(
    order: &OrderUpdateData,
    identity: &OrderIdentity,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
) {
    let Some(submitted_qty) = quantity_at_precision(identity.quantity, size_precision) else {
        return;
    };
    let venue_qty = parse_quantity_at_precision(&order.original_qty, size_precision);
    let qty_changed = matches!(venue_qty, Some(q) if q != submitted_qty);

    let updated_price = identity.price.and_then(|submitted_price| {
        let venue_price = parse_price_at_precision(&order.original_price, price_precision)?;
        let submitted = price_at_precision(submitted_price, price_precision)?;
        (venue_price != submitted).then_some(venue_price)
    });

    if !qty_changed && updated_price.is_none() {
        return;
    }

    let trigger_price =
        parse_optional_positive_price_at_precision(&order.stop_price, price_precision);
    let event_price = updated_price.or_else(|| {
        identity
            .price
            .and_then(|p| price_at_precision(p, price_precision))
    });
    let event_qty = if qty_changed {
        venue_qty.expect("qty_changed implies venue_qty is Some")
    } else {
        submitted_qty
    };
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        event_qty,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(venue_order_id),
        Some(account_id),
        event_price,
        trigger_price,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));

    refresh_identity(
        dispatch_state,
        client_order_id,
        qty_changed.then(|| venue_qty.unwrap()),
        updated_price,
    );
}

fn parse_optional_positive_price_at_precision(raw: &str, precision: u8) -> Option<Price> {
    let decimal = parse_required_decimal(raw, "optional_price").ok()?;
    if decimal <= Decimal::ZERO {
        return None;
    }

    Price::from_decimal_dp(decimal, precision).ok()
}

/// TRADE_LITE variant of [`emit_order_delta_if_changed`]. TRADE_LITE messages
/// carry `q` and `p` only (no stop price), so this detects venue-side qty
/// deltas (reduce-only auto-reduction) and price deltas (priceMatch) before
/// the fill.
#[expect(clippy::too_many_arguments)]
fn emit_trade_lite_delta_if_changed(
    msg: &BinanceFuturesTradeLiteMsg,
    identity: &OrderIdentity,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
) {
    let Some(submitted_qty) = quantity_at_precision(identity.quantity, size_precision) else {
        return;
    };
    let venue_qty = parse_quantity_at_precision(&msg.original_qty, size_precision);
    let qty_changed = matches!(venue_qty, Some(q) if q != submitted_qty);

    let updated_price = identity.price.and_then(|submitted_price| {
        let venue_price = parse_price_at_precision(&msg.original_price, price_precision)?;
        let submitted = price_at_precision(submitted_price, price_precision)?;
        (venue_price != submitted).then_some(venue_price)
    });

    if !qty_changed && updated_price.is_none() {
        return;
    }

    let event_price = updated_price.or_else(|| {
        identity
            .price
            .and_then(|p| price_at_precision(p, price_precision))
    });
    let event_qty = if qty_changed {
        venue_qty.expect("qty_changed implies venue_qty is Some")
    } else {
        submitted_qty
    };
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        event_qty,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(venue_order_id),
        Some(account_id),
        event_price,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));

    refresh_identity(
        dispatch_state,
        client_order_id,
        qty_changed.then(|| venue_qty.unwrap()),
        updated_price,
    );
}

fn refresh_identity(
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    new_quantity: Option<Quantity>,
    new_price: Option<Price>,
) {
    if let Some(mut entry) = dispatch_state.order_identities.get_mut(&client_order_id) {
        if let Some(qty) = new_quantity {
            entry.quantity = qty;
        }

        if let Some(price) = new_price {
            entry.price = Some(price);
        }
    }
}

/// Dispatches a TRADE_LITE fill.
///
/// TRADE_LITE carries the subset of fields needed to emit `OrderFilled`:
/// no commission, position side, or reduce-only flag. Tracked orders emit
/// `OrderFilled`; untracked orders are skipped (the matching full
/// ORDER_TRADE_UPDATE will provide a proper reconciliation report).
pub(crate) fn dispatch_trade_lite(
    msg: &BinanceFuturesTradeLiteMsg,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let symbol_ustr = msg.symbol;
    let ts_init = clock.get_time_ns();
    let ts_event = parse_millis_or_init(msg.event_time, "Futures TRADE_LITE event time", ts_init);

    let Some((cached_instrument, price_precision, size_precision)) =
        resolve_instrument_metadata(http_client, symbol_ustr, product_type, "TRADE_LITE")
    else {
        return;
    };
    let instrument_id = cached_instrument.id();

    let client_order_id =
        match decode_client_order_id(&msg.client_order_id, BINANCE_NAUTILUS_FUTURES_BROKER_ID) {
            Ok(client_order_id) => client_order_id,
            Err(e) => {
                log::warn!("Skipping Futures TRADE_LITE with invalid client order ID: {e}");
                return;
            }
        };

    let Some(identity) = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone())
    else {
        log::debug!("TRADE_LITE for untracked order {client_order_id}, skipping");
        return;
    };

    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    if dispatch_state.promote_algo_order_id(client_order_id, venue_order_id) == Some(true) {
        emit_venue_order_id_update(
            &identity,
            client_order_id,
            venue_order_id,
            account_id,
            ts_event,
            ts_init,
            emitter,
        );
    }

    ensure_accepted_emitted(
        client_order_id,
        account_id,
        venue_order_id,
        &identity,
        emitter,
        dispatch_state,
        ts_init,
    );

    // Reconcile venue-side qty/price deltas (reduce-only auto-reduction or
    // priceMatch) before the fill: TRADE_LITE can arrive without a prior NEW
    // for fast fills.
    emit_trade_lite_delta_if_changed(
        msg,
        &identity,
        client_order_id,
        venue_order_id,
        account_id,
        price_precision,
        size_precision,
        ts_event,
        ts_init,
        emitter,
        dispatch_state,
    );

    let (last_qty, last_px) =
        match parse_trade_lite_fill_event_fields(msg, price_precision, size_precision) {
            Ok(fields) => fields,
            Err(e) => {
                log::error!("Failed to parse TRADE_LITE fill event: {e}");
                return;
            }
        };

    let liquidity_side = if msg.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    // TRADE_LITE does not carry commission_asset, so fall back to the
    // instrument's quote currency (COIN-M and non-USDT USD-M symbols).
    let quote_currency = cached_instrument.quote_currency();

    let filled = OrderFilled::new(
        emitter.trader_id(),
        identity.strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        TradeId::new(msg.trade_id.to_string()),
        identity.order_side,
        identity.order_type,
        last_qty,
        last_px,
        quote_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        identity.venue_position_id,
        None,
        None,
    );

    dispatch_state.insert_filled(client_order_id);
    emitter.send_order_event(OrderEventAny::Filled(filled));
}

fn parse_trade_lite_fill_event_fields(
    msg: &BinanceFuturesTradeLiteMsg,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<(Quantity, Price)> {
    let last_qty = parse_required_quantity_at_precision(
        &msg.last_filled_qty,
        size_precision,
        "last_filled_qty",
    )?;
    let last_px = parse_required_price_at_precision(
        &msg.last_filled_price,
        price_precision,
        "last_filled_price",
    )?;

    Ok((last_qty, last_px))
}

pub(crate) fn with_venue_position_id(
    report: OrderStatusReport,
    venue_position_id: Option<PositionId>,
) -> OrderStatusReport {
    match venue_position_id {
        Some(position_id) => report.with_venue_position_id(position_id),
        None => report,
    }
}

/// Derives a venue position ID from the instrument and Binance position side.
///
/// Returns `None` when `use_position_ids` is false or the venue uses one-way mode.
///
/// # Errors
///
/// Returns an error when position IDs are enabled but the position side is missing or unknown.
pub(crate) fn make_venue_position_id(
    use_position_ids: bool,
    instrument_id: InstrumentId,
    position_side: Option<BinancePositionSide>,
) -> anyhow::Result<Option<PositionId>> {
    if !use_position_ids {
        return Ok(None);
    }

    let position_side = position_side.context("missing position_side")?;
    let side = match position_side {
        BinancePositionSide::Long => "LONG",
        BinancePositionSide::Short => "SHORT",
        BinancePositionSide::Both => return Ok(None),
        BinancePositionSide::Unknown => anyhow::bail!("unknown position_side"),
    };
    Ok(Some(PositionId::new(format!("{instrument_id}-{side}"))))
}

/// Dispatches exchange-generated order fills (liquidation, ADL, settlement).
///
/// Bundles the parsed `OrderStatusReport` and `FillReport` into a single
/// `OrderWithFills` send so the engine creates the external order from the
/// status report and applies the real fill (preserving `trade_id` and
/// `commission`) instead of synthesising one. Falls back to whichever report
/// parsed if the other parser fails.
///
/// Skips events with zero fill quantity (pending liquidation notifications).
#[expect(clippy::too_many_arguments)]
pub(crate) fn dispatch_exchange_generated_fill(
    msg: &BinanceFuturesOrderUpdateMsg,
    emitter: &ExecutionEventEmitter,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
    taker_fee: Option<Decimal>,
    quote_currency: Currency,
    bnfcr_currency: Currency,
    venue_position_id: Option<PositionId>,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let last_qty = match parse_exchange_generated_fill_quantity(order) {
        Ok(last_qty) => last_qty,
        Err(e) => {
            log::error!("Failed to parse exchange-generated fill quantity: {e}");
            return;
        }
    };

    let order_kind = if order.is_liquidation() {
        "liquidation"
    } else if order.is_adl() {
        "ADL"
    } else {
        "settlement"
    };

    let Some(last_qty) = last_qty else {
        log::warn!(
            "Exchange-generated {order_kind} pending: symbol={}, client_order_id={}, status={:?}",
            order.symbol,
            order.client_order_id,
            order.order_status,
        );
        return;
    };

    let dedup_key = (order.symbol, order.trade_id);
    let mut guard = seen_trade_ids.lock();
    let is_duplicate = guard.contains(&dedup_key);
    guard.add(dedup_key);
    drop(guard);

    if is_duplicate {
        log::debug!(
            "Duplicate trade_id={} for {}, skipping",
            order.trade_id,
            order.symbol
        );
        return;
    }

    log::warn!(
        "Exchange-generated {order_kind} fill: symbol={}, client_order_id={}, qty={last_qty}, exec_type={:?}",
        order.symbol,
        order.client_order_id,
        order.execution_type,
    );

    let fill = match parse_futures_order_update_to_fill(
        msg,
        account_id,
        instrument_id,
        price_precision,
        size_precision,
        taker_fee,
        Some(quote_currency),
        bnfcr_currency,
        venue_position_id,
        ts_init,
    ) {
        Ok(fill) => Some(fill),
        Err(e) => {
            log::error!("Failed to parse fill report: {e}");
            None
        }
    };

    let status = match parse_futures_order_update_to_order_status(
        msg,
        instrument_id,
        price_precision,
        size_precision,
        account_id,
        false, // Exchange-generated fills are not subject to expired-as-canceled
        ts_init,
    ) {
        Ok(status) => Some(with_venue_position_id(status, venue_position_id)),
        Err(e) => {
            log::error!("Failed to parse order status report: {e}");
            None
        }
    };

    emit_bundled_or_individual(emitter, status, fill);
}

fn parse_exchange_generated_fill_quantity(
    order: &OrderUpdateData,
) -> anyhow::Result<Option<Decimal>> {
    let last_qty = parse_required_decimal(&order.last_filled_qty, "last_filled_qty")?;
    Ok((!last_qty.is_zero()).then_some(last_qty))
}

/// Bundles status + fill into a single `OrderWithFills` send when both parsed,
/// otherwise emits whichever side parsed on its own.
///
/// Sending the fill alone would let the engine bootstrap a synthetic order at
/// `last_qty`, which then closes on the first partial fill and rejects
/// subsequent fills for the same venue order. Sending whichever report parsed
/// instead of dropping both keeps the position in sync when only one parser
/// fails.
fn emit_bundled_or_individual(
    emitter: &ExecutionEventEmitter,
    status: Option<OrderStatusReport>,
    fill: Option<FillReport>,
) {
    match (status, fill) {
        (Some(status), Some(fill)) => emitter.send_order_with_fills(status, vec![fill]),
        (Some(status), None) => emitter.send_order_status_report(status),
        (None, Some(fill)) => emitter.send_fill_report(fill),
        (None, None) => {}
    }
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn dispatch_algo_update(
    msg: &BinanceFuturesAlgoUpdateMsg,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    triggered_algo_ids: &Arc<AtomicSet<ClientOrderId>>,
    algo_client_ids: &Arc<AtomicSet<ClientOrderId>>,
    use_position_ids: bool,
) {
    use crate::common::enums::BinanceAlgoStatus;

    let algo_data = &msg.algo_order;
    let ts_init = clock.get_time_ns();
    let ts_event =
        parse_millis_or_init(msg.event_time, "Futures algo dispatch event time", ts_init);
    let client_order_id = match decode_algo_client_id(algo_data) {
        Ok(client_order_id) => client_order_id,
        Err(e) => {
            log::warn!("Skipping Futures algo update with invalid client order ID: {e}");
            return;
        }
    };

    let symbol_ustr = algo_data.symbol;
    let Some((instrument, price_precision, size_precision)) =
        resolve_instrument_metadata(http_client, symbol_ustr, product_type, "algo update")
    else {
        return;
    };
    let instrument_id = instrument.id();

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());
    let venue_position_id = if identity.is_none() {
        match make_venue_position_id(
            use_position_ids,
            instrument_id,
            Some(algo_data.position_side),
        ) {
            Ok(venue_position_id) => venue_position_id,
            Err(e) => {
                log::warn!(
                    "Skipping untracked Futures algo update with invalid position side: {e}"
                );
                return;
            }
        }
    } else {
        None
    };

    match algo_data.algo_status {
        BinanceAlgoStatus::New => {
            algo_client_ids.insert(client_order_id);
            let venue_order_id = VenueOrderId::new(algo_data.algo_id.to_string());
            dispatch_state.insert_algo_order_id(client_order_id, venue_order_id);

            if let Some(identity) = identity
                && !dispatch_state.has_emitted_accepted(&client_order_id)
            {
                dispatch_state.insert_accepted(client_order_id);
                let accepted = OrderAccepted::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                );
                emitter.send_order_event(OrderEventAny::Accepted(accepted));
            }
        }
        BinanceAlgoStatus::Triggering => {
            log::debug!(
                "Algo order triggering: client_order_id={}, algo_id={}, symbol={}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.symbol
            );
        }
        BinanceAlgoStatus::Triggered => {
            triggered_algo_ids.insert(client_order_id);

            if let Some(actual_order_id) = algo_data
                .actual_order_id
                .as_ref()
                .filter(|id| !id.is_empty())
                .map(|id| VenueOrderId::new(id.clone()))
            {
                let should_emit = dispatch_state
                    .promote_algo_order_id(client_order_id, actual_order_id)
                    .unwrap_or(true);

                if should_emit && let Some(identity) = identity {
                    emit_venue_order_id_update(
                        &identity,
                        client_order_id,
                        actual_order_id,
                        account_id,
                        ts_event,
                        ts_init,
                        emitter,
                    );
                }
            }

            log::debug!(
                "Algo order triggered: client_order_id={}, algo_id={}, actual_order_id={:?}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.actual_order_id
            );
        }
        BinanceAlgoStatus::Canceled | BinanceAlgoStatus::Expired => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);
            dispatch_state.cleanup_terminal(client_order_id);

            if let Some(identity) = identity {
                let venue_order_id = algo_data
                    .actual_order_id
                    .as_ref()
                    .filter(|id| !id.is_empty())
                    .map(|id| VenueOrderId::new(id.clone()));

                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    venue_order_id,
                    Some(account_id),
                    None,
                );
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            } else {
                match parse_futures_algo_update_to_order_status(
                    algo_data,
                    msg.event_time,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    ts_init,
                ) {
                    Ok(Some(report)) => emitter.send_order_status_report(with_venue_position_id(
                        report,
                        venue_position_id,
                    )),
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse algo order status report: {e}"),
                }
            }
        }
        BinanceAlgoStatus::Rejected => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);
            dispatch_state.cleanup_terminal(client_order_id);

            if let Some(identity) = identity {
                emitter.emit_order_rejected_event(
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    "REJECTED",
                    ts_init,
                    false,
                );
            } else {
                match parse_futures_algo_update_to_order_status(
                    algo_data,
                    msg.event_time,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    ts_init,
                ) {
                    Ok(Some(report)) => emitter.send_order_status_report(with_venue_position_id(
                        report,
                        venue_position_id,
                    )),
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse algo order status report: {e}"),
                }
            }
        }
        BinanceAlgoStatus::Finished => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);
            dispatch_state.cleanup_terminal(client_order_id);

            let executed_qty = match algo_data.executed_qty.as_deref() {
                Some(raw) => match parse_required_decimal(raw, "executed_qty") {
                    Ok(executed_qty) => Some(executed_qty),
                    Err(e) => {
                        log::error!("Failed to parse algo executed quantity: {e}");
                        return;
                    }
                },
                None => None,
            };

            if let Some(executed_qty) = executed_qty.filter(|qty| *qty > Decimal::ZERO) {
                log::debug!(
                    "Algo order finished with fills: client_order_id={}, executed_qty={}",
                    algo_data.client_algo_id,
                    executed_qty
                );
            } else {
                log::debug!(
                    "Algo order finished without fills: client_order_id={}",
                    algo_data.client_algo_id
                );
            }
        }
        BinanceAlgoStatus::Unknown => {
            log::warn!(
                "Unknown algo status: client_order_id={}, algo_id={}",
                algo_data.client_algo_id,
                algo_data.algo_id
            );
        }
    }
}

fn resolve_instrument_metadata(
    http_client: &BinanceFuturesHttpClient,
    symbol: ustr::Ustr,
    product_type: BinanceProductType,
    update: &str,
) -> Option<(BinanceFuturesInstrument, u8, u8)> {
    let expected_id = format_instrument_id(&symbol, product_type);
    let Some(instrument) = http_client
        .instruments_cache()
        .get(&symbol)
        .map(|instrument| instrument.value().clone())
    else {
        log::error!("Skipping Futures {update} without instrument metadata for {expected_id}");
        return None;
    };
    let instrument_id = instrument.id();
    if instrument_id != expected_id {
        log::error!(
            "Skipping Futures {update} because instrument metadata ID {instrument_id} does not match {expected_id}"
        );
        return None;
    }
    let (price_precision, size_precision) = match instrument.precisions() {
        Ok(precisions) => precisions,
        Err(e) => {
            log::error!("Skipping Futures {update} with invalid metadata for {expected_id}: {e}");
            return None;
        }
    };

    Some((instrument, price_precision, size_precision))
}

fn emit_venue_order_id_update(
    identity: &OrderIdentity,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
) {
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        identity.quantity,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(venue_order_id),
        Some(account_id),
        identity.price,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{AccountType, OrderSide, OrderStatus, OrderType},
        identifiers::{StrategyId, TraderId},
    };
    use rstest::rstest;
    use serde::de::DeserializeOwned;

    use super::*;
    use crate::{
        common::{
            dispatch::OrderIdentity,
            enums::{
                BinanceAlgoStatus, BinanceContractStatus, BinanceEnvironment, BinanceTradingStatus,
            },
            testing::load_fixture_string,
        },
        futures::http::{
            client::BinanceFuturesHttpClient,
            models::{BinanceFuturesCoinSymbol, BinanceFuturesUsdSymbol},
        },
    };

    #[rstest]
    #[case::long(BinancePositionSide::Long, "ETHUSDT-PERP.BINANCE-LONG")]
    #[case::short(BinancePositionSide::Short, "ETHUSDT-PERP.BINANCE-SHORT")]
    fn test_make_venue_position_id_enabled(
        #[case] side: BinancePositionSide,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(true, instrument_id, Some(side)).unwrap();
        assert_eq!(result, Some(PositionId::from(expected)));
    }

    #[rstest]
    fn test_make_venue_position_id_keeps_one_way_mode_unkeyed() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result =
            make_venue_position_id(true, instrument_id, Some(BinancePositionSide::Both)).unwrap();
        assert_eq!(result, None);
    }

    #[rstest]
    #[case::missing(None, "missing position_side")]
    #[case::unknown(Some(BinancePositionSide::Unknown), "unknown position_side")]
    fn test_make_venue_position_id_rejects_invalid_side(
        #[case] side: Option<BinancePositionSide>,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let error = make_venue_position_id(true, instrument_id, side).unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    fn make_status_report() -> OrderStatusReport {
        use nautilus_model::enums::TimeInForce;
        OrderStatusReport::new(
            AccountId::from("BINANCE-001"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(ClientOrderId::from("O-PARSER-001")),
            VenueOrderId::from("V-PARSER-001"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from(1),
            Quantity::from(1),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
    }

    fn make_fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("BINANCE-001"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            VenueOrderId::from("V-PARSER-001"),
            TradeId::from("T-PARSER-001"),
            OrderSide::Buy,
            Quantity::from(1),
            Price::from("50000.0"),
            Money::new(0.0, Currency::USD()),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-PARSER-001")),
            None,
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
    }

    #[rstest]
    fn test_emit_bundled_when_both_parsed() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);

        emit_bundled_or_individual(
            &emitter,
            Some(make_status_report()),
            Some(make_fill_report()),
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, ref fills)) if fills.len() == 1
        ));
    }

    #[rstest]
    fn test_emit_status_alone_when_fill_parser_fails() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);

        emit_bundled_or_individual(&emitter, Some(make_status_report()), None);

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ExecutionEvent::Report(ExecutionReport::Order(_))
        ));
    }

    #[rstest]
    fn test_emit_fill_alone_when_status_parser_fails() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);

        emit_bundled_or_individual(&emitter, None, Some(make_fill_report()));

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ExecutionEvent::Report(ExecutionReport::Fill(_))
        ));
    }

    #[rstest]
    fn test_emit_nothing_when_both_parsers_fail() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);

        emit_bundled_or_individual(&emitter, None, None);

        let events = collect_events(&mut rx);
        assert!(events.is_empty());
    }

    #[rstest]
    #[case::long(BinancePositionSide::Long)]
    #[case::short(BinancePositionSide::Short)]
    #[case::both(BinancePositionSide::Both)]
    #[case::unknown(BinancePositionSide::Unknown)]
    fn test_make_venue_position_id_disabled(#[case] side: BinancePositionSide) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(false, instrument_id, Some(side)).unwrap();
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_tracked_trade() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Order(OrderEventAny::Filled(fill))
                        if fill.trade_id == TradeId::new("12345678")
                ))
                .count(),
            1
        );
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_untracked_trade() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        // The untracked TRADE path now emits a single bundled OrderWithFills
        // report; the duplicate trade_id is suppressed by seen_trade_ids dedup.
        assert_eq!(events.len(), 1);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, fills))
                        if status.client_order_id == Some(ClientOrderId::from("TEST"))
                            && fills.len() == 1
                            && fills[0].trade_id == TradeId::new("12345678")
                ))
                .count(),
            1
        );
    }

    #[rstest]
    fn test_dispatch_order_update_invalid_client_order_id_emits_nothing() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        msg.order.client_order_id = "x-aHRE4BCj-R".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        assert!(collect_events(&mut rx).is_empty());
        assert!(dispatch_state.order_identities.is_empty());
    }

    #[rstest]
    fn test_dispatch_algo_update_invalid_client_order_id_emits_nothing() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        msg.algo_order.client_algo_id = "x-aHRE4BCj-Tinvalid".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let triggered_algo_ids = Arc::new(AtomicSet::new());
        let algo_client_ids = Arc::new(AtomicSet::new());

        dispatch_algo_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            &triggered_algo_ids,
            &algo_client_ids,
            false,
        );

        assert!(collect_events(&mut rx).is_empty());
        assert!(triggered_algo_ids.is_empty());
        assert!(algo_client_ids.is_empty());
        assert!(dispatch_state.order_identities.is_empty());
    }

    #[rstest]
    fn test_dispatch_algo_update_untracked_canceled_keeps_one_way_mode_unkeyed() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesAlgoUpdateMsg = load_user_data_fixture("algo_update_canceled.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let triggered_algo_ids = Arc::new(AtomicSet::new());
        let algo_client_ids = Arc::new(AtomicSet::new());

        dispatch_algo_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            &triggered_algo_ids,
            &algo_client_ids,
            true,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                assert_eq!(report.venue_position_id, None);
            }
            other => panic!("Expected OrderStatusReport, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_algo_update_triggered_emits_updated_with_actual_order_id() {
        let ts_init = UnixNanos::from(42);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));
        let mut msg: BinanceFuturesAlgoUpdateMsg = load_user_data_fixture("algo_update_new.json");
        let client_order_id = ClientOrderId::from("TEST-ALGO");
        let instrument_id = InstrumentId::from("BNBUSDT-PERP.BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let actual_order_id = VenueOrderId::from("22542179");
        let quantity = Quantity::from("0.01");
        let price = Price::from("750.00");
        msg.algo_order.client_algo_id = client_order_id.to_string();
        msg.algo_order.algo_status = BinanceAlgoStatus::Triggered;
        msg.algo_order.actual_order_id = Some(actual_order_id.to_string());
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            instrument_id,
            Some(price),
            quantity,
        );
        dispatch_state.insert_accepted(client_order_id);
        let triggered_algo_ids = Arc::new(AtomicSet::new());
        let algo_client_ids = Arc::new(AtomicSet::new());

        dispatch_algo_update(
            &msg,
            &emitter,
            &http_client,
            account_id,
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            &triggered_algo_ids,
            &algo_client_ids,
            false,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(triggered_algo_ids.contains(&client_order_id));

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(updated.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(updated.instrument_id, instrument_id);
                assert_eq!(updated.client_order_id, client_order_id);
                assert_eq!(updated.venue_order_id, Some(actual_order_id));
                assert_eq!(updated.account_id, Some(account_id));
                assert_eq!(updated.quantity, quantity);
                assert_eq!(updated.price, Some(price));
                assert_eq!(updated.trigger_price, None);
                assert_eq!(updated.protection_price, None);
                assert_eq!(
                    updated.ts_event,
                    UnixNanos::from_millis(msg.event_time as u64)
                );
                assert_eq!(updated.ts_init, ts_init);
                assert!(!updated.reconciliation);
                assert!(!updated.is_quote_quantity);
                assert_eq!(updated.causation_id, None);
            }
            other => panic!("Expected OrderUpdated, was {other:?}"),
        }
    }

    #[rstest]
    #[case::matching_event_first(true)]
    #[case::algo_triggered_first(false)]
    fn test_dispatch_promotes_algo_id_once_for_event_order(#[case] matching_event_first: bool) {
        let ts_init = UnixNanos::from(42);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));
        let mut algo_msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_new.json");
        let order_msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_new.json");
        let client_order_id = ClientOrderId::from("TEST");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let actual_order_id = VenueOrderId::from(order_msg.order.order_id.to_string());
        algo_msg.algo_order.client_algo_id = client_order_id.to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            instrument_id,
            None,
            Quantity::from("0.001"),
        );
        let triggered_algo_ids = Arc::new(AtomicSet::new());
        let algo_client_ids = Arc::new(AtomicSet::new());
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_algo_update(
            &algo_msg,
            &emitter,
            &http_client,
            account_id,
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            &triggered_algo_ids,
            &algo_client_ids,
            false,
        );
        algo_msg.algo_order.algo_status = BinanceAlgoStatus::Triggered;
        algo_msg.algo_order.actual_order_id = Some(actual_order_id.to_string());
        let dispatch_order = || {
            dispatch_order_update(
                &order_msg,
                &emitter,
                &http_client,
                account_id,
                BinanceProductType::UsdM,
                clock,
                &dispatch_state,
                false,
                Decimal::new(4, 4),
                Currency::USDT(),
                false,
                false,
                &seen_trade_ids,
            );
        };
        let dispatch_algo = || {
            dispatch_algo_update(
                &algo_msg,
                &emitter,
                &http_client,
                account_id,
                BinanceProductType::UsdM,
                clock,
                &dispatch_state,
                &triggered_algo_ids,
                &algo_client_ids,
                false,
            );
        };
        let updated_ts_event = if matching_event_first {
            dispatch_order();
            dispatch_algo();
            UnixNanos::from_millis(order_msg.event_time as u64)
        } else {
            dispatch_algo();
            dispatch_order();
            UnixNanos::from_millis(algo_msg.event_time as u64)
        };

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 2);
        assert!(algo_client_ids.contains(&client_order_id));
        assert!(triggered_algo_ids.contains(&client_order_id));
        assert_eq!(
            dispatch_state.promoted_algo_order_id(&client_order_id),
            Some(actual_order_id),
            "matching-engine ID should replace the Algo Service ID"
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(accepted.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(accepted.instrument_id, instrument_id);
                assert_eq!(accepted.client_order_id, client_order_id);
                assert_eq!(
                    accepted.venue_order_id,
                    VenueOrderId::from(algo_msg.algo_order.algo_id.to_string())
                );
                assert_eq!(accepted.account_id, account_id);
                assert_eq!(
                    accepted.ts_event,
                    UnixNanos::from_millis(algo_msg.event_time as u64)
                );
                assert_eq!(accepted.ts_init, ts_init);
                assert!(!accepted.reconciliation);
                assert_eq!(accepted.causation_id, None);
            }
            other => panic!("Expected OrderAccepted, was {other:?}"),
        }

        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(updated.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(updated.instrument_id, instrument_id);
                assert_eq!(updated.client_order_id, client_order_id);
                assert_eq!(updated.venue_order_id, Some(actual_order_id));
                assert_eq!(updated.account_id, Some(account_id));
                assert_eq!(updated.quantity, Quantity::from("0.001"));
                assert_eq!(updated.price, None);
                assert_eq!(updated.trigger_price, None);
                assert_eq!(updated.protection_price, None);
                assert_eq!(updated.ts_event, updated_ts_event);
                assert_eq!(updated.ts_init, ts_init);
                assert!(!updated.reconciliation);
                assert!(!updated.is_quote_quantity);
                assert_eq!(updated.causation_id, None);
            }
            other => panic!("Expected OrderUpdated, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_exchange_generated_fill() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        // Exchange-generated fills emit a single bundled OrderWithFills report.
        // The duplicate trade_id is suppressed by the seen_trade_ids dedup.
        assert_eq!(events.len(), 1);
        let ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, fills)) = &events[0]
        else {
            panic!(
                "Expected bundled exchange-generated fill, was {:?}",
                events[0]
            );
        };
        let expected_position_id = Some(PositionId::from("BTCUSDT-PERP.BINANCE-LONG"));
        assert_eq!(status.order_status, OrderStatus::Filled);
        assert_eq!(status.venue_position_id, expected_position_id);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, TradeId::new("12345999"));
        assert_eq!(fills[0].venue_position_id, expected_position_id);
    }

    #[rstest]
    fn test_parse_exchange_generated_fill_quantity_rejects_invalid_fill_quantity() {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        msg.order.last_filled_qty = "not-a-number".to_string();

        let result = parse_exchange_generated_fill_quantity(&msg.order);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("last_filled_qty"));
    }

    fn collect_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();

        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        events
    }

    fn create_test_emitter(
        clock: &'static AtomicTime,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("BINANCE-001"),
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn create_test_http_client(clock: &'static AtomicTime) -> BinanceFuturesHttpClient {
        let client = BinanceFuturesHttpClient::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Live,
            clock,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .expect("Test HTTP client should be created");
        client.instruments_cache().insert(
            ustr::Ustr::from("BTCUSDT"),
            usdm_instrument("BTCUSDT", "USDT"),
        );
        client.instruments_cache().insert(
            ustr::Ustr::from("BNBUSDT"),
            usdm_instrument("BNBUSDT", "USDT"),
        );
        client
    }

    fn create_tracked_dispatch_state(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
    ) -> WsDispatchState {
        let dispatch_state = WsDispatchState::default();
        dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("TEST-STRATEGY"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price: None,
                quantity: Quantity::from("1"),
                venue_position_id: None,
            },
        );
        dispatch_state
    }

    fn load_user_data_fixture<T: DeserializeOwned>(filename: &str) -> T {
        let path = format!("futures/user_data_json/{filename}");
        serde_json::from_str(&load_fixture_string(&path))
            .unwrap_or_else(|e| panic!("Failed to parse fixture {path}: {e}"))
    }

    fn build_expired_order_update() -> BinanceFuturesOrderUpdateMsg {
        let json = r#"{
            "e":"ORDER_TRADE_UPDATE","T":1568879465651,"E":1568879465651,
            "o":{
                "s":"BTCUSDT","c":"TEST","S":"BUY","o":"LIMIT","f":"GTC",
                "q":"0.001","p":"7100.50","ap":"0","sp":"0",
                "x":"EXPIRED","X":"EXPIRED","i":8886774,
                "l":"0","z":"0","L":"0","N":"USDT","n":"0",
                "T":1568879465651,"t":0,"b":"0","a":"0","m":false,"R":false,
                "wt":"CONTRACT_PRICE","ot":"LIMIT","ps":"LONG","cp":false,
                "AP":"0","cr":"0","pP":false,"si":0,"ss":0,"rp":"0",
                "V":"EXPIRE_TAKER"
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    fn build_amendment_order_update() -> BinanceFuturesOrderUpdateMsg {
        let json = r#"{
            "e":"ORDER_TRADE_UPDATE","T":1568879465651,"E":1568879465651,
            "o":{
                "s":"BTCUSDT","c":"TEST","S":"BUY","o":"LIMIT","f":"GTC",
                "q":"0.002","p":"7200.00","ap":"0","sp":"0",
                "x":"AMENDMENT","X":"NEW","i":8886774,
                "l":"0","z":"0","L":"0","N":"USDT","n":"0",
                "T":1568879465651,"t":0,"b":"0","a":"0","m":false,"R":false,
                "wt":"CONTRACT_PRICE","ot":"LIMIT","ps":"LONG","cp":false,
                "AP":"0","cr":"0","pP":false,"si":0,"ss":0,"rp":"0",
                "V":"EXPIRE_TAKER"
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    fn build_new_order_update_with_price(price: &str) -> BinanceFuturesOrderUpdateMsg {
        build_new_order_update_with_qty_and_price("0.001", price)
    }

    fn build_new_order_update_with_qty_and_price(
        qty: &str,
        price: &str,
    ) -> BinanceFuturesOrderUpdateMsg {
        let json = format!(
            r#"{{
                "e":"ORDER_TRADE_UPDATE","T":1568879465651,"E":1568879465651,
                "o":{{
                    "s":"BTCUSDT","c":"TEST","S":"BUY","o":"LIMIT","f":"GTC",
                    "q":"{qty}","p":"{price}","ap":"0","sp":"0",
                    "x":"NEW","X":"NEW","i":8886774,
                    "l":"0","z":"0","L":"0","N":"USDT","n":"0",
                    "T":1568879465651,"t":0,"b":"0","a":"0","m":false,"R":false,
                    "wt":"CONTRACT_PRICE","ot":"LIMIT","ps":"LONG","cp":false,
                    "AP":"0","cr":"0","pP":false,"si":0,"ss":0,"rp":"0",
                    "V":"EXPIRE_TAKER"
                }}
            }}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn build_trade_order_update_with_trade_id(
        qty: &str,
        last_qty: &str,
        cum_qty: &str,
        price: &str,
        trade_id: i64,
        order_status: &str,
    ) -> BinanceFuturesOrderUpdateMsg {
        let json = format!(
            r#"{{
                "e":"ORDER_TRADE_UPDATE","T":1568879465651,"E":1568879465651,
                "o":{{
                    "s":"BTCUSDT","c":"TEST","S":"BUY","o":"LIMIT","f":"GTC",
                    "q":"{qty}","p":"{price}","ap":"{price}","sp":"0",
                    "x":"TRADE","X":"{order_status}","i":8886774,
                    "l":"{last_qty}","z":"{cum_qty}","L":"{price}","N":"USDT","n":"0.01",
                    "T":1568879465651,"t":{trade_id},"b":"0","a":"0","m":false,"R":true,
                    "wt":"CONTRACT_PRICE","ot":"LIMIT","ps":"LONG","cp":false,
                    "AP":"0","cr":"0","pP":false,"si":0,"ss":0,"rp":"0",
                    "V":"EXPIRE_TAKER"
                }}
            }}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn create_tracked_state_with_price_and_qty(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        price: Option<Price>,
        quantity: Quantity,
    ) -> WsDispatchState {
        let dispatch_state = WsDispatchState::default();
        dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("TEST-STRATEGY"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price,
                quantity,
                venue_position_id: None,
            },
        );
        dispatch_state
    }

    #[rstest]
    #[case::as_canceled(true)]
    #[case::as_expired(false)]
    fn test_dispatch_order_update_expired_respects_treat_flag(
        #[case] treat_expired_as_canceled: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let msg = build_expired_order_update();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );

        // Pre-seed the accepted flag so ensure_accepted_emitted does not
        // synthesize an OrderAccepted ahead of the terminal event.
        dispatch_state.insert_accepted(ClientOrderId::from("TEST"));
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            treat_expired_as_canceled,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);

        if treat_expired_as_canceled {
            match &events[0] {
                ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
                    assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                    assert_eq!(event.venue_order_id, Some(VenueOrderId::from("8886774")));
                    assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                }
                other => panic!("Expected OrderCanceled, was {other:?}"),
            }
        } else {
            match &events[0] {
                ExecutionEvent::Order(OrderEventAny::Expired(event)) => {
                    assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                    assert_eq!(event.venue_order_id, Some(VenueOrderId::from("8886774")));
                    assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                }
                other => panic!("Expected OrderExpired, was {other:?}"),
            }
        }
    }

    #[rstest]
    fn test_dispatch_order_update_amendment_emits_updated() {
        let clock = get_atomic_clock_realtime();
        let msg = build_amendment_order_update();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.venue_order_id, Some(VenueOrderId::from("8886774")));
                assert_eq!(event.price, Some(Price::new(7200.00, 8)));
                assert_eq!(event.quantity, Quantity::new(0.002, 8));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
            }
            other => panic!("Expected OrderUpdated, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_update_new_with_price_match_divergence_emits_updated() {
        let clock = get_atomic_clock_realtime();

        // Submitted with price 7000 and qty 0.001; venue confirmed qty but
        // adjusted price to 7100.50 (priceMatch).
        let msg = build_new_order_update_with_price("7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7000.0, 8)),
            Quantity::new(0.001, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 2);

        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.price, Some(Price::new(7100.50, 8)));
                assert_eq!(event.quantity, Quantity::new(0.001, 8));
            }
            other => panic!("Expected OrderUpdated for priceMatch divergence, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_update_trade_emits_updated_then_filled_for_reduced_qty() {
        // Fast-fill scenario: TRADE arrives without a prior NEW. The reduce-only
        // order was submitted at 0.005 BTC, venue auto-reduced to 0.001 and filled.
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 3, "expected Accepted, Updated, Filled");
        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.quantity, Quantity::new(0.001, 8));
            }
            other => panic!("Expected OrderUpdated for reduce-only qty delta, was {other:?}"),
        }

        match &events[2] {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => assert_eq!(
                fill.position_id,
                Some(PositionId::from("BTCUSDT-PERP.BINANCE-LONG")),
            ),
            other => panic!("Expected OrderFilled, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_update_trade_rejects_invalid_fill_price() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.last_filled_price = "not-a-number".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        dispatch_state.insert_accepted(client_order_id);
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_)))),
            "invalid fill price must not emit OrderFilled"
        );
    }

    #[rstest]
    fn test_dispatch_order_update_trade_skips_invalid_commission_only() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.commission = Some("not-a-number".to_string());
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        dispatch_state.insert_accepted(client_order_id);
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        let fills: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ExecutionEvent::Order(OrderEventAny::Filled(fill)) => Some(fill),
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].commission, None);
    }

    #[rstest]
    fn test_dispatch_order_update_new_after_algo_accepted_reconciles_reduced_quantity() {
        let ts_init = UnixNanos::from(42);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));

        // Submitted reduce-only at 0.005 BTC; venue auto-reduced to 0.001 (position size).
        let msg = build_new_order_update_with_price("7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            instrument_id,
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );
        dispatch_state.insert_accepted(client_order_id);
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            account_id,
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1);

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(event.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.venue_order_id, Some(VenueOrderId::from("8886774")));
                assert_eq!(event.account_id, Some(account_id));
                assert_eq!(event.quantity, Quantity::new(0.001, 8));
                assert_eq!(event.price, Some(Price::new(7100.50, 8)));
                assert_eq!(event.trigger_price, None);
                assert_eq!(event.protection_price, None);
                assert!(!event.is_quote_quantity);
                assert_eq!(
                    event.ts_event,
                    UnixNanos::from_millis(msg.event_time as u64)
                );
                assert_eq!(event.ts_init, ts_init);
                assert!(!event.reconciliation);
                assert_eq!(event.causation_id, None);
            }
            other => panic!("Expected OrderUpdated for reduce-only quantity delta, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_update_trade_does_not_re_emit_updated_after_identity_refresh() {
        // Two TRADE messages with distinct trade_ids both report the venue's
        // reduced qty 0.001 (submitted 0.005). The first must emit OrderUpdated
        // and refresh the cached identity; the second must not re-emit.
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        let first = build_trade_order_update_with_trade_id(
            "0.001",
            "0.0005",
            "0.0005",
            "7100.50",
            1_000_001,
            "PARTIALLY_FILLED",
        );
        let second = build_trade_order_update_with_trade_id(
            "0.001", "0.0005", "0.001", "7100.50", 1_000_002, "FILLED",
        );

        for msg in [&first, &second] {
            dispatch_order_update(
                msg,
                &emitter,
                &http_client,
                AccountId::from("BINANCE-001"),
                BinanceProductType::UsdM,
                clock,
                &dispatch_state,
                false,
                Decimal::new(4, 4),
                Currency::USDT(),
                false,
                false,
                &seen_trade_ids,
            );
        }

        let events = collect_events(&mut rx);
        let updated_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Updated(_))))
            .count();
        let filled_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))))
            .count();
        let accepted_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))))
            .count();
        assert_eq!(
            accepted_count, 1,
            "OrderAccepted should be synthesized once"
        );
        assert_eq!(updated_count, 1, "OrderUpdated should be emitted only once");
        assert_eq!(filled_count, 2, "expected one OrderFilled per TRADE");
    }

    #[rstest]
    fn test_dispatch_trade_lite_does_not_re_emit_updated_after_identity_refresh() {
        // Two TRADE_LITE calls report the venue's reduced qty 0.001 (submitted
        // 0.005). The first must emit Accepted+Updated+Filled; the second must
        // emit only Filled because the cached identity quantity was refreshed.
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );

        for _ in 0..2 {
            dispatch_trade_lite(
                &msg,
                &emitter,
                &http_client,
                AccountId::from("BINANCE-001"),
                BinanceProductType::UsdM,
                clock,
                &dispatch_state,
            );
        }

        let events = collect_events(&mut rx);
        let updated_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Updated(_))))
            .count();
        let filled_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))))
            .count();
        let accepted_count = events
            .iter()
            .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))))
            .count();
        assert_eq!(accepted_count, 1);
        assert_eq!(updated_count, 1, "TRADE_LITE delta must not re-emit");
        assert_eq!(filled_count, 2);
    }

    #[rstest]
    fn test_dispatch_trade_lite_emits_updated_for_reduced_qty() {
        // Submitted reduce-only at 0.005 BTC; TRADE_LITE arrives without prior
        // NEW with q=0.001. The handler must emit Accepted, Updated, Filled.
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );

        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 3, "expected Accepted, Updated, Filled");
        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.quantity, Quantity::new(0.001, 8));
                assert_eq!(event.price, Some(Price::new(7100.50, 8)));
            }
            other => panic!("Expected OrderUpdated for TRADE_LITE qty delta, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_trade_lite_rejects_invalid_fill_quantity() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        msg.last_filled_qty = "not-a-number".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        dispatch_state.insert_accepted(client_order_id);

        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_)))),
            "invalid fill quantity must not emit OrderFilled"
        );
    }

    #[rstest]
    fn test_dispatch_trade_lite_rejects_invalid_fill_price() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        msg.last_filled_price = "not-a-number".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        dispatch_state.insert_accepted(client_order_id);

        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_)))),
            "invalid fill price must not emit OrderFilled"
        );
    }

    #[rstest]
    fn test_dispatch_order_update_new_with_zero_venue_qty_skips_updated() {
        // A malformed venue payload with q="0" must not emit a zero-quantity
        // OrderUpdated (which would later trip Quantity::positive invariants).
        let clock = get_atomic_clock_realtime();
        let msg = build_new_order_update_with_qty_and_price("0", "7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.005, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 1, "only OrderAccepted should be emitted");
        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
    }

    #[rstest]
    fn test_dispatch_order_update_new_with_matching_price_skips_updated() {
        let clock = get_atomic_clock_realtime();

        // Submitted with price 7100.50 and qty 0.001, venue confirmed identically (no delta).
        let msg = build_new_order_update_with_price("7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
            Quantity::new(0.001, 8),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            false,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert_eq!(
            events.len(),
            1,
            "no OrderUpdated expected when price matches"
        );
        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
    }

    fn usdm_instrument(symbol: &str, quote_asset: &str) -> BinanceFuturesInstrument {
        BinanceFuturesInstrument::UsdM(BinanceFuturesUsdSymbol {
            symbol: ustr::Ustr::from(symbol),
            pair: ustr::Ustr::from(symbol),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            status: BinanceTradingStatus::Trading,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: ustr::Ustr::from("BTC"),
            quote_asset: ustr::Ustr::from(quote_asset),
            margin_asset: ustr::Ustr::from(quote_asset),
            price_precision: 2,
            quantity_precision: 3,
            base_asset_precision: 8,
            quote_precision: 8,
            underlying_type: None,
            underlying_sub_type: vec![],
            settle_plan: None,
            trigger_protect: None,
            liquidation_fee: None,
            market_take_bound: None,
            order_types: vec![],
            time_in_force: vec![],
            filters: vec![serde_json::json!({})],
        })
    }

    fn coinm_instrument(symbol: &str) -> BinanceFuturesInstrument {
        BinanceFuturesInstrument::CoinM(BinanceFuturesCoinSymbol {
            symbol: ustr::Ustr::from(symbol),
            pair: ustr::Ustr::from("BTCUSD"),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            contract_status: Some(BinanceContractStatus::Trading),
            contract_size: 100,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: ustr::Ustr::from("BTC"),
            quote_asset: ustr::Ustr::from("USD"),
            margin_asset: ustr::Ustr::from("BTC"),
            price_precision: 1,
            quantity_precision: 0,
            base_asset_precision: 8,
            quote_precision: 8,
            equal_qty_precision: None,
            trigger_protect: None,
            market_take_bound: None,
            liquidation_fee: None,
            order_types: vec![],
            time_in_force: vec![],
            filters: vec![],
        })
    }

    #[rstest]
    fn test_resolve_instrument_metadata_rejects_product_mismatch() {
        let clock = get_atomic_clock_realtime();
        let http_client = create_test_http_client(clock);
        let symbol = ustr::Ustr::from("BTCUSDT");
        http_client
            .instruments_cache()
            .insert(symbol, coinm_instrument("BTCUSDT"));

        let resolved = resolve_instrument_metadata(
            &http_client,
            symbol,
            BinanceProductType::UsdM,
            "test update",
        );

        assert!(resolved.is_none());
    }

    #[rstest]
    fn test_dispatch_trade_lite_tracked_emits_filled() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state
            .order_identities
            .get_mut(&ClientOrderId::from("TEST"))
            .unwrap()
            .venue_position_id = Some(PositionId::from("BTCUSDT-PERP.BINANCE-LONG"));
        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        let fills: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ExecutionEvent::Order(OrderEventAny::Filled(fill)) => Some(fill),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 1);
        let fill = fills[0];
        assert_eq!(fill.trade_id, TradeId::new("12345678"));
        assert_eq!(fill.client_order_id, ClientOrderId::from("TEST"));
        assert_eq!(fill.last_qty, Quantity::new(0.001, 8));
        assert_eq!(fill.last_px, Price::new(7100.50, 8));
        assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
        assert_eq!(fill.currency, Currency::USDT());
        assert_eq!(
            fill.position_id,
            Some(PositionId::from("BTCUSDT-PERP.BINANCE-LONG")),
        );
        assert!(fill.commission.is_none());
    }

    #[rstest]
    fn test_dispatch_trade_lite_untracked_is_noop() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        assert!(events.is_empty(), "untracked TRADE_LITE should not emit");
    }

    #[rstest]
    fn test_dispatch_trade_lite_invalid_client_order_id_emits_nothing() {
        let clock = get_atomic_clock_realtime();
        let mut msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        msg.client_order_id = "client-é".to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();

        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        assert!(collect_events(&mut rx).is_empty());
        assert!(dispatch_state.order_identities.is_empty());
    }

    #[rstest]
    fn test_dispatch_trade_lite_uses_instrument_quote_currency() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        http_client
            .instruments_cache()
            .insert(ustr::Ustr::from("BTCUSDT"), coinm_instrument("BTCUSDT"));

        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::CoinM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        let fill = events
            .iter()
            .find_map(|event| match event {
                ExecutionEvent::Order(OrderEventAny::Filled(fill)) => Some(fill),
                _ => None,
            })
            .expect("expected OrderFilled event");
        assert_eq!(fill.currency, Currency::from("USD"));
    }

    #[rstest]
    fn test_dispatch_trade_lite_promotes_algo_id_before_fill() {
        let ts_init = UnixNanos::from(42);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));
        let mut algo_msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_new.json");
        let trade_msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let client_order_id = ClientOrderId::from("TEST");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let actual_order_id = VenueOrderId::from(trade_msg.order_id.to_string());
        algo_msg.algo_order.client_algo_id = client_order_id.to_string();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_state_with_price_and_qty(
            client_order_id,
            instrument_id,
            Some(Price::from("7100.50")),
            Quantity::from("0.001"),
        );
        let triggered_algo_ids = Arc::new(AtomicSet::new());
        let algo_client_ids = Arc::new(AtomicSet::new());

        dispatch_algo_update(
            &algo_msg,
            &emitter,
            &http_client,
            account_id,
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            &triggered_algo_ids,
            &algo_client_ids,
            false,
        );
        dispatch_trade_lite(
            &trade_msg,
            &emitter,
            &http_client,
            account_id,
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        assert_eq!(events.len(), 3);
        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(accepted.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(accepted.instrument_id, instrument_id);
                assert_eq!(accepted.client_order_id, client_order_id);
                assert_eq!(
                    accepted.venue_order_id,
                    VenueOrderId::from(algo_msg.algo_order.algo_id.to_string())
                );
                assert_eq!(accepted.account_id, account_id);
                assert_eq!(
                    accepted.ts_event,
                    UnixNanos::from_millis(algo_msg.event_time as u64)
                );
                assert_eq!(accepted.ts_init, ts_init);
                assert!(!accepted.reconciliation);
                assert_eq!(accepted.causation_id, None);
            }
            other => panic!("Expected OrderAccepted before promotion, was {other:?}"),
        }

        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(updated.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(updated.instrument_id, instrument_id);
                assert_eq!(updated.client_order_id, client_order_id);
                assert_eq!(updated.venue_order_id, Some(actual_order_id));
                assert_eq!(updated.account_id, Some(account_id));
                assert_eq!(updated.quantity, Quantity::from("0.001"));
                assert_eq!(updated.price, Some(Price::from("7100.50")));
                assert_eq!(updated.trigger_price, None);
                assert_eq!(updated.protection_price, None);
                assert_eq!(
                    updated.ts_event,
                    UnixNanos::from_millis(trade_msg.event_time as u64)
                );
                assert_eq!(updated.ts_init, ts_init);
                assert!(!updated.reconciliation);
                assert!(!updated.is_quote_quantity);
                assert_eq!(updated.causation_id, None);
            }
            other => panic!("Expected OrderUpdated before fill, was {other:?}"),
        }

        match &events[2] {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.trader_id, TraderId::from("TESTER-001"));
                assert_eq!(filled.strategy_id, StrategyId::from("TEST-STRATEGY"));
                assert_eq!(filled.instrument_id, instrument_id);
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(filled.venue_order_id, actual_order_id);
                assert_eq!(filled.account_id, account_id);
                assert_eq!(filled.trade_id, TradeId::new("12345678"));
                assert_eq!(filled.order_side, OrderSide::Buy);
                assert_eq!(filled.order_type, OrderType::Limit);
                assert_eq!(filled.last_qty, Quantity::from("0.001"));
                assert_eq!(filled.last_px, Price::from("7100.50"));
                assert_eq!(filled.currency, Currency::USDT());
                assert_eq!(filled.liquidity_side, LiquiditySide::Maker);
                assert_eq!(
                    filled.ts_event,
                    UnixNanos::from_millis(trade_msg.event_time as u64)
                );
                assert_eq!(filled.ts_init, ts_init);
                assert!(!filled.reconciliation);
                assert_eq!(filled.position_id, None);
                assert_eq!(filled.commission, None);
                assert_eq!(filled.info, None);
                assert_eq!(filled.causation_id, None);
            }
            other => panic!("Expected OrderFilled after promotion, was {other:?}"),
        }
        assert_eq!(
            dispatch_state.promoted_algo_order_id(&client_order_id),
            Some(actual_order_id)
        );
    }

    #[rstest]
    fn test_dispatch_order_update_trade_tracked_skips_fill_when_use_trade_lite() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade_partial.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_dispatch_state(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state.insert_algo_order_id(client_order_id, VenueOrderId::from("2148719"));
        assert_eq!(
            dispatch_state.promote_algo_order_id(
                client_order_id,
                VenueOrderId::from(msg.order.order_id.to_string())
            ),
            Some(true)
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            true, // use_trade_lite
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_)))),
            "tracked Trade under use_trade_lite should not emit OrderFilled"
        );
        assert!(
            dispatch_state
                .order_identities
                .contains_key(&client_order_id),
            "non-terminal fill should not clean up identity"
        );
    }

    #[rstest]
    fn test_dispatch_order_update_trade_tracked_runs_cleanup_when_terminal_with_use_trade_lite() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_dispatch_state(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            true, // use_trade_lite
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_)))),
            "tracked Trade under use_trade_lite should not emit OrderFilled"
        );
        assert!(
            !dispatch_state
                .order_identities
                .contains_key(&client_order_id),
            "terminal fill should still clean up identity"
        );
        assert_eq!(
            dispatch_state.promote_algo_order_id(client_order_id, VenueOrderId::from("8886774")),
            None,
            "terminal fill should clean up algo order IDs"
        );
    }

    #[rstest]
    fn test_dispatch_order_update_trade_untracked_still_emits_reports_with_use_trade_lite() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            Currency::USDT(),
            false,
            true, // use_trade_lite
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        let bundled: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, fills)) =
                    event
                {
                    Some((status, fills))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(bundled.len(), 1);
        assert_eq!(bundled[0].1.len(), 1);
        let expected_position_id = Some(PositionId::from("BTCUSDT-PERP.BINANCE-LONG"));
        assert_eq!(bundled[0].0.venue_position_id, expected_position_id);
        assert_eq!(bundled[0].1[0].venue_position_id, expected_position_id);
    }

    #[rstest]
    fn test_dispatch_trade_lite_uses_usdm_instrument_quote_currency() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        http_client.instruments_cache().insert(
            ustr::Ustr::from("BTCUSDT"),
            usdm_instrument("BTCUSDT", "BUSD"),
        );

        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_trade_lite(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
        );

        let events = collect_events(&mut rx);
        let fill = events
            .iter()
            .find_map(|event| match event {
                ExecutionEvent::Order(OrderEventAny::Filled(fill)) => Some(fill),
                _ => None,
            })
            .expect("expected OrderFilled event");
        assert_eq!(fill.currency, Currency::from("BUSD"));
    }
}

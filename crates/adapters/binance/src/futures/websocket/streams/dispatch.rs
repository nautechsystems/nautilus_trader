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

use std::sync::{Arc, Mutex};

use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_common::{cache::fifo::FifoCache, live::get_runtime};
use nautilus_core::{AtomicSet, MUTEX_POISONED, UUID4, UnixNanos, time::AtomicTime};
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
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{
    messages::{
        BinanceExecutionType, BinanceFuturesAlgoUpdateMsg, BinanceFuturesOrderUpdateMsg,
        BinanceFuturesTradeLiteMsg, BinanceFuturesWsStreamsMessage,
    },
    parse_exec::{
        decode_algo_client_id, parse_futures_account_update,
        parse_futures_algo_update_to_order_status, parse_futures_order_update_to_fill,
        parse_futures_order_update_to_order_status,
    },
};
use crate::{
    common::{
        consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        dispatch::{WsDispatchState, ensure_accepted_emitted},
        encoder::decode_broker_id,
        enums::{BinancePositionSide, BinanceProductType},
        symbol::format_instrument_id,
    },
    futures::http::client::{BinanceFuturesHttpClient, BinanceFuturesInstrument},
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
    pub treat_expired_as_canceled: bool,
    pub use_trade_lite: bool,
    pub seen_trade_ids: Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
    pub cancellation_token: CancellationToken,
}

/// Spawns the user data stream dispatch task. The task consumes `stream` and
/// routes each message through `dispatch_fn`.
pub(crate) fn spawn_user_stream_dispatch<S, F>(
    stream: S,
    ctx: Arc<DispatchCtx>,
    recovery_tx: tokio::sync::mpsc::UnboundedSender<()>,
    dispatch_fn: F,
) -> JoinHandle<()>
where
    S: Stream<Item = BinanceFuturesWsStreamsMessage> + Send + 'static,
    F: Fn(BinanceFuturesWsStreamsMessage, &DispatchCtx, &tokio::sync::mpsc::UnboundedSender<()>)
        + Send
        + Sync
        + 'static,
{
    let cancel = ctx.cancellation_token.clone();

    get_runtime().spawn(async move {
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
    })
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
            );
        }
        BinanceFuturesWsStreamsMessage::AccountUpdate(update) => {
            let ts_init = clock.get_time_ns();

            if let Some(state) = parse_futures_account_update(&update, account_id, ts_init) {
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
            log::error!(
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
    treat_expired_as_canceled: bool,
    use_trade_lite: bool,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let symbol_ustr = ustr::Ustr::from(order.symbol.as_str());
    let ts_init = clock.get_time_ns();
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let cache = http_client.instruments_cache();
    let cached_instrument = cache.get(&symbol_ustr);

    let (instrument_id, price_precision, size_precision) = if let Some(ref inst) = cached_instrument
    {
        (
            inst.id(),
            inst.price_precision() as u8,
            inst.quantity_precision() as u8,
        )
    } else {
        let id = format_instrument_id(&symbol_ustr, product_type);
        log::warn!(
            "Instrument not in cache for {}, using default precision",
            order.symbol
        );
        (id, 8, 8)
    };

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));

    // Exchange-generated orders (liquidation/ADL/settlement) are routed through
    // reconciliation reports regardless of tracked/untracked state, because
    // they have no locally submitted identity
    if order.is_exchange_generated() {
        let is_linear = cached_instrument
            .as_ref()
            .map_or(product_type == BinanceProductType::UsdM, |inst| {
                matches!(inst.value(), BinanceFuturesInstrument::UsdM(_))
            });

        let quote_currency = cached_instrument
            .as_ref()
            .map_or_else(Currency::USDT, |inst| inst.value().quote_currency());

        let taker_fee = if is_linear {
            Some(default_taker_fee)
        } else {
            None
        };

        let venue_position_id =
            make_venue_position_id(use_position_ids, instrument_id, order.position_side);

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

        match order.execution_type {
            BinanceExecutionType::New => {
                if dispatch_state.has_emitted_accepted(&client_order_id)
                    || dispatch_state.has_filled(&client_order_id)
                {
                    log::debug!("Skipping duplicate Accepted for {client_order_id}");
                    return;
                }
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

                // Detect venue-assigned price changes (e.g. priceMatch orders)
                if let Some(submitted_price) = identity.price {
                    let venue_price: f64 = order.original_price.parse().unwrap_or(0.0);

                    if venue_price > 0.0 {
                        let venue_price = Price::new(venue_price, price_precision);
                        let submitted_at_precision =
                            Price::new(submitted_price.as_f64(), price_precision);

                        if venue_price != submitted_at_precision {
                            let quantity: f64 = order.original_qty.parse().unwrap_or(0.0);
                            let trigger_price: f64 = order.stop_price.parse().unwrap_or(0.0);
                            let updated = OrderUpdated::new(
                                emitter.trader_id(),
                                identity.strategy_id,
                                identity.instrument_id,
                                client_order_id,
                                Quantity::new(quantity, size_precision),
                                UUID4::new(),
                                ts_event,
                                ts_init,
                                false,
                                Some(venue_order_id),
                                Some(account_id),
                                Some(venue_price),
                                if trigger_price > 0.0 {
                                    Some(Price::new(trigger_price, price_precision))
                                } else {
                                    None
                                },
                                None,
                                false,
                            );
                            emitter.send_order_event(OrderEventAny::Updated(updated));
                        }
                    }
                }
            }
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
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

                // When use_trade_lite is on, the TRADE_LITE handler owns the
                // fill emission. This arm still runs so the terminal-state
                // cleanup below fires (it needs `z` from ORDER_TRADE_UPDATE,
                // which TRADE_LITE does not carry).
                if !use_trade_lite && !is_duplicate {
                    let last_qty: f64 = order.last_filled_qty.parse().unwrap_or(0.0);
                    let last_px: f64 = order.last_filled_price.parse().unwrap_or(0.0);
                    let commission: f64 = order
                        .commission
                        .as_deref()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0);
                    let commission_currency = order
                        .commission_asset
                        .as_ref()
                        .map_or_else(Currency::USDT, |a| Currency::from(a.as_str()));

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
                        Quantity::new(last_qty, size_precision),
                        Price::new(last_px, price_precision),
                        commission_currency,
                        liquidity_side,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        None,
                        Some(Money::new(commission, commission_currency)),
                    );

                    dispatch_state.insert_filled(client_order_id);
                    emitter.send_order_event(OrderEventAny::Filled(filled));
                }

                let cum_qty: f64 = order.cumulative_filled_qty.parse().unwrap_or(0.0);
                let orig_qty: f64 = order.original_qty.parse().unwrap_or(0.0);

                if (orig_qty - cum_qty) <= 0.0 {
                    dispatch_state.cleanup_terminal(client_order_id);
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
                let quantity: f64 = order.original_qty.parse().unwrap_or(0.0);
                let price: f64 = order.original_price.parse().unwrap_or(0.0);

                let updated = OrderUpdated::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    Quantity::new(quantity, size_precision),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    Some(Price::new(price, price_precision)),
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
        // venue_position_id is intentionally None here: the engine assigns
        // position IDs during event processing, and setting one from the
        // adapter could split a partially filled order across two positions.
        match order.execution_type {
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
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
                    None,
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
                    Ok(status) => Some(status),
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
                    Ok(status) => emitter.send_order_status_report(status),
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
    let symbol_ustr = ustr::Ustr::from(msg.symbol.as_str());
    let ts_init = clock.get_time_ns();
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let cache = http_client.instruments_cache();
    let cached_instrument = cache.get(&symbol_ustr);

    let (instrument_id, price_precision, size_precision) = if let Some(ref inst) = cached_instrument
    {
        (
            inst.id(),
            inst.price_precision() as u8,
            inst.quantity_precision() as u8,
        )
    } else {
        let id = format_instrument_id(&symbol_ustr, product_type);
        log::warn!(
            "Instrument not in cache for {}, using default precision",
            msg.symbol
        );
        (id, 8, 8)
    };

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &msg.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));

    let Some(identity) = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone())
    else {
        log::debug!("TRADE_LITE for untracked order {client_order_id}, skipping");
        return;
    };

    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());

    ensure_accepted_emitted(
        client_order_id,
        account_id,
        venue_order_id,
        &identity,
        emitter,
        dispatch_state,
        ts_init,
    );

    let last_qty: f64 = msg.last_filled_qty.parse().unwrap_or(0.0);
    let last_px: f64 = msg.last_filled_price.parse().unwrap_or(0.0);

    let liquidity_side = if msg.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    // TRADE_LITE does not carry commission_asset, so fall back to the
    // instrument's quote currency (COIN-M and non-USDT USD-M symbols).
    let quote_currency = cached_instrument
        .as_ref()
        .map_or_else(Currency::USDT, |inst| inst.value().quote_currency());

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
        Quantity::new(last_qty, size_precision),
        Price::new(last_px, price_precision),
        quote_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        None,
        None,
    );

    dispatch_state.insert_filled(client_order_id);
    emitter.send_order_event(OrderEventAny::Filled(filled));
}

/// Derives a venue position ID from the instrument and Binance position side.
///
/// Returns `None` when `use_position_ids` is false.
pub(crate) fn make_venue_position_id(
    use_position_ids: bool,
    instrument_id: InstrumentId,
    position_side: BinancePositionSide,
) -> Option<PositionId> {
    if !use_position_ids {
        return None;
    }

    let side = match position_side {
        BinancePositionSide::Long => "LONG",
        BinancePositionSide::Short => "SHORT",
        BinancePositionSide::Both => "BOTH",
        _ => "UNKNOWN",
    };
    Some(PositionId::new(format!("{instrument_id}-{side}")))
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
    venue_position_id: Option<PositionId>,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let last_qty: f64 = order.last_filled_qty.parse().unwrap_or(0.0);

    let order_kind = if order.is_liquidation() {
        "liquidation"
    } else if order.is_adl() {
        "ADL"
    } else {
        "settlement"
    };

    if last_qty == 0.0 {
        log::warn!(
            "Exchange-generated {order_kind} pending: symbol={}, client_order_id={}, status={:?}",
            order.symbol,
            order.client_order_id,
            order.order_status,
        );
        return;
    }

    let dedup_key = (order.symbol, order.trade_id);
    let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
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
        Ok(status) => Some(status),
        Err(e) => {
            log::error!("Failed to parse order status report: {e}");
            None
        }
    };

    emit_bundled_or_individual(emitter, status, fill);
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
) {
    use crate::common::enums::BinanceAlgoStatus;

    let algo_data = &msg.algo_order;
    let ts_init = clock.get_time_ns();
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);
    let client_order_id = decode_algo_client_id(algo_data);

    let symbol_ustr = ustr::Ustr::from(algo_data.symbol.as_str());
    let (instrument_id, _price_precision, _size_precision) =
        if let Some(inst) = http_client.instruments_cache().get(&symbol_ustr) {
            (
                inst.id(),
                inst.price_precision() as u8,
                inst.quantity_precision() as u8,
            )
        } else {
            let id = format_instrument_id(&symbol_ustr, product_type);
            log::warn!(
                "Instrument not in cache for {}, using default precision",
                algo_data.symbol
            );
            (id, 8, 8)
        };

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());

    match algo_data.algo_status {
        BinanceAlgoStatus::New => {
            algo_client_ids.insert(client_order_id);
        }
        BinanceAlgoStatus::Triggering => {
            log::info!(
                "Algo order triggering: client_order_id={}, algo_id={}, symbol={}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.symbol
            );
        }
        BinanceAlgoStatus::Triggered => {
            triggered_algo_ids.insert(client_order_id);
            log::info!(
                "Algo order triggered: client_order_id={}, algo_id={}, actual_order_id={:?}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.actual_order_id
            );
        }
        BinanceAlgoStatus::Canceled | BinanceAlgoStatus::Expired => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);

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
                );
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            } else if let Some(report) = parse_futures_algo_update_to_order_status(
                algo_data,
                msg.event_time,
                instrument_id,
                _price_precision,
                _size_precision,
                account_id,
                ts_init,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        BinanceAlgoStatus::Rejected => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);

            if let Some(identity) = identity {
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.emit_order_rejected_event(
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    "REJECTED",
                    ts_init,
                    false,
                );
            } else if let Some(report) = parse_futures_algo_update_to_order_status(
                algo_data,
                msg.event_time,
                instrument_id,
                _price_precision,
                _size_precision,
                account_id,
                ts_init,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        BinanceAlgoStatus::Finished => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);
            dispatch_state.cleanup_terminal(client_order_id);

            let executed_qty: f64 = algo_data
                .executed_qty
                .as_ref()
                .and_then(|q| q.parse().ok())
                .unwrap_or(0.0);

            if executed_qty > 0.0 {
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
            enums::{BinanceContractStatus, BinanceEnvironment, BinanceTradingStatus},
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
    #[case::both(BinancePositionSide::Both, "ETHUSDT-PERP.BINANCE-BOTH")]
    #[case::unknown(BinancePositionSide::Unknown, "ETHUSDT-PERP.BINANCE-UNKNOWN")]
    fn test_make_venue_position_id_enabled(
        #[case] side: BinancePositionSide,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(true, instrument_id, side);
        assert_eq!(result, Some(PositionId::from(expected)));
    }

    fn make_status_report() -> OrderStatusReport {
        use nautilus_model::enums::TimeInForce;
        OrderStatusReport::new(
            AccountId::from("BINANCE-001"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(ClientOrderId::from("O-PARSER-001")),
            VenueOrderId::from("V-PARSER-001"),
            OrderSide::Buy,
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
    fn test_make_venue_position_id_disabled(#[case] side: BinancePositionSide) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(false, instrument_id, side);
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_tracked_trade() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
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
            true,
            Decimal::new(4, 4),
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
            false,
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        // Exchange-generated fills emit a single bundled OrderWithFills report.
        // The duplicate trade_id is suppressed by the seen_trade_ids dedup.
        assert_eq!(events.len(), 1);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, fills))
                        if status.order_status == OrderStatus::Filled
                            && fills.len() == 1
                            && fills[0].trade_id == TradeId::new("12345999")
                ))
                .count(),
            1
        );
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
        BinanceFuturesHttpClient::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Mainnet,
            clock,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .expect("Test HTTP client should be created")
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
        let json = format!(
            r#"{{
                "e":"ORDER_TRADE_UPDATE","T":1568879465651,"E":1568879465651,
                "o":{{
                    "s":"BTCUSDT","c":"TEST","S":"BUY","o":"LIMIT","f":"GTC",
                    "q":"0.001","p":"{price}","ap":"0","sp":"0",
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

    fn create_tracked_state_with_price(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        price: Option<Price>,
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

        // Submitted with price 7000, venue filled it at 7100.50 (priceMatch).
        let msg = build_new_order_update_with_price("7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7000.0, 8)),
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
    fn test_dispatch_order_update_new_with_matching_price_skips_updated() {
        let clock = get_atomic_clock_realtime();

        // Submitted with price 7100.50, venue confirmed at 7100.50 (no drift).
        let msg = build_new_order_update_with_price("7100.50");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("TEST");
        let dispatch_state = create_tracked_state_with_price(
            client_order_id,
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Some(Price::new(7100.50, 8)),
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
    fn test_dispatch_trade_lite_tracked_emits_filled() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesTradeLiteMsg = load_user_data_fixture("trade_lite.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
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
            false,
            true, // use_trade_lite
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);
        let bundled = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, fills))
                        if fills.len() == 1
                )
            })
            .count();
        assert_eq!(
            bundled, 1,
            "untracked Trade should emit a single bundled OrderWithFills regardless of use_trade_lite"
        );
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

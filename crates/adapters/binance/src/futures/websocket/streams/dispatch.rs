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
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{
    messages::{
        BinanceExecutionType, BinanceFuturesAlgoUpdateMsg, BinanceFuturesOrderUpdateMsg,
        BinanceFuturesWsStreamsMessage,
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
                seen_trade_ids,
            );
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

                if is_duplicate {
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

                match parse_futures_order_update_to_fill(
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
                    Ok(fill) => emitter.send_fill_report(fill),
                    Err(e) => log::error!("Failed to parse fill report: {e}"),
                }

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
/// Sends a `FillReport` first, then an `OrderStatusReport`. The fill report
/// is dropped by the engine (order not yet in cache). The status report
/// triggers `create_external_order`, which builds the order and applies an
/// inferred fill from `avg_px`/`filled_qty`. Real fill metadata (commission,
/// trade_id) is lost; see `engine-bundled-fill-reconciliation` plan for the
/// path to preserving it.
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

    match parse_futures_order_update_to_fill(
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
        Ok(fill) => emitter.send_fill_report(fill),
        Err(e) => log::error!("Failed to parse fill report: {e}"),
    }

    match parse_futures_order_update_to_order_status(
        msg,
        instrument_id,
        price_precision,
        size_precision,
        account_id,
        false, // Exchange-generated fills are not subject to expired-as-canceled
        ts_init,
    ) {
        Ok(status) => emitter.send_order_status_report(status),
        Err(e) => log::error!("Failed to parse order status report: {e}"),
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
            dispatch::OrderIdentity, enums::BinanceEnvironment, testing::load_fixture_string,
        },
        futures::http::client::BinanceFuturesHttpClient,
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
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Fill(fill))
                        if fill.trade_id == TradeId::new("12345678")
                ))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Order(status))
                        if status.client_order_id == Some(ClientOrderId::from("TEST"))
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
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Fill(fill))
                        if fill.trade_id == TradeId::new("12345999")
                ))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Order(status))
                        if status.order_status == OrderStatus::Filled
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
}

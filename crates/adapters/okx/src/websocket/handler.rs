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

//! WebSocket message handler for OKX.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel, serializing them to JSON
//! and sending via the WebSocket. Raw messages are received from the network, deserialized,
//! and transformed into `NautilusWsMessage` events which are emitted back to the client.
//!
//! Responsibilities:
//! - Command processing: Receives `HandlerCommand` from client, executes WebSocket operations.
//! - Message transformation: Parses raw venue messages into Nautilus domain events.
//! - Pending state tracking: Owns `AHashMap` for matching requests/responses (single-threaded).
//! - Retry logic: Retries transient WebSocket send failures using `RetryManager`.
//! - Error event emission: Emits `OrderRejected`, `OrderCancelRejected` when retries exhausted.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{AtomicTime, UUID4, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{OrderStatus, OrderType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderModifyRejected, OrderRejected,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Money, Quantity},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, TEXT_PING, TEXT_PONG, WebSocketClient},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{OKXSubscriptionEvent, OKXWsChannel, OKXWsOperation},
    error::OKXWsError,
    messages::{
        ExecutionReport, NautilusWsMessage, OKXAlgoOrderMsg, OKXBookMsg, OKXOrderMsg,
        OKXSubscription, OKXSubscriptionArg, OKXWebSocketArg, OKXWebSocketError, OKXWsMessage,
        OKXWsRequest, WsAmendOrderParams, WsCancelAlgoOrderParamsBuilder,
        WsCancelOrderParamsBuilder, WsMassCancelParams, WsPostAlgoOrderParams, WsPostOrderParams,
    },
    parse::{
        OrderStateSnapshot, ParsedOrderEvent, parse_algo_order_msg, parse_book_msg_vec,
        parse_order_event, parse_order_msg, parse_ws_message_data,
    },
    subscription::topic_from_websocket_arg,
};
use crate::{
    common::{
        consts::{
            OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE, OKX_POST_ONLY_ERROR_CODE,
            should_retry_error_code,
        },
        enums::{
            OKXBookAction, OKXInstrumentType, OKXOrderStatus, OKXSide, OKXTargetCurrency,
            OKXTradeMode,
        },
        parse::{
            determine_order_type, is_market_price, okx_instrument_type, parse_account_state,
            parse_client_order_id, parse_millisecond_timestamp, parse_position_status_report,
            parse_price, parse_quantity,
        },
    },
    http::models::{OKXAccount, OKXPosition},
    websocket::client::{
        OKX_RATE_LIMIT_KEY_AMEND, OKX_RATE_LIMIT_KEY_CANCEL, OKX_RATE_LIMIT_KEY_ORDER,
        OKX_RATE_LIMIT_KEY_SUBSCRIPTION,
    },
};

/// Data cached for pending place requests to correlate with responses.
type PlaceRequestData = (
    PendingOrderParams,
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
);

/// Data cached for pending cancel requests to correlate with responses.
type CancelRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// Data cached for pending amend requests to correlate with responses.
type AmendRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

#[derive(Debug)]
pub enum PendingOrderParams {
    Regular(WsPostOrderParams),
    Algo(WsPostAlgoOrderParams),
}

/// Commands sent from the outer client to the inner message handler.
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
#[allow(missing_debug_implementations)]
pub enum HandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Authenticate {
        payload: String,
    },
    InitializeInstruments(Vec<InstrumentAny>),
    UpdateInstrument(InstrumentAny),
    Subscribe {
        args: Vec<OKXSubscriptionArg>,
    },
    Unsubscribe {
        args: Vec<OKXSubscriptionArg>,
    },
    PlaceOrder {
        params: WsPostOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    PlaceAlgoOrder {
        params: WsPostAlgoOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    AmendOrder {
        params: WsAmendOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    },
    CancelOrder {
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    },
    CancelAlgoOrder {
        client_order_id: Option<ClientOrderId>,
        algo_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    },
    MassCancel {
        instrument_id: InstrumentId,
    },
    BatchPlaceOrders {
        args: Vec<Value>,
        request_id: String,
    },
    BatchAmendOrders {
        args: Vec<Value>,
        request_id: String,
    },
    BatchCancelOrders {
        args: Vec<Value>,
        request_id: String,
    },
}

pub(super) struct OKXWsFeedHandler {
    clock: &'static AtomicTime,
    account_id: AccountId,
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions_state: SubscriptionState,
    retry_manager: RetryManager<OKXWsError>,
    pending_place_requests: AHashMap<String, PlaceRequestData>,
    pending_cancel_requests: AHashMap<String, CancelRequestData>,
    pending_amend_requests: AHashMap<String, AmendRequestData>,
    pending_mass_cancel_requests: AHashMap<String, InstrumentId>,
    pending_messages: VecDeque<NautilusWsMessage>,
    active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
    client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
    emitted_accepted: FifoCache<VenueOrderId, 10_000>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    fee_cache: AHashMap<Ustr, Money>,
    filled_qty_cache: AHashMap<Ustr, Quantity>,
    order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot>,
    funding_rate_cache: AHashMap<Ustr, (Ustr, u64)>, // Cache (funding_rate, funding_time) by inst_id
    last_account_state: Option<AccountState>,
    request_id_counter: AtomicU64,
}

impl OKXWsFeedHandler {
    /// Creates a new [`OKXWsFeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: AccountId,
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
        client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            account_id,
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions_state,
            retry_manager: create_websocket_retry_manager(),
            pending_place_requests: AHashMap::new(),
            pending_cancel_requests: AHashMap::new(),
            pending_amend_requests: AHashMap::new(),
            pending_mass_cancel_requests: AHashMap::new(),
            pending_messages: VecDeque::new(),
            active_client_orders,
            client_id_aliases,
            emitted_accepted: FifoCache::new(),
            instruments_cache: AHashMap::new(),
            fee_cache: AHashMap::new(),
            filled_qty_cache: AHashMap::new(),
            order_state_cache: AHashMap::new(),
            funding_rate_cache: AHashMap::new(),
            last_account_state: None,
            request_id_counter: AtomicU64::new(0),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), ()> {
        self.out_tx.send(msg).map_err(|_| ())
    }

    async fn send_with_retry(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), OKXWsError> {
        if let Some(client) = &self.inner {
            let keys_owned: Option<Vec<Ustr>> = rate_limit_keys.map(|k| k.to_vec());
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        let keys = keys_owned.clone();
                        async move {
                            client
                                .send_text(payload, keys.as_deref())
                                .await
                                .map_err(|e| OKXWsError::ClientError(format!("Send failed: {e}")))
                        }
                    },
                    should_retry_okx_error,
                    create_okx_timeout_error,
                )
                .await
        } else {
            Err(OKXWsError::ClientError(
                "No active WebSocket client".to_string(),
            ))
        }
    }

    pub(super) async fn send_pong(&self) -> anyhow::Result<()> {
        match self.send_with_retry(TEXT_PONG.to_string(), None).await {
            Ok(()) => {
                log::trace!("Sent pong response to OKX text ping");
                Ok(())
            }
            Err(e) => {
                log::warn!("Failed to send pong after retries: error={e}");
                Err(anyhow::anyhow!("Failed to send pong: {e}"))
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return None;
                        }
                        HandlerCommand::Authenticate { payload } => {
                            if let Err(e) = self.send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice())).await {
                                log::error!("Failed to send authentication message after retries: error={e}");
                            }
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                        HandlerCommand::Subscribe { args } => {
                            if let Err(e) = self.handle_subscribe(args).await {
                                log::error!("Failed to handle subscribe command: error={e}");
                            }
                        }
                        HandlerCommand::Unsubscribe { args } => {
                            if let Err(e) = self.handle_unsubscribe(args).await {
                                log::error!("Failed to handle unsubscribe command: error={e}");
                            }
                        }
                        HandlerCommand::CancelOrder {
                            client_order_id,
                            venue_order_id,
                            instrument_id,
                            trader_id,
                            strategy_id,
                        } => {
                            if let Err(e) = self
                                .handle_cancel_order(
                                    client_order_id,
                                    venue_order_id,
                                    instrument_id,
                                    trader_id,
                                    strategy_id,
                                )
                                .await
                            {
                                log::error!("Failed to handle cancel order command: error={e}");
                            }
                        }
                        HandlerCommand::CancelAlgoOrder {
                            client_order_id,
                            algo_order_id,
                            instrument_id,
                            trader_id,
                            strategy_id,
                        } => {
                            if let Err(e) = self
                                .handle_cancel_algo_order(
                                    client_order_id,
                                    algo_order_id,
                                    instrument_id,
                                    trader_id,
                                    strategy_id,
                                )
                                .await
                            {
                                log::error!("Failed to handle cancel algo order command: error={e}");
                            }
                        }
                        HandlerCommand::PlaceOrder {
                            params,
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                        } => {
                            if let Err(e) = self
                                .handle_place_order(
                                    params,
                                    client_order_id,
                                    trader_id,
                                    strategy_id,
                                    instrument_id,
                                )
                                .await
                            {
                                log::error!("Failed to handle place order command: error={e}");
                            }
                        }
                        HandlerCommand::PlaceAlgoOrder {
                            params,
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                        } => {
                            if let Err(e) = self
                                .handle_place_algo_order(
                                    params,
                                    client_order_id,
                                    trader_id,
                                    strategy_id,
                                    instrument_id,
                                )
                                .await
                            {
                                log::error!("Failed to handle place algo order command: error={e}");
                            }
                        }
                        HandlerCommand::AmendOrder {
                            params,
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                            venue_order_id,
                        } => {
                            if let Err(e) = self
                                .handle_amend_order(
                                    params,
                                    client_order_id,
                                    trader_id,
                                    strategy_id,
                                    instrument_id,
                                    venue_order_id,
                                )
                                .await
                            {
                                log::error!("Failed to handle amend order command: error={e}");
                            }
                        }
                        HandlerCommand::MassCancel { instrument_id } => {
                            if let Err(e) = self.handle_mass_cancel(instrument_id).await {
                                log::error!("Failed to handle mass cancel command: error={e}");
                            }
                        }
                        HandlerCommand::BatchCancelOrders { args, request_id } => {
                            if let Err(e) = self.handle_batch_cancel_orders(args, request_id).await {
                                log::error!("Failed to handle batch cancel orders command: error={e}");
                            }
                        }
                        HandlerCommand::BatchPlaceOrders { args, request_id } => {
                            if let Err(e) = self.handle_batch_place_orders(args, request_id).await {
                                log::error!("Failed to handle batch place orders command: error={e}");
                            }
                        }
                        HandlerCommand::BatchAmendOrders { args, request_id } => {
                            if let Err(e) = self.handle_batch_amend_orders(args, request_id).await {
                                log::error!("Failed to handle batch amend orders command: error={e}");
                            }
                        }
                    }
                    // Continue processing following command
                    continue;
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(std::sync::atomic::Ordering::Acquire) {
                        log::debug!("Stop signal received during idle period");
                        return None;
                    }
                    continue;
                }

                msg = self.raw_rx.recv() => {
                    let event = match msg {
                        Some(msg) => match Self::parse_raw_message(msg) {
                            Some(event) => event,
                            None => continue,
                        },
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    let ts_init = self.clock.get_time_ns();

            match event {
                OKXWsMessage::Ping => {
                    if let Err(e) = self.send_pong().await {
                        log::warn!("Failed to send pong response: error={e}");
                    }
                    continue;
                }
                OKXWsMessage::Login {
                    code, msg, conn_id, ..
                } => {
                    if code == "0" {
                        self.auth_tracker.succeed();

                        // Must return immediately to deliver Authenticated message.
                        // Using push_back() + continue blocks the select! loop and prevents
                        // the spawn block from receiving this event, breaking reconnection flow.
                        return Some(NautilusWsMessage::Authenticated);
                    }

                    log::error!("WebSocket authentication failed: error={msg}");
                    self.auth_tracker.fail(msg.clone());

                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: Some(conn_id),
                        timestamp: self.clock.get_time_ns().as_u64(),
                    };
                    self.pending_messages
                        .push_back(NautilusWsMessage::Error(error));
                    continue;
                }
                OKXWsMessage::BookData { arg, action, data } => {
                    if let Some(msg) = self.handle_book_data(arg, action, data, ts_init) {
                        return Some(msg);
                    }
                    continue;
                }
                OKXWsMessage::OrderResponse {
                    id,
                    op,
                    code,
                    msg,
                    data,
                } => {
                    if let Some(msg) = self.handle_order_response(id, op, code, msg, data, ts_init) {
                        return Some(msg);
                    }
                    continue;
                }
                OKXWsMessage::Data { arg, data } => {
                    let OKXWebSocketArg {
                        channel, inst_id, ..
                    } = arg;

                    match channel {
                        OKXWsChannel::Account => {
                            if let Some(msg) = self.handle_account_data(data, ts_init) {
                                return Some(msg);
                            }
                            continue;
                        }
                        OKXWsChannel::Positions => {
                            self.handle_positions_data(data, ts_init);
                            continue;
                        }
                        OKXWsChannel::Orders => {
                            if let Some(msg) = self.handle_orders_data(data, ts_init) {
                                return Some(msg);
                            }
                            continue;
                        }
                        OKXWsChannel::OrdersAlgo => {
                            if let Some(msg) = self.handle_algo_orders_data(data, ts_init) {
                                return Some(msg);
                            }
                            continue;
                        }
                        _ => {
                            if let Some(msg) =
                                self.handle_other_channel_data(channel, inst_id, data, ts_init)
                            {
                                return Some(msg);
                            }
                            continue;
                        }
                    }
                }
                OKXWsMessage::Error { code, msg } => {
                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: None,
                        timestamp: self.clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                OKXWsMessage::Reconnected => {
                    return Some(NautilusWsMessage::Reconnected);
                }
                OKXWsMessage::Subscription {
                    event,
                    arg,
                    code,
                    msg,
                    ..
                } => {
                    let topic = topic_from_websocket_arg(&arg);
                    let success = code.as_deref().is_none_or(|c| c == "0");

                    match event {
                        OKXSubscriptionEvent::Subscribe => {
                            if success {
                                self.subscriptions_state.confirm_subscribe(&topic);
                            } else {
                                log::warn!("Subscription failed: topic={topic:?}, error={msg:?}, code={code:?}");
                                self.subscriptions_state.mark_failure(&topic);
                            }
                        }
                        OKXSubscriptionEvent::Unsubscribe => {
                            if success {
                                self.subscriptions_state.confirm_unsubscribe(&topic);
                            } else {
                                log::warn!("Unsubscription failed - restoring subscription: topic={topic:?}, error={msg:?}, code={code:?}");
                                // Venue rejected unsubscribe, so we're still subscribed. Restore state:
                                self.subscriptions_state.confirm_unsubscribe(&topic); // Clear pending_unsubscribe
                                self.subscriptions_state.mark_subscribe(&topic);      // Mark as subscribing
                                self.subscriptions_state.confirm_subscribe(&topic);   // Confirm subscription
                            }
                        }
                    }

                    continue;
                }
                OKXWsMessage::ChannelConnCount { .. } => continue,
            }
                }

                // Handle shutdown - either channel closed or stream ended
                else => {
                    log::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    pub(super) fn is_post_only_auto_cancel(msg: &OKXOrderMsg) -> bool {
        if msg.state != OKXOrderStatus::Canceled {
            return false;
        }

        let cancel_source_matches = matches!(
            msg.cancel_source.as_deref(),
            Some(source) if source == OKX_POST_ONLY_CANCEL_SOURCE
        );

        let reason_matches = matches!(
            msg.cancel_source_reason.as_deref(),
            Some(reason) if reason.contains("POST_ONLY")
        );

        if !(cancel_source_matches || reason_matches) {
            return false;
        }

        msg.acc_fill_sz
            .as_ref()
            .is_none_or(|filled| filled == "0" || filled.is_empty())
    }

    fn try_handle_post_only_auto_cancel(
        &mut self,
        msg: &OKXOrderMsg,
        ts_init: UnixNanos,
        exec_reports: &mut Vec<ExecutionReport>,
    ) -> bool {
        if !Self::is_post_only_auto_cancel(msg) {
            return false;
        }

        let Some(client_order_id) = parse_client_order_id(&msg.cl_ord_id) else {
            return false;
        };

        let Some((_, (trader_id, strategy_id, instrument_id))) =
            self.active_client_orders.remove(&client_order_id)
        else {
            return false;
        };

        self.client_id_aliases.remove(&client_order_id);

        if !exec_reports.is_empty() {
            let reports = std::mem::take(exec_reports);
            self.pending_messages
                .push_back(NautilusWsMessage::ExecutionReports(reports));
        }

        let reason = msg
            .cancel_source_reason
            .as_ref()
            .filter(|reason| !reason.is_empty())
            .map_or_else(
                || Ustr::from(OKX_POST_ONLY_CANCEL_REASON),
                |reason| Ustr::from(reason.as_str()),
            );

        let ts_event = parse_millisecond_timestamp(msg.u_time);
        let rejected = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            true,
        );

        self.pending_messages
            .push_back(NautilusWsMessage::OrderRejected(rejected));

        true
    }

    fn register_client_order_aliases(
        &self,
        raw_child: &Option<ClientOrderId>,
        parent_from_msg: &Option<ClientOrderId>,
    ) -> Option<ClientOrderId> {
        if let Some(parent) = parent_from_msg {
            self.client_id_aliases.insert(*parent, *parent);
            if let Some(child) = raw_child.as_ref().filter(|child| **child != *parent) {
                self.client_id_aliases.insert(*child, *parent);
            }
            Some(*parent)
        } else if let Some(child) = raw_child.as_ref() {
            if let Some(mapped) = self.client_id_aliases.get(child) {
                Some(*mapped.value())
            } else {
                self.client_id_aliases.insert(*child, *child);
                Some(*child)
            }
        } else {
            None
        }
    }

    fn adjust_execution_report(
        &self,
        report: ExecutionReport,
        effective_client_id: &Option<ClientOrderId>,
        raw_child: &Option<ClientOrderId>,
    ) -> ExecutionReport {
        match report {
            ExecutionReport::Order(status_report) => {
                let mut adjusted = status_report;
                let mut final_id = *effective_client_id;

                if final_id.is_none() {
                    final_id = adjusted.client_order_id;
                }

                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    if adjusted.client_order_id != Some(final_id_value) {
                        adjusted = adjusted.with_client_order_id(final_id_value);
                    }
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);

                    if let Some(child) =
                        raw_child.as_ref().filter(|child| **child != final_id_value)
                    {
                        adjusted = adjusted.with_linked_order_ids(vec![*child]);
                    }
                }

                ExecutionReport::Order(adjusted)
            }
            ExecutionReport::Fill(mut fill_report) => {
                let mut final_id = *effective_client_id;
                if final_id.is_none() {
                    final_id = fill_report.client_order_id;
                }
                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    fill_report.client_order_id = Some(final_id_value);
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);
                }

                ExecutionReport::Fill(fill_report)
            }
        }
    }

    fn update_caches_with_report(&mut self, report: &ExecutionReport) {
        match report {
            ExecutionReport::Fill(fill_report) => {
                let order_id = fill_report.venue_order_id.inner();
                let current_fee = self
                    .fee_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Money::new(0.0, fill_report.commission.currency));
                let total_fee = current_fee + fill_report.commission;
                self.fee_cache.insert(order_id, total_fee);

                let current_filled_qty = self
                    .filled_qty_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Quantity::zero(fill_report.last_qty.precision));
                let total_filled_qty = current_filled_qty + fill_report.last_qty;
                self.filled_qty_cache.insert(order_id, total_filled_qty);
            }
            ExecutionReport::Order(status_report) => {
                if matches!(status_report.order_status, OrderStatus::Filled) {
                    self.fee_cache.remove(&status_report.venue_order_id.inner());
                    self.filled_qty_cache
                        .remove(&status_report.venue_order_id.inner());
                }

                if matches!(
                    status_report.order_status,
                    OrderStatus::Canceled
                        | OrderStatus::Expired
                        | OrderStatus::Filled
                        | OrderStatus::Rejected,
                ) {
                    self.emitted_accepted.remove(&status_report.venue_order_id);
                    if let Some(client_order_id) = status_report.client_order_id {
                        self.order_state_cache.remove(&client_order_id);
                        self.active_client_orders.remove(&client_order_id);
                        self.client_id_aliases.remove(&client_order_id);
                    }
                    if let Some(linked) = &status_report.linked_order_ids {
                        for child in linked {
                            self.client_id_aliases.remove(child);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_order_response(
        &mut self,
        id: Option<String>,
        op: OKXWsOperation,
        code: String,
        msg: String,
        data: Vec<Value>,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        if code == "0" {
            log::debug!("Order operation successful: id={id:?} op={op} code={code}");

            if op == OKXWsOperation::BatchCancelOrders {
                log::debug!(
                    "Batch cancel operation successful: id={id:?} cancel_count={}",
                    data.len()
                );

                // Check for per-order errors even when top-level code is "0"
                for (idx, entry) in data.iter().enumerate() {
                    if let Some(entry_code) = entry.get("sCode").and_then(|v| v.as_str())
                        && entry_code != "0"
                    {
                        let entry_msg = entry
                            .get("sMsg")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error");

                        if let Some(cl_ord_id_str) = entry
                            .get("clOrdId")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            log::error!(
                                "Batch cancel partial failure for order {cl_ord_id_str}: sCode={entry_code} sMsg={entry_msg}"
                            );
                            // TODO: Emit OrderCancelRejected for this specific order
                        } else {
                            log::error!(
                                "Batch cancel entry[{idx}] failed: sCode={entry_code} sMsg={entry_msg} data={entry:?}"
                            );
                        }
                    }
                }

                return None;
            } else if op == OKXWsOperation::MassCancel
                && let Some(request_id) = &id
                && let Some(instrument_id) = self.pending_mass_cancel_requests.remove(request_id)
            {
                log::debug!("Mass cancel operation successful for instrument: {instrument_id}");
            } else if op == OKXWsOperation::Order
                && let Some(request_id) = &id
                && let Some((params, client_order_id, trader_id, strategy_id, instrument_id)) =
                    self.pending_place_requests.remove(request_id)
            {
                let (venue_order_id, ts_accepted) = if let Some(first) = data.first() {
                    let ord_id = first
                        .get("ordId")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(VenueOrderId::new);

                    let ts = first
                        .get("ts")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map_or_else(
                            || self.clock.get_time_ns(),
                            |ms| UnixNanos::from(ms * 1_000_000),
                        );

                    (ord_id, ts)
                } else {
                    (None, self.clock.get_time_ns())
                };

                if let Some(instrument) = self.instruments_cache.get(&instrument_id.symbol.inner())
                {
                    match params {
                        PendingOrderParams::Regular(order_params) => {
                            let order_type = determine_order_type(
                                order_params.ord_type,
                                order_params.px.as_deref().unwrap_or(""),
                            );

                            let is_explicit_quote_sized = order_params
                                .tgt_ccy
                                .is_some_and(|tgt| tgt == OKXTargetCurrency::QuoteCcy);

                            // SPOT market BUY in cash mode with no tgt_ccy defaults to quote-sizing
                            let is_implicit_quote_sized = order_params.tgt_ccy.is_none()
                                && order_params.side == OKXSide::Buy
                                && order_type == OrderType::Market
                                && order_params.td_mode == OKXTradeMode::Cash
                                && instrument.instrument_class().as_ref() == "SPOT";

                            if is_explicit_quote_sized || is_implicit_quote_sized {
                                // For quote-sized orders, sz is in quote currency (USDT),
                                // not base currency (ETH). We can't accurately parse the
                                // base quantity without the fill price, so we skip the
                                // synthetic OrderAccepted and rely on the orders channel
                                log::debug!(
                                    "Skipping synthetic OrderAccepted for {} quote-sized order: client_order_id={client_order_id}, venue_order_id={venue_order_id:?}",
                                    if is_explicit_quote_sized {
                                        "explicit"
                                    } else {
                                        "implicit"
                                    },
                                );
                                return None;
                            }

                            let Some(v_order_id) = venue_order_id else {
                                log::error!(
                                    "No venue_order_id for accepted order: client_order_id={client_order_id}"
                                );
                                return None;
                            };

                            // Check if already emitted from orders channel push
                            if self.emitted_accepted.contains(&v_order_id) {
                                log::debug!(
                                    "Skipping duplicate OrderAccepted from operation response for venue_order_id={v_order_id}"
                                );
                                return None;
                            }
                            self.emitted_accepted.add(v_order_id);

                            let accepted = OrderAccepted::new(
                                trader_id,
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                v_order_id,
                                self.account_id,
                                UUID4::new(),
                                ts_accepted,
                                ts_init,
                                false, // Not from reconciliation
                            );

                            log::debug!(
                                "Order accepted: client_order_id={client_order_id}, venue_order_id={v_order_id}"
                            );

                            return Some(NautilusWsMessage::OrderAccepted(accepted));
                        }
                        PendingOrderParams::Algo(_) => {
                            log::debug!(
                                "Algo order placement confirmed: client_order_id={client_order_id}, venue_order_id={venue_order_id:?}"
                            );
                        }
                    }
                } else {
                    log::error!("Instrument not found for accepted order: {instrument_id}");
                }
            }

            if let Some(first) = data.first()
                && let Some(success_msg) = first.get("sMsg").and_then(|value| value.as_str())
            {
                log::debug!("Order details: {success_msg}");
            }

            return None;
        }

        let error_msg = data
            .first()
            .and_then(|d| d.get("sMsg"))
            .and_then(|s| s.as_str())
            .unwrap_or(&msg)
            .to_string();

        if let Some(first) = data.first() {
            log::debug!(
                "Error data fields: {}",
                serde_json::to_string_pretty(first)
                    .unwrap_or_else(|_| "unable to serialize".to_string())
            );
        }

        log::warn!("Order operation failed: id={id:?} op={op} code={code} msg={error_msg}");

        let ts_event = self.clock.get_time_ns();

        if let Some(request_id) = &id {
            match op {
                OKXWsOperation::Order => {
                    if let Some((_params, client_order_id, trader_id, strategy_id, instrument_id)) =
                        self.pending_place_requests.remove(request_id)
                    {
                        let due_post_only = is_post_only_rejection(code.as_str(), &data);
                        let rejected = OrderRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            self.account_id,
                            Ustr::from(error_msg.as_str()),
                            UUID4::new(),
                            ts_event,
                            ts_init,
                            false, // Not from reconciliation
                            due_post_only,
                        );

                        return Some(NautilusWsMessage::OrderRejected(rejected));
                    }
                }
                OKXWsOperation::CancelOrder => {
                    if let Some((
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                        venue_order_id,
                    )) = self.pending_cancel_requests.remove(request_id)
                    {
                        let rejected = OrderCancelRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(error_msg.as_str()),
                            UUID4::new(),
                            ts_event,
                            ts_init,
                            false, // Not from reconciliation
                            venue_order_id,
                            Some(self.account_id),
                        );

                        return Some(NautilusWsMessage::OrderCancelRejected(rejected));
                    }
                }
                OKXWsOperation::AmendOrder => {
                    if let Some((
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                        venue_order_id,
                    )) = self.pending_amend_requests.remove(request_id)
                    {
                        let rejected = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(error_msg.as_str()),
                            UUID4::new(),
                            ts_event,
                            ts_init,
                            false, // Not from reconciliation
                            venue_order_id,
                            Some(self.account_id),
                        );

                        return Some(NautilusWsMessage::OrderModifyRejected(rejected));
                    }
                }
                OKXWsOperation::OrderAlgo => {
                    if let Some((_params, client_order_id, trader_id, strategy_id, instrument_id)) =
                        self.pending_place_requests.remove(request_id)
                    {
                        let due_post_only = is_post_only_rejection(code.as_str(), &data);
                        let rejected = OrderRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            self.account_id,
                            Ustr::from(error_msg.as_str()),
                            UUID4::new(),
                            ts_event,
                            ts_init,
                            false, // Not from reconciliation
                            due_post_only,
                        );

                        return Some(NautilusWsMessage::OrderRejected(rejected));
                    }
                }
                OKXWsOperation::CancelAlgos => {
                    if let Some((
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                        venue_order_id,
                    )) = self.pending_cancel_requests.remove(request_id)
                    {
                        let rejected = OrderCancelRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(error_msg.as_str()),
                            UUID4::new(),
                            ts_event,
                            ts_init,
                            false, // Not from reconciliation
                            venue_order_id,
                            Some(self.account_id),
                        );

                        return Some(NautilusWsMessage::OrderCancelRejected(rejected));
                    }
                }
                OKXWsOperation::MassCancel => {
                    if let Some(instrument_id) =
                        self.pending_mass_cancel_requests.remove(request_id)
                    {
                        log::error!(
                            "Mass cancel operation failed for {instrument_id}: code={code} msg={error_msg}"
                        );
                        let error = OKXWebSocketError {
                            code,
                            message: format!("Mass cancel failed for {instrument_id}: {error_msg}"),
                            conn_id: None,
                            timestamp: ts_event.as_u64(),
                        };
                        return Some(NautilusWsMessage::Error(error));
                    } else {
                        log::error!("Mass cancel operation failed: code={code} msg={error_msg}");
                    }
                }
                OKXWsOperation::BatchCancelOrders => {
                    log::warn!(
                        "Batch cancel operation failed: id={id:?} code={code} msg={error_msg} data_count={}",
                        data.len()
                    );

                    // Iterate through data array to check per-order errors
                    for (idx, entry) in data.iter().enumerate() {
                        let entry_code =
                            entry.get("sCode").and_then(|v| v.as_str()).unwrap_or(&code);
                        let entry_msg = entry
                            .get("sMsg")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&error_msg);

                        if entry_code != "0" {
                            // Try to extract client order ID for targeted error events
                            if let Some(cl_ord_id_str) = entry
                                .get("clOrdId")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                            {
                                log::error!(
                                    "Batch cancel failed for order {cl_ord_id_str}: sCode={entry_code} sMsg={entry_msg}"
                                );
                                // TODO: Emit OrderCancelRejected event once we track
                                // batch cancel metadata (client_order_id, trader_id, etc.)
                            } else {
                                log::error!(
                                    "Batch cancel entry[{idx}] failed: sCode={entry_code} sMsg={entry_msg} data={entry:?}"
                                );
                            }
                        }
                    }

                    // Emit generic error for the batch operation
                    let error = OKXWebSocketError {
                        code,
                        message: format!("Batch cancel failed: {error_msg}"),
                        conn_id: None,
                        timestamp: ts_event.as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                _ => log::warn!("Unhandled operation type for rejection: {op}"),
            }
        }

        let error = OKXWebSocketError {
            code,
            message: error_msg,
            conn_id: None,
            timestamp: ts_event.as_u64(),
        };
        Some(NautilusWsMessage::Error(error))
    }

    fn handle_book_data(
        &self,
        arg: OKXWebSocketArg,
        action: OKXBookAction,
        data: Vec<OKXBookMsg>,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(inst_id) = arg.inst_id else {
            log::error!("Instrument ID missing for book data event");
            return None;
        };

        let inst = self.instruments_cache.get(&inst_id)?;

        let instrument_id = inst.id();
        let price_precision = inst.price_precision();
        let size_precision = inst.size_precision();

        match parse_book_msg_vec(
            data,
            &instrument_id,
            price_precision,
            size_precision,
            action,
            ts_init,
        ) {
            Ok(payloads) => Some(NautilusWsMessage::Data(payloads)),
            Err(e) => {
                log::error!("Failed to parse book message: {e}");
                None
            }
        }
    }

    fn handle_account_data(
        &mut self,
        data: Value,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Value::Array(arr) = data else {
            log::error!("Account data is not an array");
            return None;
        };

        let first = arr.into_iter().next()?;

        let account: OKXAccount = match serde_json::from_value(first) {
            Ok(acc) => acc,
            Err(e) => {
                log::error!("Failed to parse account data: {e}");
                return None;
            }
        };

        match parse_account_state(&account, self.account_id, ts_init) {
            Ok(account_state) => {
                if let Some(last_account_state) = &self.last_account_state
                    && account_state.has_same_balances_and_margins(last_account_state)
                {
                    return None;
                }
                self.last_account_state = Some(account_state.clone());
                Some(NautilusWsMessage::AccountUpdate(account_state))
            }
            Err(e) => {
                log::error!("Failed to parse account state: {e}");
                None
            }
        }
    }

    fn handle_positions_data(&mut self, data: Value, ts_init: UnixNanos) {
        match serde_json::from_value::<Vec<OKXPosition>>(data) {
            Ok(positions) => {
                log::debug!("Received {} position update(s)", positions.len());

                for position in positions {
                    let instrument = match self.instruments_cache.get(&position.inst_id) {
                        Some(inst) => inst,
                        None => {
                            log::warn!(
                                "Received position update for unknown instrument {}, skipping",
                                position.inst_id
                            );
                            continue;
                        }
                    };

                    let instrument_id = instrument.id();
                    let size_precision = instrument.size_precision();

                    match parse_position_status_report(
                        position,
                        self.account_id,
                        instrument_id,
                        size_precision,
                        ts_init,
                    ) {
                        Ok(position_report) => {
                            self.pending_messages
                                .push_back(NautilusWsMessage::PositionUpdate(position_report));
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to parse position status report for {instrument_id}: {e}"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to parse positions data: {e}");
            }
        }
    }

    fn handle_orders_data(&mut self, data: Value, ts_init: UnixNanos) -> Option<NautilusWsMessage> {
        let orders: Vec<OKXOrderMsg> = match serde_json::from_value(data) {
            Ok(orders) => orders,
            Err(e) => {
                log::error!("Failed to deserialize orders channel payload: {e}");
                return None;
            }
        };

        log::debug!(
            "Received {} order message(s) from orders channel",
            orders.len()
        );

        let mut exec_reports: Vec<ExecutionReport> = Vec::with_capacity(orders.len());

        for msg in orders {
            log::debug!(
                "Processing order message: inst_id={}, cl_ord_id={}, state={:?}, exec_type={:?}",
                msg.inst_id,
                msg.cl_ord_id,
                msg.state,
                msg.exec_type
            );

            if self.try_handle_post_only_auto_cancel(&msg, ts_init, &mut exec_reports) {
                continue;
            }

            let raw_child = parse_client_order_id(&msg.cl_ord_id);
            let parent_from_msg = msg
                .algo_cl_ord_id
                .as_ref()
                .filter(|value| !value.is_empty())
                .map(ClientOrderId::new);
            let effective_client_id =
                self.register_client_order_aliases(&raw_child, &parent_from_msg);

            let Some(instrument) = self.instruments_cache.get(&msg.inst_id) else {
                log::error!(
                    "No instrument found for inst_id: {inst_id}",
                    inst_id = msg.inst_id
                );
                continue;
            };
            let price_precision = instrument.price_precision();
            let size_precision = instrument.size_precision();

            let order_metadata = effective_client_id
                .and_then(|cid| self.active_client_orders.get(&cid).map(|e| *e.value()));

            let previous_fee = self.fee_cache.get(&msg.ord_id).copied();
            let previous_filled_qty = self.filled_qty_cache.get(&msg.ord_id).copied();
            let previous_state =
                effective_client_id.and_then(|cid| self.order_state_cache.get(&cid).cloned());

            // SAFETY: order_metadata being Some implies effective_client_id is Some
            if let (Some((trader_id, strategy_id, _instrument_id)), Some(canonical_client_id)) =
                (order_metadata, effective_client_id)
            {
                match parse_order_event(
                    &msg,
                    canonical_client_id,
                    self.account_id,
                    trader_id,
                    strategy_id,
                    instrument,
                    previous_fee,
                    previous_filled_qty,
                    previous_state.as_ref(),
                    ts_init,
                ) {
                    Ok(event) => {
                        self.process_parsed_order_event(
                            event,
                            &msg,
                            price_precision,
                            size_precision,
                            canonical_client_id,
                            &raw_child,
                            &mut exec_reports,
                        );
                    }
                    Err(e) => log::error!("Failed to parse order event: {e}"),
                }
            } else {
                // External order or not tracked - use old parse_order_msg for backward compatibility
                match parse_order_msg(
                    &msg,
                    self.account_id,
                    &self.instruments_cache,
                    &self.fee_cache,
                    &self.filled_qty_cache,
                    ts_init,
                ) {
                    Ok(report) => {
                        log::debug!("Parsed external order as execution report: {report:?}");
                        let adjusted =
                            self.adjust_execution_report(report, &effective_client_id, &raw_child);
                        self.update_caches_with_report(&adjusted);
                        exec_reports.push(adjusted);
                    }
                    Err(e) => log::error!("Failed to parse order message: {e}"),
                }
            }
        }

        if !exec_reports.is_empty() {
            log::debug!(
                "Pushing {count} execution report(s) to message queue",
                count = exec_reports.len()
            );
            self.pending_messages
                .push_back(NautilusWsMessage::ExecutionReports(exec_reports));
        }

        self.pending_messages.pop_front()
    }

    /// Processes a parsed order event and emits the appropriate message.
    #[allow(clippy::too_many_arguments)]
    fn process_parsed_order_event(
        &mut self,
        event: ParsedOrderEvent,
        msg: &OKXOrderMsg,
        price_precision: u8,
        size_precision: u8,
        canonical_client_id: ClientOrderId,
        raw_child: &Option<ClientOrderId>,
        exec_reports: &mut Vec<ExecutionReport>,
    ) {
        let venue_order_id = VenueOrderId::new(msg.ord_id);

        match event {
            ParsedOrderEvent::Accepted(accepted) => {
                if self.emitted_accepted.contains(&venue_order_id) {
                    log::debug!(
                        "Skipping duplicate OrderAccepted for venue_order_id={venue_order_id}"
                    );
                    return;
                }
                self.emitted_accepted.add(venue_order_id);
                self.update_order_state_cache(
                    &canonical_client_id,
                    msg,
                    price_precision,
                    size_precision,
                );

                self.pending_messages
                    .push_back(NautilusWsMessage::OrderAccepted(accepted));
            }
            ParsedOrderEvent::Canceled(canceled) => {
                self.cleanup_terminal_order(&canonical_client_id, &venue_order_id);
                self.pending_messages
                    .push_back(NautilusWsMessage::OrderCanceled(canceled));
            }
            ParsedOrderEvent::Expired(expired) => {
                self.cleanup_terminal_order(&canonical_client_id, &venue_order_id);
                self.pending_messages
                    .push_back(NautilusWsMessage::OrderExpired(expired));
            }
            ParsedOrderEvent::Triggered(triggered) => {
                self.update_order_state_cache(
                    &canonical_client_id,
                    msg,
                    price_precision,
                    size_precision,
                );
                self.pending_messages
                    .push_back(NautilusWsMessage::OrderTriggered(triggered));
            }
            ParsedOrderEvent::Updated(updated) => {
                self.update_order_state_cache(
                    &canonical_client_id,
                    msg,
                    price_precision,
                    size_precision,
                );
                self.pending_messages
                    .push_back(NautilusWsMessage::OrderUpdated(updated));
            }
            ParsedOrderEvent::Fill(fill_report) => {
                let effective_client_id = Some(canonical_client_id);
                let adjusted = self.adjust_execution_report(
                    ExecutionReport::Fill(fill_report),
                    &effective_client_id,
                    raw_child,
                );
                self.update_caches_with_report(&adjusted);

                if msg.state == OKXOrderStatus::Filled {
                    self.cleanup_terminal_order(&canonical_client_id, &venue_order_id);
                }

                exec_reports.push(adjusted);
            }
            ParsedOrderEvent::StatusOnly(status_report) => {
                let effective_client_id = Some(canonical_client_id);
                let adjusted = self.adjust_execution_report(
                    ExecutionReport::Order(*status_report),
                    &effective_client_id,
                    raw_child,
                );
                self.update_caches_with_report(&adjusted);
                exec_reports.push(adjusted);
            }
        }
    }

    /// Updates the order state cache for detecting future updates.
    fn update_order_state_cache(
        &mut self,
        client_order_id: &ClientOrderId,
        msg: &OKXOrderMsg,
        price_precision: u8,
        size_precision: u8,
    ) {
        let venue_order_id = VenueOrderId::new(msg.ord_id);
        let quantity = parse_quantity(&msg.sz, size_precision).ok();
        let price = if is_market_price(&msg.px) {
            None
        } else {
            parse_price(&msg.px, price_precision).ok()
        };

        if let Some(qty) = quantity {
            self.order_state_cache.insert(
                *client_order_id,
                OrderStateSnapshot {
                    venue_order_id,
                    quantity: qty,
                    price,
                },
            );
        }
    }

    /// Cleans up tracking state for terminal orders.
    fn cleanup_terminal_order(
        &mut self,
        client_order_id: &ClientOrderId,
        venue_order_id: &VenueOrderId,
    ) {
        self.emitted_accepted.remove(venue_order_id);
        self.order_state_cache.remove(client_order_id);
        self.active_client_orders.remove(client_order_id);
        self.client_id_aliases.remove(client_order_id);
        self.client_id_aliases.retain(|_, v| *v != *client_order_id);

        self.fee_cache.remove(&venue_order_id.inner());
        self.filled_qty_cache.remove(&venue_order_id.inner());
    }

    fn handle_algo_orders_data(
        &mut self,
        data: Value,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let orders: Vec<OKXAlgoOrderMsg> = match serde_json::from_value(data) {
            Ok(orders) => orders,
            Err(e) => {
                log::error!("Failed to deserialize algo orders payload: {e}");
                return None;
            }
        };

        let mut exec_reports: Vec<ExecutionReport> = Vec::with_capacity(orders.len());

        for msg in orders {
            let raw_child = parse_client_order_id(&msg.cl_ord_id);
            let parent_from_msg = parse_client_order_id(&msg.algo_cl_ord_id);
            let effective_client_id =
                self.register_client_order_aliases(&raw_child, &parent_from_msg);

            match parse_algo_order_msg(msg, self.account_id, &self.instruments_cache, ts_init) {
                Ok(report) => {
                    let adjusted =
                        self.adjust_execution_report(report, &effective_client_id, &raw_child);
                    self.update_caches_with_report(&adjusted);
                    exec_reports.push(adjusted);
                }
                Err(e) => {
                    log::error!("Failed to parse algo order message: {e}");
                }
            }
        }

        if exec_reports.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::ExecutionReports(exec_reports))
        }
    }

    fn handle_other_channel_data(
        &mut self,
        channel: OKXWsChannel,
        inst_id: Option<Ustr>,
        data: Value,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(inst_id) = inst_id else {
            log::error!("No instrument for channel {channel:?}");
            return None;
        };

        let Some(instrument) = self.instruments_cache.get(&inst_id) else {
            log::error!("No instrument for channel {channel:?}, inst_id {inst_id:?}");
            return None;
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        match parse_ws_message_data(
            &channel,
            data,
            &instrument_id,
            price_precision,
            size_precision,
            ts_init,
            &mut self.funding_rate_cache,
            &self.instruments_cache,
        ) {
            Ok(Some(msg)) => {
                if let NautilusWsMessage::Instrument(ref inst) = msg {
                    self.instruments_cache
                        .insert(inst.symbol().inner(), inst.as_ref().clone());
                }
                Some(msg)
            }
            Ok(None) => None,
            Err(e) => {
                log::error!("Error parsing message for channel {channel:?}: {e}");
                None
            }
        }
    }

    pub(crate) fn parse_raw_message(
        msg: tokio_tungstenite::tungstenite::Message,
    ) -> Option<OKXWsMessage> {
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                if text == TEXT_PONG {
                    log::trace!("Received pong from OKX");
                    return None;
                }
                if text == TEXT_PING {
                    log::trace!("Received ping from OKX (text)");
                    return Some(OKXWsMessage::Ping);
                }

                if text == RECONNECTED {
                    log::debug!("Received WebSocket reconnection signal");
                    return Some(OKXWsMessage::Reconnected);
                }
                log::trace!("Received WebSocket message: {text}");

                match serde_json::from_str(&text) {
                    Ok(ws_event) => match &ws_event {
                        OKXWsMessage::Error { code, msg } => {
                            log::error!("WebSocket error: {code} - {msg}");
                            Some(ws_event)
                        }
                        OKXWsMessage::Login {
                            event,
                            code,
                            msg,
                            conn_id,
                        } => {
                            if code == "0" {
                                log::info!("WebSocket authenticated: conn_id={conn_id}");
                            } else {
                                log::error!(
                                    "WebSocket authentication failed: event={event}, code={code}, error={msg}"
                                );
                            }
                            Some(ws_event)
                        }
                        OKXWsMessage::Subscription {
                            event,
                            arg,
                            conn_id,
                            ..
                        } => {
                            let channel_str = serde_json::to_string(&arg.channel)
                                .expect("Invalid OKX websocket channel")
                                .trim_matches('"')
                                .to_string();
                            log::debug!("{event}d: channel={channel_str}, conn_id={conn_id}");
                            Some(ws_event)
                        }
                        OKXWsMessage::ChannelConnCount {
                            event: _,
                            channel,
                            conn_count,
                            conn_id,
                        } => {
                            let channel_str = serde_json::to_string(&channel)
                                .expect("Invalid OKX websocket channel")
                                .trim_matches('"')
                                .to_string();
                            log::debug!(
                                "Channel connection status: channel={channel_str}, connections={conn_count}, conn_id={conn_id}",
                            );
                            None
                        }
                        OKXWsMessage::Ping => {
                            log::trace!("Ignoring ping event parsed from text payload");
                            None
                        }
                        OKXWsMessage::Data { .. } => Some(ws_event),
                        OKXWsMessage::BookData { .. } => Some(ws_event),
                        OKXWsMessage::OrderResponse {
                            id,
                            op,
                            code,
                            msg: _,
                            data,
                        } => {
                            if code == "0" {
                                log::debug!(
                                    "Order operation successful: id={id:?}, op={op}, code={code}"
                                );

                                if let Some(order_data) = data.first() {
                                    let success_msg = order_data
                                        .get("sMsg")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("Order operation successful");
                                    log::debug!("Order success details: {success_msg}");
                                }
                            }
                            Some(ws_event)
                        }
                        OKXWsMessage::Reconnected => {
                            // This shouldn't happen as we handle RECONNECTED string directly
                            log::warn!("Unexpected Reconnected event from deserialization");
                            None
                        }
                    },
                    Err(e) => {
                        log::error!("Failed to parse message: {e}: {text}");
                        None
                    }
                }
            }
            Message::Ping(_payload) => {
                log::trace!("Received binary ping frame from OKX");
                Some(OKXWsMessage::Ping)
            }
            Message::Pong(payload) => {
                log::trace!("Received pong frame from OKX ({} bytes)", payload.len());
                None
            }
            Message::Binary(msg) => {
                log::debug!("Raw binary: {msg:?}");
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message");
                None
            }
            msg => {
                log::warn!("Unexpected message: {msg}");
                None
            }
        }
    }

    fn generate_unique_request_id(&self) -> String {
        self.request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string()
    }

    fn get_instrument_type_and_family_from_instrument(
        instrument: &InstrumentAny,
    ) -> anyhow::Result<(OKXInstrumentType, String)> {
        let inst_type = okx_instrument_type(instrument)?;
        let symbol = instrument.symbol().inner();

        // Determine instrument family based on instrument type
        let inst_family = match instrument {
            InstrumentAny::CurrencyPair(_) => symbol.as_str().to_string(),
            InstrumentAny::CryptoPerpetual(_) => {
                // For SWAP: "BTC-USDT-SWAP" -> "BTC-USDT"
                symbol
                    .as_str()
                    .strip_suffix("-SWAP")
                    .unwrap_or(symbol.as_str())
                    .to_string()
            }
            InstrumentAny::CryptoFuture(_) => {
                // For FUTURES: "BTC-USDT-250328" -> "BTC-USDT"
                // Extract the base pair by removing date suffix
                let s = symbol.as_str();
                if let Some(idx) = s.rfind('-') {
                    s[..idx].to_string()
                } else {
                    s.to_string()
                }
            }
            _ => {
                anyhow::bail!("Unsupported instrument type for OKX");
            }
        };

        Ok((inst_type, inst_family))
    }

    async fn handle_mass_cancel(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .instruments_cache
            .get(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Unknown instrument {instrument_id}"))?;

        let (inst_type, inst_family) =
            Self::get_instrument_type_and_family_from_instrument(instrument)?;

        let params = WsMassCancelParams {
            inst_type,
            inst_family: Ustr::from(&inst_family),
        };

        let args =
            vec![serde_json::to_value(params).map_err(|e| anyhow::anyhow!("JSON error: {e}"))?];

        let request_id = self.generate_unique_request_id();

        self.pending_mass_cancel_requests
            .insert(request_id.clone(), instrument_id);

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::MassCancel,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize mass cancel request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_CANCEL.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent mass cancel for {instrument_id}");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send mass cancel after retries: error={e}");

                self.pending_mass_cancel_requests.remove(&request_id);

                let error = OKXWebSocketError {
                    code: "CLIENT_ERROR".to_string(),
                    message: format!("Mass cancel failed for {instrument_id}: {e}"),
                    conn_id: None,
                    timestamp: self.clock.get_time_ns().as_u64(),
                };
                let _ = self.send(NautilusWsMessage::Error(error));

                Err(anyhow::anyhow!("Failed to send mass cancel: {e}"))
            }
        }
    }

    async fn handle_batch_cancel_orders(
        &self,
        args: Vec<Value>,
        request_id: String,
    ) -> anyhow::Result<()> {
        let request = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchCancelOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize batch cancel request: {e}"))?;

        if let Some(client) = &self.inner {
            client
                .send_text(payload, Some(OKX_RATE_LIMIT_KEY_CANCEL.as_slice()))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send batch cancel: {e}"))?;
            log::debug!("Sent batch cancel orders");
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active WebSocket client"))
        }
    }

    async fn handle_batch_place_orders(
        &self,
        args: Vec<Value>,
        request_id: String,
    ) -> anyhow::Result<()> {
        let request = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize batch place request: {e}"))?;

        if let Some(client) = &self.inner {
            client
                .send_text(payload, Some(OKX_RATE_LIMIT_KEY_ORDER.as_slice()))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send batch place: {e}"))?;
            log::debug!("Sent batch place orders");
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active WebSocket client"))
        }
    }

    async fn handle_batch_amend_orders(
        &self,
        args: Vec<Value>,
        request_id: String,
    ) -> anyhow::Result<()> {
        let request = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchAmendOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize batch amend request: {e}"))?;

        if let Some(client) = &self.inner {
            client
                .send_text(payload, Some(OKX_RATE_LIMIT_KEY_AMEND.as_slice()))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send batch amend: {e}"))?;
            log::debug!("Sent batch amend orders");
            Ok(())
        } else {
            Err(anyhow::anyhow!("No active WebSocket client"))
        }
    }

    async fn handle_subscribe(&self, args: Vec<OKXSubscriptionArg>) -> anyhow::Result<()> {
        for arg in &args {
            log::debug!(
                "Subscribing to channel: channel={:?}, inst_id={:?}",
                arg.channel,
                arg.inst_id
            );
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Subscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize subscription: {e}"))?;

        self.send_with_retry(json_txt, Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send subscription after retries: {e}"))?;
        Ok(())
    }

    async fn handle_unsubscribe(&self, args: Vec<OKXSubscriptionArg>) -> anyhow::Result<()> {
        for arg in &args {
            log::debug!(
                "Unsubscribing from channel: channel={:?}, inst_id={:?}",
                arg.channel,
                arg.inst_id
            );
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Unsubscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize unsubscription: {e}"))?;

        self.send_with_retry(json_txt, Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscription after retries: {e}"))?;
        Ok(())
    }

    async fn handle_place_order(
        &mut self,
        params: WsPostOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let request_id = self.generate_unique_request_id();

        self.pending_place_requests.insert(
            request_id.clone(),
            (
                PendingOrderParams::Regular(params.clone()),
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            ),
        );

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::Order,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize place order request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_ORDER.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent place order request");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send place order after retries: error={e}");

                self.pending_place_requests.remove(&request_id);

                let ts_now = self.clock.get_time_ns();
                let rejected = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    self.account_id,
                    Ustr::from(&format!("WebSocket send failed: {e}")),
                    UUID4::new(),
                    ts_now, // ts_event
                    ts_now, // ts_init
                    false,  // Not from reconciliation
                    false,  // Not due to post-only
                );
                let _ = self.send(NautilusWsMessage::OrderRejected(rejected));

                Err(anyhow::anyhow!("Failed to send place order: {e}"))
            }
        }
    }

    async fn handle_place_algo_order(
        &mut self,
        params: WsPostAlgoOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let request_id = self.generate_unique_request_id();

        self.pending_place_requests.insert(
            request_id.clone(),
            (
                PendingOrderParams::Algo(params.clone()),
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            ),
        );

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::OrderAlgo,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize place algo order request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_ORDER.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent place algo order request");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send place algo order after retries: error={e}");

                self.pending_place_requests.remove(&request_id);

                let ts_now = self.clock.get_time_ns();
                let rejected = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    self.account_id,
                    Ustr::from(&format!("WebSocket send failed: {e}")),
                    UUID4::new(),
                    ts_now, // ts_event
                    ts_now, // ts_init
                    false,  // Not from reconciliation
                    false,  // Not due to post-only
                );
                let _ = self.send(NautilusWsMessage::OrderRejected(rejected));

                Err(anyhow::anyhow!("Failed to send place algo order: {e}"))
            }
        }
    }

    async fn handle_cancel_order(
        &mut self,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) -> anyhow::Result<()> {
        let mut builder = WsCancelOrderParamsBuilder::default();
        builder.inst_id(instrument_id.symbol.as_str());

        if let Some(venue_order_id) = venue_order_id {
            builder.ord_id(venue_order_id.as_str());
        }

        if let Some(client_order_id) = client_order_id {
            builder.cl_ord_id(client_order_id.as_str());
        }

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build cancel params: {e}"))?;

        let request_id = self.generate_unique_request_id();

        // Track pending request if we have a client order ID
        if let Some(client_order_id) = client_order_id {
            self.pending_cancel_requests.insert(
                request_id.clone(),
                (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                ),
            );
        }

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::CancelOrder,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize cancel request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_CANCEL.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent cancel order request");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send cancel order after retries: error={e}");

                self.pending_cancel_requests.remove(&request_id);

                if let Some(client_order_id) = client_order_id {
                    let ts_now = self.clock.get_time_ns();
                    let rejected = OrderCancelRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Ustr::from(&format!("WebSocket send failed: {e}")),
                        UUID4::new(),
                        ts_now, // ts_event
                        ts_now, // ts_init
                        false,  // Not from reconciliation
                        venue_order_id,
                        Some(self.account_id),
                    );
                    let _ = self.send(NautilusWsMessage::OrderCancelRejected(rejected));
                }

                Err(anyhow::anyhow!("Failed to send cancel order: {e}"))
            }
        }
    }

    async fn handle_amend_order(
        &mut self,
        params: WsAmendOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<()> {
        let request_id = self.generate_unique_request_id();

        self.pending_amend_requests.insert(
            request_id.clone(),
            (
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            ),
        );

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::AmendOrder,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize amend order request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_AMEND.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent amend order request");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send amend order after retries: error={e}");

                self.pending_amend_requests.remove(&request_id);

                let ts_now = self.clock.get_time_ns();
                let rejected = OrderModifyRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Ustr::from(&format!("WebSocket send failed: {e}")),
                    UUID4::new(),
                    ts_now, // ts_event
                    ts_now, // ts_init
                    false,  // Not from reconciliation
                    venue_order_id,
                    Some(self.account_id),
                );
                let _ = self.send(NautilusWsMessage::OrderModifyRejected(rejected));

                Err(anyhow::anyhow!("Failed to send amend order: {e}"))
            }
        }
    }

    async fn handle_cancel_algo_order(
        &mut self,
        client_order_id: Option<ClientOrderId>,
        algo_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) -> anyhow::Result<()> {
        let mut builder = WsCancelAlgoOrderParamsBuilder::default();
        builder.inst_id(instrument_id.symbol.as_str());

        if let Some(client_order_id) = &client_order_id {
            builder.algo_cl_ord_id(client_order_id.as_str());
        }

        if let Some(algo_id) = &algo_order_id {
            builder.algo_id(algo_id.as_str());
        }

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build cancel algo params: {e}"))?;

        let request_id = self.generate_unique_request_id();

        // Track pending cancellation if we have a client order ID
        if let Some(client_order_id) = client_order_id {
            self.pending_cancel_requests.insert(
                request_id.clone(),
                (client_order_id, trader_id, strategy_id, instrument_id, None),
            );
        }

        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: OKXWsOperation::CancelAlgos,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize cancel algo request: {e}"))?;

        match self
            .send_with_retry(payload, Some(OKX_RATE_LIMIT_KEY_CANCEL.as_slice()))
            .await
        {
            Ok(()) => {
                log::debug!("Sent cancel algo order request");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to send cancel algo order after retries: error={e}");

                self.pending_cancel_requests.remove(&request_id);

                if let Some(client_order_id) = client_order_id {
                    let ts_now = self.clock.get_time_ns();
                    let rejected = OrderCancelRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Ustr::from(&format!("WebSocket send failed: {e}")),
                        UUID4::new(),
                        ts_now, // ts_event
                        ts_now, // ts_init
                        false,  // Not from reconciliation
                        None,
                        Some(self.account_id),
                    );
                    let _ = self.send(NautilusWsMessage::OrderCancelRejected(rejected));
                }

                Err(anyhow::anyhow!("Failed to send cancel algo order: {e}"))
            }
        }
    }
}

/// Returns `true` when an OKX error payload represents a post-only rejection.
pub fn is_post_only_rejection(code: &str, data: &[Value]) -> bool {
    if code == OKX_POST_ONLY_ERROR_CODE {
        return true;
    }

    for entry in data {
        if let Some(s_code) = entry.get("sCode").and_then(|value| value.as_str())
            && s_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }

        if let Some(inner_code) = entry.get("code").and_then(|value| value.as_str())
            && inner_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }
    }

    false
}

/// Case-insensitive substring check.
#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Determines if an OKX WebSocket error should trigger a retry.
fn should_retry_okx_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => should_retry_error_code(error_code),
        OKXWsError::TungsteniteError(_) => true, // Network errors are retryable
        OKXWsError::ClientError(msg) => {
            // Retry on timeout and connection errors
            contains_ignore_ascii_case(msg, "timeout")
                || contains_ignore_ascii_case(msg, "timed out")
                || contains_ignore_ascii_case(msg, "connection")
                || contains_ignore_ascii_case(msg, "network")
        }
        OKXWsError::AuthenticationError(_)
        | OKXWsError::JsonError(_)
        | OKXWsError::ParsingError(_) => {
            // Don't retry authentication or parsing errors automatically
            false
        }
    }
}

/// Creates a timeout error for the retry manager.
fn create_okx_timeout_error(msg: String) -> OKXWsError {
    OKXWsError::ClientError(msg)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use dashmap::DashMap;
    use nautilus_model::{
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
        types::{Money, Quantity},
    };
    use nautilus_network::websocket::{AuthTracker, SubscriptionState};
    use rstest::rstest;

    use super::{NautilusWsMessage, OKXWsFeedHandler};
    use crate::websocket::parse::OrderStateSnapshot;

    const OKX_WS_TOPIC_DELIMITER: char = ':';

    #[allow(clippy::type_complexity)]
    fn create_test_handler() -> (
        OKXWsFeedHandler,
        tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
        Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
        Arc<DashMap<ClientOrderId, ClientOrderId>>,
    ) {
        let account_id = AccountId::new("OKX-001");
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        let active_client_orders = Arc::new(DashMap::new());
        let client_id_aliases = Arc::new(DashMap::new());
        let auth_tracker = AuthTracker::new();
        let subscriptions_state = SubscriptionState::new(OKX_WS_TOPIC_DELIMITER);

        let handler = OKXWsFeedHandler::new(
            account_id,
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            active_client_orders.clone(),
            client_id_aliases.clone(),
            auth_tracker,
            subscriptions_state,
        );

        (handler, out_rx, active_client_orders, client_id_aliases)
    }

    #[rstest]
    fn test_is_post_only_rejection_detects_by_code() {
        assert!(super::is_post_only_rejection("51019", &[]));
    }

    #[rstest]
    fn test_is_post_only_rejection_detects_by_inner_code() {
        let data = vec![serde_json::json!({
            "sCode": "51019"
        })];
        assert!(super::is_post_only_rejection("50000", &data));
    }

    #[rstest]
    fn test_is_post_only_rejection_false_for_unrelated_error() {
        let data = vec![serde_json::json!({
            "sMsg": "Insufficient balance"
        })];
        assert!(!super::is_post_only_rejection("50000", &data));
    }

    #[rstest]
    fn test_handler_register_client_order_aliases_with_parent() {
        let (handler, _out_rx, _active, client_id_aliases) = create_test_handler();

        let child = Some(ClientOrderId::new("CHILD-001"));
        let parent = Some(ClientOrderId::new("PARENT-001"));

        let result = handler.register_client_order_aliases(&child, &parent);

        assert_eq!(result, Some(ClientOrderId::new("PARENT-001")));
        assert!(client_id_aliases.contains_key(&ClientOrderId::new("PARENT-001")));
        assert!(client_id_aliases.contains_key(&ClientOrderId::new("CHILD-001")));
        assert_eq!(
            *client_id_aliases
                .get(&ClientOrderId::new("CHILD-001"))
                .unwrap(),
            ClientOrderId::new("PARENT-001")
        );
    }

    #[rstest]
    fn test_handler_register_client_order_aliases_without_parent() {
        let (handler, _out_rx, _active, client_id_aliases) = create_test_handler();

        let child = Some(ClientOrderId::new("ORDER-001"));
        let parent: Option<ClientOrderId> = None;

        let result = handler.register_client_order_aliases(&child, &parent);

        assert_eq!(result, Some(ClientOrderId::new("ORDER-001")));
        assert!(client_id_aliases.contains_key(&ClientOrderId::new("ORDER-001")));
        assert_eq!(
            *client_id_aliases
                .get(&ClientOrderId::new("ORDER-001"))
                .unwrap(),
            ClientOrderId::new("ORDER-001")
        );
    }

    #[rstest]
    fn test_handler_cleanup_terminal_order_removes_all_state() {
        let (mut handler, _out_rx, active_client_orders, client_id_aliases) = create_test_handler();

        let canonical = ClientOrderId::new("PARENT-001");
        let child = ClientOrderId::new("CHILD-001");
        let venue_id = VenueOrderId::new("VENUE-001");
        let trader_id = TraderId::new("TRADER-001");
        let strategy_id = StrategyId::new("STRATEGY-001");
        let instrument_id = InstrumentId::from("ETH-USDT-PERP.OKX");

        active_client_orders.insert(canonical, (trader_id, strategy_id, instrument_id));
        client_id_aliases.insert(canonical, canonical);
        client_id_aliases.insert(child, canonical);
        handler
            .fee_cache
            .insert(venue_id.inner(), Money::from("0.001 USDT"));
        handler
            .filled_qty_cache
            .insert(venue_id.inner(), Quantity::from("1.0"));
        handler.order_state_cache.insert(
            canonical,
            OrderStateSnapshot {
                venue_order_id: venue_id,
                quantity: Quantity::from("1.0"),
                price: None,
            },
        );

        handler.cleanup_terminal_order(&canonical, &venue_id);

        assert!(!active_client_orders.contains_key(&canonical));
        assert!(!client_id_aliases.contains_key(&canonical));
        assert!(!client_id_aliases.contains_key(&child));
        assert!(!handler.fee_cache.contains_key(&venue_id.inner()));
        assert!(!handler.filled_qty_cache.contains_key(&venue_id.inner()));
        assert!(!handler.order_state_cache.contains_key(&canonical));
    }

    #[rstest]
    fn test_handler_cleanup_terminal_order_removes_multiple_children() {
        let (mut handler, _out_rx, _active, client_id_aliases) = create_test_handler();

        let canonical = ClientOrderId::new("PARENT-001");
        let child1 = ClientOrderId::new("CHILD-001");
        let child2 = ClientOrderId::new("CHILD-002");
        let child3 = ClientOrderId::new("CHILD-003");
        let venue_id = VenueOrderId::new("VENUE-001");

        client_id_aliases.insert(canonical, canonical);
        client_id_aliases.insert(child1, canonical);
        client_id_aliases.insert(child2, canonical);
        client_id_aliases.insert(child3, canonical);

        handler.cleanup_terminal_order(&canonical, &venue_id);

        assert!(!client_id_aliases.contains_key(&canonical));
        assert!(!client_id_aliases.contains_key(&child1));
        assert!(!client_id_aliases.contains_key(&child2));
        assert!(!client_id_aliases.contains_key(&child3));
        assert!(client_id_aliases.is_empty());
    }

    #[rstest]
    fn test_handler_cleanup_does_not_affect_other_orders() {
        let (mut handler, _out_rx, active_client_orders, client_id_aliases) = create_test_handler();

        let canonical1 = ClientOrderId::new("PARENT-001");
        let child1 = ClientOrderId::new("CHILD-001");
        let venue_id1 = VenueOrderId::new("VENUE-001");

        let canonical2 = ClientOrderId::new("PARENT-002");
        let child2 = ClientOrderId::new("CHILD-002");
        let venue_id2 = VenueOrderId::new("VENUE-002");

        let trader_id = TraderId::new("TRADER-001");
        let strategy_id = StrategyId::new("STRATEGY-001");
        let instrument_id = InstrumentId::from("ETH-USDT-PERP.OKX");

        active_client_orders.insert(canonical1, (trader_id, strategy_id, instrument_id));
        active_client_orders.insert(canonical2, (trader_id, strategy_id, instrument_id));
        client_id_aliases.insert(canonical1, canonical1);
        client_id_aliases.insert(child1, canonical1);
        client_id_aliases.insert(canonical2, canonical2);
        client_id_aliases.insert(child2, canonical2);
        handler
            .fee_cache
            .insert(venue_id1.inner(), Money::from("0.001 USDT"));
        handler
            .fee_cache
            .insert(venue_id2.inner(), Money::from("0.002 USDT"));

        handler.cleanup_terminal_order(&canonical1, &venue_id1);

        assert!(!active_client_orders.contains_key(&canonical1));
        assert!(!client_id_aliases.contains_key(&canonical1));
        assert!(!client_id_aliases.contains_key(&child1));
        assert!(!handler.fee_cache.contains_key(&venue_id1.inner()));

        assert!(active_client_orders.contains_key(&canonical2));
        assert!(client_id_aliases.contains_key(&canonical2));
        assert!(client_id_aliases.contains_key(&child2));
        assert!(handler.fee_cache.contains_key(&venue_id2.inner()));
    }

    mod channel_routing {
        use nautilus_core::nanos::UnixNanos;
        use nautilus_model::{
            identifiers::{InstrumentId, Symbol},
            instruments::{CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
            types::{Currency, Price, Quantity},
        };
        use rstest::rstest;
        use ustr::Ustr;

        use super::*;
        use crate::{
            common::{enums::OKXBookAction, testing::load_test_json},
            websocket::{enums::OKXWsChannel, messages::OKXWsMessage},
        };

        fn create_spot_instrument() -> InstrumentAny {
            let instrument_id = InstrumentId::from("BTC-USDT.OKX");
            InstrumentAny::CurrencyPair(CurrencyPair::new(
                instrument_id,
                Symbol::from("BTC-USDT"),
                Currency::BTC(),
                Currency::USDT(),
                2,
                8,
                Price::from("0.01"),
                Quantity::from("0.00000001"),
                None, // multiplier
                None, // lot_size
                None, // max_quantity
                None, // min_quantity
                None, // max_notional
                None, // min_notional
                None, // max_price
                None, // min_price
                None, // margin_init
                None, // margin_maint
                None, // maker_fee
                None, // taker_fee
                UnixNanos::default(),
                UnixNanos::default(),
            ))
        }

        fn create_swap_instrument() -> InstrumentAny {
            let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
            InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
                instrument_id,
                Symbol::from("BTC-USDT-SWAP"),
                Currency::BTC(),
                Currency::USDT(),
                Currency::USDT(),
                false,
                2,
                8,
                Price::from("0.01"),
                Quantity::from("0.00000001"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                UnixNanos::default(),
                UnixNanos::default(),
            ))
        }

        fn create_handler_with_instruments(instruments: Vec<InstrumentAny>) -> OKXWsFeedHandler {
            let (mut handler, _, _, _) = create_test_handler();
            for inst in instruments {
                handler
                    .instruments_cache
                    .insert(inst.symbol().inner(), inst);
            }
            handler
        }

        #[rstest]
        fn test_parse_raw_message_ticker_channel() {
            let json = load_test_json("ws_tickers.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::Data { arg, data } => {
                    assert!(
                        matches!(arg.channel, OKXWsChannel::Tickers),
                        "Expected Tickers channel"
                    );
                    assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                    assert!(data.is_array());
                }
                _ => panic!("Expected OKXWsMessage::Data variant"),
            }
        }

        #[rstest]
        fn test_parse_raw_message_trades_channel() {
            let json = load_test_json("ws_trades.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::Data { arg, data } => {
                    assert!(
                        matches!(arg.channel, OKXWsChannel::Trades),
                        "Expected Trades channel"
                    );
                    assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USD")));
                    assert!(data.is_array());
                }
                _ => panic!("Expected OKXWsMessage::Data variant"),
            }
        }

        #[rstest]
        fn test_parse_raw_message_books_channel() {
            let json = load_test_json("ws_books_snapshot.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::BookData { arg, action, data } => {
                    assert!(
                        matches!(arg.channel, OKXWsChannel::Books),
                        "Expected Books channel"
                    );
                    assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                    assert!(
                        matches!(action, OKXBookAction::Snapshot),
                        "Expected snapshot action"
                    );
                    assert!(!data.is_empty());
                }
                _ => panic!("Expected OKXWsMessage::BookData variant"),
            }
        }

        #[rstest]
        fn test_parse_raw_message_candle_channel() {
            let json = load_test_json("ws_candle.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::Data { arg, data } => {
                    // Candle channel variant is Candle1Day for "candle1D"
                    assert!(
                        matches!(arg.channel, OKXWsChannel::Candle1Day),
                        "Expected Candle1Day channel, was {:?}",
                        arg.channel
                    );
                    assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                    assert!(data.is_array());
                }
                _ => panic!("Expected OKXWsMessage::Data variant"),
            }
        }

        #[rstest]
        fn test_parse_raw_message_funding_rate_channel() {
            let json = load_test_json("ws_funding_rate.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::Data { arg, data } => {
                    assert!(
                        matches!(arg.channel, OKXWsChannel::FundingRate),
                        "Expected FundingRate channel"
                    );
                    assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT-SWAP")));
                    assert!(data.is_array());
                }
                _ => panic!("Expected OKXWsMessage::Data variant"),
            }
        }

        #[rstest]
        fn test_parse_raw_message_bbo_tbt_channel() {
            let json = load_test_json("ws_bbo_tbt.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            match msg {
                OKXWsMessage::Data { arg, data } => {
                    assert!(
                        matches!(arg.channel, OKXWsChannel::BboTbt),
                        "Expected BboTbt channel"
                    );
                    assert!(data.is_array());
                }
                _ => panic!("Expected OKXWsMessage::Data variant"),
            }
        }

        #[rstest]
        fn test_handle_other_channel_data_tickers() {
            let mut handler = create_handler_with_instruments(vec![create_spot_instrument()]);
            let json = load_test_json("ws_tickers.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { arg, data } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_other_channel_data(arg.channel, arg.inst_id, data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::Data(payloads) => {
                    assert!(!payloads.is_empty(), "Should produce data payloads");
                }
                other => panic!("Expected NautilusWsMessage::Data, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_other_channel_data_trades() {
            // Create instrument with BTC-USD symbol (matches test data)
            let instrument_id = InstrumentId::from("BTC-USD.OKX");
            let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
                instrument_id,
                Symbol::from("BTC-USD"),
                Currency::BTC(),
                Currency::USD(),
                1,
                8,
                Price::from("0.1"),
                Quantity::from("0.00000001"),
                None, // multiplier
                None, // lot_size
                None, // max_quantity
                None, // min_quantity
                None, // max_notional
                None, // min_notional
                None, // max_price
                None, // min_price
                None, // margin_init
                None, // margin_maint
                None, // maker_fee
                None, // taker_fee
                UnixNanos::default(),
                UnixNanos::default(),
            ));

            let mut handler = create_handler_with_instruments(vec![instrument]);
            let json = load_test_json("ws_trades.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { arg, data } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_other_channel_data(arg.channel, arg.inst_id, data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::Data(payloads) => {
                    assert!(!payloads.is_empty(), "Should produce trade data payloads");
                }
                other => panic!("Expected NautilusWsMessage::Data, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_book_data_snapshot() {
            let handler = create_handler_with_instruments(vec![create_spot_instrument()]);
            let json = load_test_json("ws_books_snapshot.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::BookData { arg, action, data } = msg else {
                panic!("Expected OKXWsMessage::BookData");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_book_data(arg, action, data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::Data(payloads) => {
                    assert!(!payloads.is_empty(), "Should produce order book payloads");
                }
                other => panic!("Expected NautilusWsMessage::Data, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_book_data_update() {
            let handler = create_handler_with_instruments(vec![create_spot_instrument()]);
            let json = load_test_json("ws_books_update.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::BookData { arg, action, data } = msg else {
                panic!("Expected OKXWsMessage::BookData");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_book_data(arg, action, data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::Data(payloads) => {
                    assert!(
                        !payloads.is_empty(),
                        "Should produce order book delta payloads"
                    );
                }
                other => panic!("Expected NautilusWsMessage::Data, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_other_channel_data_candles() {
            let mut handler = create_handler_with_instruments(vec![create_spot_instrument()]);
            let json = load_test_json("ws_candle.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { arg, data } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_other_channel_data(arg.channel, arg.inst_id, data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::Data(payloads) => {
                    assert!(!payloads.is_empty(), "Should produce bar data payloads");
                }
                other => panic!("Expected NautilusWsMessage::Data, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_other_channel_data_funding_rate() {
            let mut handler = create_handler_with_instruments(vec![create_swap_instrument()]);
            let json = load_test_json("ws_funding_rate.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { arg, data } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_other_channel_data(arg.channel, arg.inst_id, data, ts_init);

            // Funding rate returns FundingRates variant when rate changes
            assert!(result.is_none() || matches!(result, Some(NautilusWsMessage::FundingRates(_))));
        }

        #[rstest]
        fn test_handle_account_data_parses_successfully() {
            let mut handler = create_handler_with_instruments(vec![]);
            let json = load_test_json("ws_account.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { data, .. } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_account_data(data, ts_init);

            assert!(result.is_some());
            match result.unwrap() {
                NautilusWsMessage::AccountUpdate(account_state) => {
                    assert!(
                        !account_state.balances.is_empty(),
                        "Should have balance data"
                    );
                }
                other => panic!("Expected NautilusWsMessage::AccountUpdate, was {other:?}"),
            }
        }

        #[rstest]
        fn test_handle_other_channel_data_missing_instrument() {
            let mut handler = create_handler_with_instruments(vec![]);
            let json = load_test_json("ws_tickers.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::Data { arg, data } = msg else {
                panic!("Expected OKXWsMessage::Data");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_other_channel_data(arg.channel, arg.inst_id, data, ts_init);

            // Should return None when instrument is not in cache
            assert!(result.is_none());
        }

        #[rstest]
        fn test_handle_book_data_missing_instrument() {
            let handler = create_handler_with_instruments(vec![]);
            let json = load_test_json("ws_books_snapshot.json");
            let msg: OKXWsMessage = serde_json::from_str(&json).unwrap();

            let OKXWsMessage::BookData { arg, action, data } = msg else {
                panic!("Expected OKXWsMessage::BookData");
            };

            let ts_init = UnixNanos::from(1_000_000_000u64);
            let result = handler.handle_book_data(arg, action, data, ts_init);

            // Should return None when instrument is not in cache
            assert!(result.is_none());
        }
    }
}

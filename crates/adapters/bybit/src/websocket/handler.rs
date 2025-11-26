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

//! WebSocket message handler for Bybit.

use std::{
    collections::VecDeque,
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::cache::quote::QuoteCache;
use nautilus_core::{UUID4, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{BarSpecification, BarType, Data},
    enums::{AggregationSource, BarAggregation, PriceType},
    events::{OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::BybitWsOperation,
    error::{BybitWsError, create_bybit_timeout_error, should_retry_bybit_error},
    messages::{
        BybitWebSocketError, BybitWsHeader, BybitWsMessage, BybitWsRequest, NautilusWsMessage,
    },
    parse::{
        parse_kline_topic, parse_millis_i64, parse_orderbook_deltas, parse_orderbook_quote,
        parse_ticker_linear_funding, parse_ws_account_state, parse_ws_fill_report,
        parse_ws_kline_bar, parse_ws_order_status_report, parse_ws_position_status_report,
        parse_ws_trade_tick,
    },
};
use crate::{
    common::{
        consts::BYBIT_NAUTILUS_BROKER_ID,
        enums::{BybitProductType, BybitTimeInForce, BybitWsOrderRequestOp},
        parse::{make_bybit_symbol, parse_price_with_precision, parse_quantity_with_precision},
    },
    websocket::messages::{
        BybitBatchOrderError, BybitWsAmendOrderParams, BybitWsCancelOrderParams,
        BybitWsOrderResponse, BybitWsPlaceOrderParams,
    },
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Authenticate {
        payload: String,
    },
    Subscribe {
        topics: Vec<String>,
    },
    Unsubscribe {
        topics: Vec<String>,
    },
    SendText {
        payload: String,
    },
    PlaceOrder {
        params: BybitWsPlaceOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    AmendOrder {
        params: BybitWsAmendOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    },
    CancelOrder {
        params: BybitWsCancelOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    },
    RegisterBatchPlace {
        req_id: String,
        orders: Vec<BatchOrderData>,
    },
    RegisterBatchCancel {
        req_id: String,
        cancels: Vec<BatchCancelData>,
    },
    InitializeInstruments(Vec<InstrumentAny>),
    UpdateInstrument(InstrumentAny),
}

/// Type alias for the funding rate cache.
type FundingCache = Arc<RwLock<AHashMap<Ustr, (Option<String>, Option<String>)>>>;

/// Data cached for pending place requests to correlate with responses.
type PlaceRequestData = (ClientOrderId, TraderId, StrategyId, InstrumentId);

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

/// Data for a single order in a batch request.
type BatchOrderData = (ClientOrderId, PlaceRequestData);

/// Data for a single cancel in a batch request.
type BatchCancelData = (ClientOrderId, CancelRequestData);

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
    mm_level: Arc<AtomicU8>,
    product_type: Option<BybitProductType>,
    quote_cache: QuoteCache,
    funding_cache: FundingCache,
    retry_manager: RetryManager<BybitWsError>,
    pending_place_requests: DashMap<String, PlaceRequestData>,
    pending_cancel_requests: DashMap<String, CancelRequestData>,
    pending_amend_requests: DashMap<String, AmendRequestData>,
    pending_batch_place_requests: DashMap<String, Vec<BatchOrderData>>,
    pending_batch_cancel_requests: DashMap<String, Vec<BatchCancelData>>,
    message_queue: VecDeque<NautilusWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: Option<AccountId>,
        product_type: Option<BybitProductType>,
        mm_level: Arc<AtomicU8>,
        auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
        funding_cache: FundingCache,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions,
            instruments_cache: AHashMap::new(),
            account_id,
            mm_level,
            product_type,
            quote_cache: QuoteCache::new(),
            funding_cache,
            retry_manager: create_websocket_retry_manager(),
            pending_place_requests: DashMap::new(),
            pending_cancel_requests: DashMap::new(),
            pending_amend_requests: DashMap::new(),
            pending_batch_place_requests: DashMap::new(),
            pending_batch_cancel_requests: DashMap::new(),
            message_queue: VecDeque::new(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), ()> {
        self.out_tx.send(msg).map_err(|_| ())
    }

    fn generate_unique_request_id(&self) -> String {
        UUID4::new().to_string()
    }

    fn find_and_remove_place_request_by_client_order_id(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(String, PlaceRequestData)> {
        self.pending_place_requests
            .iter()
            .find(|entry| entry.value().0 == *client_order_id)
            .and_then(|entry| {
                let key = entry.key().clone();
                drop(entry);
                self.pending_place_requests.remove(&key)
            })
    }

    fn find_and_remove_cancel_request_by_client_order_id(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(String, CancelRequestData)> {
        self.pending_cancel_requests
            .iter()
            .find(|entry| entry.value().0 == *client_order_id)
            .and_then(|entry| {
                let key = entry.key().clone();
                drop(entry);
                self.pending_cancel_requests.remove(&key)
            })
    }

    fn find_and_remove_amend_request_by_client_order_id(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(String, AmendRequestData)> {
        self.pending_amend_requests
            .iter()
            .find(|entry| entry.value().0 == *client_order_id)
            .and_then(|entry| {
                let key = entry.key().clone();
                drop(entry);
                self.pending_amend_requests.remove(&key)
            })
    }

    fn include_referer_header(&self, time_in_force: Option<BybitTimeInForce>) -> bool {
        let is_post_only = matches!(time_in_force, Some(BybitTimeInForce::PostOnly));
        let mm_level = self.mm_level.load(Ordering::Relaxed);
        !(is_post_only && mm_level > 0)
    }

    /// Sends a WebSocket message with retry logic.
    async fn send_with_retry(&self, payload: String) -> Result<(), BybitWsError> {
        if let Some(client) = &self.client {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client
                                .send_text(payload, None)
                                .await
                                .map_err(|e| BybitWsError::Transport(format!("Send failed: {e}")))
                        }
                    },
                    should_retry_bybit_error,
                    create_bybit_timeout_error,
                )
                .await
        } else {
            Err(BybitWsError::ClientError(
                "No active WebSocket client".to_string(),
            ))
        }
    }

    /// Handles batch operation request-level failures (ret_code != 0).
    ///
    /// When a batch request fails entirely, generate rejection events for all orders
    /// in the batch and clean up tracking data.
    fn handle_batch_failure(
        &self,
        req_id: &str,
        ret_msg: &str,
        op: &str,
        ts_init: UnixNanos,
        result: &mut Vec<NautilusWsMessage>,
    ) {
        if op.contains("create") {
            if let Some((_, batch_data)) = self.pending_batch_place_requests.remove(req_id) {
                tracing::warn!(
                    req_id = %req_id,
                    ret_msg = %ret_msg,
                    num_orders = batch_data.len(),
                    "Batch place request failed"
                );

                let Some(account_id) = self.account_id else {
                    tracing::error!("Cannot create OrderRejected events: account_id is None");
                    return;
                };

                let reason = Ustr::from(ret_msg);
                for (client_order_id, (_, trader_id, strategy_id, instrument_id)) in batch_data {
                    let rejected = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        reason,
                        UUID4::new(),
                        ts_init,
                        ts_init,
                        false,
                        false,
                    );
                    result.push(NautilusWsMessage::OrderRejected(rejected));
                }
            }
        } else if op.contains("cancel")
            && let Some((_, batch_data)) = self.pending_batch_cancel_requests.remove(req_id)
        {
            tracing::warn!(
                req_id = %req_id,
                ret_msg = %ret_msg,
                num_cancels = batch_data.len(),
                "Batch cancel request failed"
            );

            let reason = Ustr::from(ret_msg);
            for (client_order_id, (_, trader_id, strategy_id, instrument_id, venue_order_id)) in
                batch_data
            {
                let rejected = OrderCancelRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    reason,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    venue_order_id,
                    self.account_id,
                );
                result.push(NautilusWsMessage::OrderCancelRejected(rejected));
            }
        }
    }

    /// Handles batch operation responses, checking for individual order failures.
    fn handle_batch_response(
        &self,
        resp: &BybitWsOrderResponse,
        result: &mut Vec<NautilusWsMessage>,
    ) {
        let Some(req_id) = &resp.req_id else {
            tracing::warn!(
                op = %resp.op,
                "Batch response missing req_id - cannot correlate with pending requests"
            );
            return;
        };

        let batch_errors = resp.extract_batch_errors();

        if resp.op.contains("create") {
            if let Some((_, batch_data)) = self.pending_batch_place_requests.remove(req_id) {
                self.process_batch_place_errors(batch_data, batch_errors, result);
            } else {
                tracing::debug!(
                    req_id = %req_id,
                    "Batch place response received but no pending request found"
                );
            }
        } else if resp.op.contains("cancel") {
            if let Some((_, batch_data)) = self.pending_batch_cancel_requests.remove(req_id) {
                self.process_batch_cancel_errors(batch_data, batch_errors, result);
            } else {
                tracing::debug!(
                    req_id = %req_id,
                    "Batch cancel response received but no pending request found"
                );
            }
        }
    }

    /// Processes individual order errors from a batch place operation.
    fn process_batch_place_errors(
        &self,
        batch_data: Vec<BatchOrderData>,
        errors: Vec<BybitBatchOrderError>,
        result: &mut Vec<NautilusWsMessage>,
    ) {
        let Some(account_id) = self.account_id else {
            tracing::error!("Cannot create OrderRejected events: account_id is None");
            return;
        };

        let clock = get_atomic_clock_realtime();
        let ts_init = clock.get_time_ns();

        for (idx, (client_order_id, (_, trader_id, strategy_id, instrument_id))) in
            batch_data.into_iter().enumerate()
        {
            if let Some(error) = errors.get(idx)
                && error.code != 0
            {
                tracing::warn!(
                    client_order_id = %client_order_id,
                    error_code = error.code,
                    error_msg = %error.msg,
                    "Batch order rejected"
                );

                let rejected = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    Ustr::from(&error.msg),
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    false,
                );
                result.push(NautilusWsMessage::OrderRejected(rejected));
            }
        }
    }

    /// Processes individual order errors from a batch cancel operation.
    fn process_batch_cancel_errors(
        &self,
        batch_data: Vec<BatchCancelData>,
        errors: Vec<BybitBatchOrderError>,
        result: &mut Vec<NautilusWsMessage>,
    ) {
        let clock = get_atomic_clock_realtime();
        let ts_init = clock.get_time_ns();

        for (idx, (client_order_id, (_, trader_id, strategy_id, instrument_id, venue_order_id))) in
            batch_data.into_iter().enumerate()
        {
            if let Some(error) = errors.get(idx)
                && error.code != 0
            {
                tracing::warn!(
                    client_order_id = %client_order_id,
                    error_code = error.code,
                    error_msg = %error.msg,
                    "Batch cancel rejected"
                );

                let rejected = OrderCancelRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Ustr::from(&error.msg),
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    venue_order_id,
                    self.account_id,
                );
                result.push(NautilusWsMessage::OrderCancelRejected(rejected));
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();

        loop {
            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            tracing::debug!("WebSocketClient received by handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            tracing::debug!("Disconnect command received");

                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                        }
                        HandlerCommand::Authenticate { payload } => {
                            tracing::debug!("Authenticate command received");
                            if let Err(e) = self.send_with_retry(payload).await {
                                tracing::error!("Failed to send authentication after retries: {e}");
                            }
                        }
                        HandlerCommand::Subscribe { topics } => {
                            for topic in topics {
                                tracing::debug!(topic = %topic, "Subscribing to topic");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    tracing::error!(topic = %topic, error = %e, "Failed to send subscription after retries");
                                }
                            }
                        }
                        HandlerCommand::Unsubscribe { topics } => {
                            for topic in topics {
                                tracing::debug!(topic = %topic, "Unsubscribing from topic");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    tracing::error!(topic = %topic, error = %e, "Failed to send unsubscription after retries");
                                }
                            }
                        }
                        HandlerCommand::SendText { payload } => {
                            if let Err(e) = self.send_with_retry(payload).await {
                                tracing::error!("Error sending text with retry: {e}");
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
                        HandlerCommand::RegisterBatchPlace { req_id, orders } => {
                            tracing::debug!(
                                req_id = %req_id,
                                num_orders = orders.len(),
                                "Registering batch place request"
                            );
                            self.pending_batch_place_requests.insert(req_id, orders);
                        }
                        HandlerCommand::RegisterBatchCancel { req_id, cancels } => {
                            tracing::debug!(
                                req_id = %req_id,
                                num_cancels = cancels.len(),
                                "Registering batch cancel request"
                            );
                            self.pending_batch_cancel_requests.insert(req_id, cancels);
                        }
                        HandlerCommand::PlaceOrder {
                            params,
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                        } => {
                            let request_id = self.generate_unique_request_id();

                            self.pending_place_requests.insert(
                                request_id.clone(),
                                (client_order_id, trader_id, strategy_id, instrument_id),
                            );

                            let referer = if self.include_referer_header(params.time_in_force) {
                                Some(BYBIT_NAUTILUS_BROKER_ID.to_string())
                            } else {
                                None
                            };

                            let request = BybitWsRequest {
                                req_id: Some(request_id.clone()),
                                op: BybitWsOrderRequestOp::Create,
                                header: BybitWsHeader::with_referer(referer),
                                args: vec![params],
                            };

                            if let Ok(payload) = serde_json::to_string(&request)
                                && let Err(e) = self.send_with_retry(payload).await
                            {
                                tracing::error!("Failed to send place order after retries: {e}");
                                self.pending_place_requests.remove(&request_id);
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
                            let request_id = self.generate_unique_request_id();

                            self.pending_amend_requests.insert(
                                request_id.clone(),
                                (client_order_id, trader_id, strategy_id, instrument_id, venue_order_id),
                            );

                            let request = BybitWsRequest {
                                req_id: Some(request_id.clone()),
                                op: BybitWsOrderRequestOp::Amend,
                                header: BybitWsHeader::now(),
                                args: vec![params],
                            };

                            if let Ok(payload) = serde_json::to_string(&request)
                                && let Err(e) = self.send_with_retry(payload).await
                            {
                                tracing::error!("Failed to send amend order after retries: {e}");
                                self.pending_amend_requests.remove(&request_id);
                            }
                        }
                        HandlerCommand::CancelOrder {
                            params,
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                            venue_order_id,
                        } => {
                            let request_id = self.generate_unique_request_id();

                            self.pending_cancel_requests.insert(
                                request_id.clone(),
                                (client_order_id, trader_id, strategy_id, instrument_id, venue_order_id),
                            );

                            let request = BybitWsRequest {
                                req_id: Some(request_id.clone()),
                                op: BybitWsOrderRequestOp::Cancel,
                                header: BybitWsHeader::now(),
                                args: vec![params],
                            };

                            if let Ok(payload) = serde_json::to_string(&request)
                                && let Err(e) = self.send_with_retry(payload).await
                            {
                                tracing::error!("Failed to send cancel order after retries: {e}");
                                self.pending_cancel_requests.remove(&request_id);
                            }
                        }
                    }

                    continue;
                }

                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received during idle period");
                        return None;
                    }
                    continue;
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            tracing::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    if let Message::Ping(data) = &msg {
                        tracing::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            tracing::warn!(error = %e, "Failed to send pong frame");
                        }
                        continue;
                    }

                    let event = match Self::parse_raw_message(msg) {
                        Some(event) => event,
                        None => continue,
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    let ts_init = clock.get_time_ns();
                    let instruments = self.instruments_cache.clone();
                    let funding_cache = Arc::clone(&self.funding_cache);
                    let nautilus_messages = self.parse_to_nautilus_messages(
                        event,
                        &instruments,
                        self.account_id,
                        self.product_type,
                        &funding_cache,
                        ts_init,
                    )
                    .await;

                    // Enqueue all parsed messages to emit them one by one
                    self.message_queue.extend(nautilus_messages);
                }
            }
        }
    }

    fn parse_raw_message(msg: Message) -> Option<BybitWsMessage> {
        use serde_json::Value;

        match msg {
            Message::Text(text) => {
                if text == nautilus_network::RECONNECTED {
                    tracing::info!("Received WebSocket reconnected signal");
                    return Some(BybitWsMessage::Reconnected);
                }

                if text.trim().eq_ignore_ascii_case("pong") {
                    return None;
                }

                tracing::trace!("Raw websocket message: {text}");

                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("Failed to parse WebSocket message: {e}: {text}");
                        return None;
                    }
                };

                Self::classify_bybit_message(&value).or(Some(BybitWsMessage::Raw(value)))
            }
            Message::Binary(msg) => {
                tracing::debug!("Raw binary: {msg:?}");
                None
            }
            Message::Close(_) => {
                tracing::debug!("Received close message, waiting for reconnection");
                None
            }
            _ => None,
        }
    }

    pub(crate) fn classify_bybit_message(value: &serde_json::Value) -> Option<BybitWsMessage> {
        use super::{
            enums::BybitWsOperation,
            messages::{
                BybitWsAuthResponse, BybitWsOrderResponse, BybitWsResponse, BybitWsSubscriptionMsg,
            },
        };

        if let Ok(op) = serde_json::from_value::<BybitWsOperation>(
            value.get("op").cloned().unwrap_or(serde_json::Value::Null),
        ) && op == BybitWsOperation::Auth
            && let Ok(auth) = serde_json::from_value::<BybitWsAuthResponse>(value.clone())
        {
            let is_success = auth.success.unwrap_or(false) || auth.ret_code.unwrap_or(-1) == 0;
            if is_success {
                return Some(BybitWsMessage::Auth(auth));
            }
            let resp = BybitWsResponse {
                op: Some(auth.op.clone()),
                topic: None,
                success: auth.success,
                conn_id: auth.conn_id.clone(),
                req_id: None,
                ret_code: auth.ret_code,
                ret_msg: auth.ret_msg,
            };
            let error = BybitWebSocketError::from_response(&resp);
            return Some(BybitWsMessage::Error(error));
        }

        if let Some(success) = value.get("success").and_then(serde_json::Value::as_bool) {
            if success {
                if let Ok(msg) = serde_json::from_value::<BybitWsSubscriptionMsg>(value.clone()) {
                    return Some(BybitWsMessage::Subscription(msg));
                }
            } else if let Ok(resp) = serde_json::from_value::<BybitWsResponse>(value.clone()) {
                let error = BybitWebSocketError::from_response(&resp);
                return Some(BybitWsMessage::Error(error));
            }
        }

        if let Some(op) = value.get("op").and_then(serde_json::Value::as_str)
            && op.starts_with("order.")
            && let Ok(order_resp) = serde_json::from_value::<BybitWsOrderResponse>(value.clone())
        {
            return Some(BybitWsMessage::OrderResponse(order_resp));
        }

        if let Some(topic) = value.get("topic").and_then(serde_json::Value::as_str) {
            if topic.starts_with("orderbook")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::Orderbook(msg));
            } else if (topic.contains("publicTrade") || topic.starts_with("trade"))
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::Trade(msg));
            } else if topic.starts_with("kline")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::Kline(msg));
            } else if topic.starts_with("tickers") {
                // Option symbols have format: BTC-6JAN23-17500-C (with hyphens, date, strike, and C/P)
                if let Some(symbol) = value
                    .get("data")
                    .and_then(|d| d.get("symbol"))
                    .and_then(|s| s.as_str())
                    && symbol.contains('-')
                    && symbol.matches('-').count() >= 3
                    && let Ok(msg) = serde_json::from_value(value.clone())
                {
                    return Some(BybitWsMessage::TickerOption(msg));
                }
                if let Ok(msg) = serde_json::from_value(value.clone()) {
                    return Some(BybitWsMessage::TickerLinear(msg));
                }
            } else if topic.starts_with("order")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::AccountOrder(msg));
            } else if topic.starts_with("execution")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::AccountExecution(msg));
            } else if topic.starts_with("wallet")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::AccountWallet(msg));
            } else if topic.starts_with("position")
                && let Ok(msg) = serde_json::from_value(value.clone())
            {
                return Some(BybitWsMessage::AccountPosition(msg));
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn parse_to_nautilus_messages(
        &mut self,
        msg: BybitWsMessage,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        account_id: Option<AccountId>,
        product_type: Option<BybitProductType>,
        funding_cache: &FundingCache,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match msg {
            BybitWsMessage::Orderbook(msg) => {
                let raw_symbol = msg.data.s;
                let symbol =
                    product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    match parse_orderbook_deltas(&msg, instrument, ts_init) {
                        Ok(deltas) => result.push(NautilusWsMessage::Deltas(deltas)),
                        Err(e) => tracing::error!("Error parsing orderbook deltas: {e}"),
                    }

                    // For depth=1 subscriptions, also emit QuoteTick from top-of-book
                    if let Some(depth_str) = msg.topic.as_str().split('.').nth(1)
                        && depth_str == "1"
                    {
                        let instrument_id = instrument.id();
                        let last_quote = self.quote_cache.get(&instrument_id);

                        match parse_orderbook_quote(&msg, instrument, last_quote, ts_init) {
                            Ok(quote) => {
                                self.quote_cache.insert(instrument_id, quote);
                                result.push(NautilusWsMessage::Data(vec![Data::Quote(quote)]));
                            }
                            Err(e) => tracing::debug!("Skipping orderbook quote: {e}"),
                        }
                    }
                } else {
                    tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Orderbook message");
                }
            }
            BybitWsMessage::Trade(msg) => {
                let mut data_vec = Vec::new();
                for trade in &msg.data {
                    let raw_symbol = trade.s;
                    let symbol =
                        product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                    if let Some(instrument) = instruments.get(&symbol) {
                        match parse_ws_trade_tick(trade, instrument, ts_init) {
                            Ok(tick) => data_vec.push(Data::Trade(tick)),
                            Err(e) => tracing::error!("Error parsing trade tick: {e}"),
                        }
                    } else {
                        tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Trade message");
                    }
                }

                if !data_vec.is_empty() {
                    result.push(NautilusWsMessage::Data(data_vec));
                }
            }
            BybitWsMessage::Kline(msg) => {
                let (interval_str, raw_symbol) = match parse_kline_topic(&msg.topic) {
                    Ok(parts) => parts,
                    Err(e) => {
                        tracing::warn!("Failed to parse kline topic: {e}");
                        return result;
                    }
                };

                let symbol = product_type
                    .map_or_else(|| raw_symbol.into(), |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    let (step, aggregation) = match interval_str.parse::<usize>() {
                        Ok(minutes) if minutes > 0 => (minutes, BarAggregation::Minute),
                        _ => {
                            tracing::warn!("Unsupported kline interval: {}", interval_str);
                            return result;
                        }
                    };

                    if let Some(non_zero_step) = NonZero::new(step) {
                        let bar_spec = BarSpecification {
                            step: non_zero_step,
                            aggregation,
                            price_type: PriceType::Last,
                        };
                        let bar_type =
                            BarType::new(instrument.id(), bar_spec, AggregationSource::External);

                        let mut data_vec = Vec::new();
                        for kline in &msg.data {
                            // Only process confirmed bars (not partial/building bars)
                            if !kline.confirm {
                                continue;
                            }
                            match parse_ws_kline_bar(kline, instrument, bar_type, false, ts_init) {
                                Ok(bar) => data_vec.push(Data::Bar(bar)),
                                Err(e) => tracing::error!("Error parsing kline to bar: {e}"),
                            }
                        }
                        if !data_vec.is_empty() {
                            result.push(NautilusWsMessage::Data(data_vec));
                        }
                    } else {
                        tracing::error!("Invalid step value: {}", step);
                    }
                } else {
                    tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Kline message");
                }
            }
            BybitWsMessage::TickerLinear(msg) => {
                let raw_symbol = msg.data.symbol;
                let symbol =
                    product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    let instrument_id = instrument.id();
                    let ts_event = parse_millis_i64(msg.ts, "ticker.ts").unwrap_or(ts_init);
                    let price_precision = instrument.price_precision();
                    let size_precision = instrument.size_precision();

                    // Parse Bybit linear ticker fields, propagate errors
                    let bid_price = msg
                        .data
                        .bid1_price
                        .as_deref()
                        .map(|s| parse_price_with_precision(s, price_precision, "bid1Price"))
                        .transpose();
                    let ask_price = msg
                        .data
                        .ask1_price
                        .as_deref()
                        .map(|s| parse_price_with_precision(s, price_precision, "ask1Price"))
                        .transpose();
                    let bid_size = msg
                        .data
                        .bid1_size
                        .as_deref()
                        .map(|s| parse_quantity_with_precision(s, size_precision, "bid1Size"))
                        .transpose();
                    let ask_size = msg
                        .data
                        .ask1_size
                        .as_deref()
                        .map(|s| parse_quantity_with_precision(s, size_precision, "ask1Size"))
                        .transpose();

                    match (bid_price, ask_price, bid_size, ask_size) {
                        (Ok(bp), Ok(ap), Ok(bs), Ok(as_)) => {
                            match self.quote_cache.process(
                                instrument_id,
                                bp,
                                ap,
                                bs,
                                as_,
                                ts_event,
                                ts_init,
                            ) {
                                Ok(quote) => {
                                    result.push(NautilusWsMessage::Data(vec![Data::Quote(quote)]));
                                }
                                Err(e) => {
                                    let raw_data = serde_json::to_string(&msg.data)
                                        .unwrap_or_else(|_| "<failed to serialize>".to_string());
                                    tracing::debug!(
                                        "Skipping partial ticker update: {e}, raw_data: {raw_data}"
                                    );
                                }
                            }
                        }
                        _ => {
                            let raw_data = serde_json::to_string(&msg.data)
                                .unwrap_or_else(|_| "<failed to serialize>".to_string());
                            tracing::warn!(
                                "Failed to parse ticker fields, skipping update, raw_data: {raw_data}"
                            );
                        }
                    }

                    // Extract funding rate if available
                    if msg.data.funding_rate.is_some() && msg.data.next_funding_time.is_some() {
                        let cache_key = (
                            msg.data.funding_rate.clone(),
                            msg.data.next_funding_time.clone(),
                        );

                        let should_publish = {
                            let cache = funding_cache.read().await;
                            cache.get(&symbol) != Some(&cache_key)
                        };

                        if should_publish {
                            match parse_ticker_linear_funding(
                                &msg.data,
                                instrument_id,
                                ts_event,
                                ts_init,
                            ) {
                                Ok(funding) => {
                                    funding_cache.write().await.insert(symbol, cache_key);
                                    result.push(NautilusWsMessage::FundingRates(vec![funding]));
                                }
                                Err(e) => {
                                    tracing::debug!("Skipping funding rate update: {e}");
                                }
                            }
                        }
                    }
                } else {
                    tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in TickerLinear message");
                }
            }
            BybitWsMessage::TickerOption(msg) => {
                let raw_symbol = &msg.data.symbol;
                let symbol = product_type.map_or_else(
                    || raw_symbol.as_str().into(),
                    |pt| make_bybit_symbol(raw_symbol, pt),
                );

                if let Some(instrument) = instruments.get(&symbol) {
                    let instrument_id = instrument.id();
                    let ts_event = parse_millis_i64(msg.ts, "ticker.ts").unwrap_or(ts_init);
                    let price_precision = instrument.price_precision();
                    let size_precision = instrument.size_precision();

                    // Parse Bybit option ticker fields (always complete), propagate errors
                    let bid_price = parse_price_with_precision(
                        &msg.data.bid_price,
                        price_precision,
                        "bidPrice",
                    );
                    let ask_price = parse_price_with_precision(
                        &msg.data.ask_price,
                        price_precision,
                        "askPrice",
                    );
                    let bid_size = parse_quantity_with_precision(
                        &msg.data.bid_size,
                        size_precision,
                        "bidSize",
                    );
                    let ask_size = parse_quantity_with_precision(
                        &msg.data.ask_size,
                        size_precision,
                        "askSize",
                    );

                    match (bid_price, ask_price, bid_size, ask_size) {
                        (Ok(bp), Ok(ap), Ok(bs), Ok(as_)) => {
                            match self.quote_cache.process(
                                instrument_id,
                                Some(bp),
                                Some(ap),
                                Some(bs),
                                Some(as_),
                                ts_event,
                                ts_init,
                            ) {
                                Ok(quote) => {
                                    result.push(NautilusWsMessage::Data(vec![Data::Quote(quote)]));
                                }
                                Err(e) => {
                                    let raw_data = serde_json::to_string(&msg.data)
                                        .unwrap_or_else(|_| "<failed to serialize>".to_string());
                                    tracing::debug!(
                                        "Skipping partial ticker update: {e}, raw_data: {raw_data}"
                                    );
                                }
                            }
                        }
                        _ => {
                            let raw_data = serde_json::to_string(&msg.data)
                                .unwrap_or_else(|_| "<failed to serialize>".to_string());
                            tracing::warn!(
                                "Failed to parse ticker fields, skipping update, raw_data: {raw_data}"
                            );
                        }
                    }
                } else {
                    tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in TickerOption message");
                }
            }
            BybitWsMessage::AccountOrder(msg) => {
                if let Some(account_id) = account_id {
                    let mut reports = Vec::new();
                    for order in &msg.data {
                        let raw_symbol = order.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, order.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_order_status_report(
                                order, instrument, account_id, ts_init,
                            ) {
                                Ok(report) => reports.push(report),
                                Err(e) => tracing::error!("Error parsing order status report: {e}"),
                            }
                        } else {
                            tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountOrder message");
                        }
                    }
                    if !reports.is_empty() {
                        result.push(NautilusWsMessage::OrderStatusReports(reports));
                    }
                }
            }
            BybitWsMessage::AccountExecution(msg) => {
                if let Some(account_id) = account_id {
                    let mut reports = Vec::new();
                    for execution in &msg.data {
                        let raw_symbol = execution.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, execution.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_fill_report(execution, account_id, instrument, ts_init) {
                                Ok(report) => reports.push(report),
                                Err(e) => tracing::error!("Error parsing fill report: {e}"),
                            }
                        } else {
                            tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountExecution message");
                        }
                    }
                    if !reports.is_empty() {
                        result.push(NautilusWsMessage::FillReports(reports));
                    }
                }
            }
            BybitWsMessage::AccountPosition(msg) => {
                if let Some(account_id) = account_id {
                    for position in &msg.data {
                        let raw_symbol = position.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, position.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_position_status_report(
                                position, account_id, instrument, ts_init,
                            ) {
                                Ok(report) => {
                                    result.push(NautilusWsMessage::PositionStatusReport(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing position status report: {e}");
                                }
                            }
                        } else {
                            tracing::debug!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountPosition message");
                        }
                    }
                }
            }
            BybitWsMessage::AccountWallet(msg) => {
                if let Some(account_id) = account_id {
                    for wallet in &msg.data {
                        let ts_event = UnixNanos::from(msg.creation_time as u64 * 1_000_000);

                        match parse_ws_account_state(wallet, account_id, ts_event, ts_init) {
                            Ok(state) => result.push(NautilusWsMessage::AccountState(state)),
                            Err(e) => tracing::error!("Error parsing account state: {e}"),
                        }
                    }
                }
            }
            BybitWsMessage::OrderResponse(resp) => {
                if resp.ret_code == 0 {
                    tracing::debug!(op = %resp.op, ret_msg = %resp.ret_msg, "Order operation successful");

                    if resp.op.contains("batch") {
                        self.handle_batch_response(&resp, &mut result);
                    } else if let Some(req_id) = &resp.req_id {
                        if resp.op.contains("create") {
                            self.pending_place_requests.remove(req_id);
                        } else if resp.op.contains("cancel") {
                            self.pending_cancel_requests.remove(req_id);
                        } else if resp.op.contains("amend") {
                            self.pending_amend_requests.remove(req_id);
                        }
                    } else if let Some(order_link_id) =
                        resp.data.get("orderLinkId").and_then(|v| v.as_str())
                    {
                        // Bybit sometimes omits req_id, search by client_order_id instead
                        let client_order_id = ClientOrderId::from(order_link_id);
                        if resp.op.contains("create") {
                            self.find_and_remove_place_request_by_client_order_id(&client_order_id);
                        } else if resp.op.contains("cancel") {
                            self.find_and_remove_cancel_request_by_client_order_id(
                                &client_order_id,
                            );
                        } else if resp.op.contains("amend") {
                            self.find_and_remove_amend_request_by_client_order_id(&client_order_id);
                        }
                    }
                } else if let Some(req_id) = &resp.req_id {
                    let clock = get_atomic_clock_realtime();
                    let ts_init = clock.get_time_ns();

                    if resp.op.contains("batch") {
                        self.handle_batch_failure(
                            req_id,
                            &resp.ret_msg,
                            &resp.op,
                            ts_init,
                            &mut result,
                        );
                    } else if resp.op.contains("create")
                        && let Some((_, (client_order_id, trader_id, strategy_id, instrument_id))) =
                            self.pending_place_requests.remove(req_id)
                    {
                        let Some(account_id) = self.account_id else {
                            tracing::error!(
                                request_id = %req_id,
                                reason = %resp.ret_msg,
                                "Cannot create OrderRejected event: account_id is None"
                            );
                            return result;
                        };

                        let rejected = OrderRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            account_id,
                            Ustr::from(&resp.ret_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            false,
                        );
                        result.push(NautilusWsMessage::OrderRejected(rejected));
                    } else if resp.op.contains("cancel")
                        && let Some((
                            _,
                            (
                                client_order_id,
                                trader_id,
                                strategy_id,
                                instrument_id,
                                venue_order_id,
                            ),
                        )) = self.pending_cancel_requests.remove(req_id)
                    {
                        let rejected = OrderCancelRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(&resp.ret_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            venue_order_id,
                            self.account_id,
                        );
                        result.push(NautilusWsMessage::OrderCancelRejected(rejected));
                    } else if resp.op.contains("amend")
                        && let Some((
                            _,
                            (
                                client_order_id,
                                trader_id,
                                strategy_id,
                                instrument_id,
                                venue_order_id,
                            ),
                        )) = self.pending_amend_requests.remove(req_id)
                    {
                        let rejected = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(&resp.ret_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            venue_order_id,
                            self.account_id,
                        );
                        result.push(NautilusWsMessage::OrderModifyRejected(rejected));
                    }
                } else if let Some(order_link_id) =
                    resp.data.get("orderLinkId").and_then(|v| v.as_str())
                {
                    // Bybit sometimes omits req_id, search by client_order_id instead
                    let clock = get_atomic_clock_realtime();
                    let ts_init = clock.get_time_ns();
                    let client_order_id = ClientOrderId::from(order_link_id);

                    if resp.op.contains("create") {
                        if let Some((_, (_, trader_id, strategy_id, instrument_id))) =
                            self.find_and_remove_place_request_by_client_order_id(&client_order_id)
                        {
                            let Some(account_id) = self.account_id else {
                                tracing::error!(
                                    client_order_id = %client_order_id,
                                    reason = %resp.ret_msg,
                                    "Cannot create OrderRejected event: account_id is None"
                                );
                                return result;
                            };

                            let rejected = OrderRejected::new(
                                trader_id,
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                account_id,
                                Ustr::from(&resp.ret_msg),
                                UUID4::new(),
                                ts_init,
                                ts_init,
                                false,
                                false,
                            );
                            result.push(NautilusWsMessage::OrderRejected(rejected));
                        }
                    } else if resp.op.contains("cancel") {
                        if let Some((
                            _,
                            (_, trader_id, strategy_id, instrument_id, venue_order_id),
                        )) =
                            self.find_and_remove_cancel_request_by_client_order_id(&client_order_id)
                        {
                            let rejected = OrderCancelRejected::new(
                                trader_id,
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                Ustr::from(&resp.ret_msg),
                                UUID4::new(),
                                ts_init,
                                ts_init,
                                false,
                                venue_order_id,
                                self.account_id,
                            );
                            result.push(NautilusWsMessage::OrderCancelRejected(rejected));
                        }
                    } else if resp.op.contains("amend")
                        && let Some((_, (_, trader_id, strategy_id, instrument_id, venue_order_id))) =
                            self.find_and_remove_amend_request_by_client_order_id(&client_order_id)
                    {
                        let rejected = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Ustr::from(&resp.ret_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            venue_order_id,
                            self.account_id,
                        );
                        result.push(NautilusWsMessage::OrderModifyRejected(rejected));
                    }
                } else {
                    tracing::warn!(
                        op = %resp.op,
                        ret_code = resp.ret_code,
                        ret_msg = %resp.ret_msg,
                        "Order operation failed but request_id could not be extracted from response"
                    );
                }
            }
            BybitWsMessage::Auth(auth_response) => {
                let is_success =
                    auth_response.success.unwrap_or(false) || (auth_response.ret_code == Some(0));

                if is_success {
                    self.auth_tracker.succeed();
                    tracing::info!("WebSocket authenticated");
                    result.push(NautilusWsMessage::Authenticated);
                } else {
                    let error_msg = auth_response
                        .ret_msg
                        .as_deref()
                        .unwrap_or("Authentication rejected");
                    self.auth_tracker.fail(error_msg);
                    tracing::error!(error = error_msg, "WebSocket authentication failed");
                    result.push(NautilusWsMessage::Error(BybitWebSocketError::from_message(
                        error_msg.to_string(),
                    )));
                }
            }
            BybitWsMessage::Error(err) => {
                result.push(NautilusWsMessage::Error(err));
            }
            BybitWsMessage::Reconnected => {
                self.quote_cache.clear();
                result.push(NautilusWsMessage::Reconnected);
            }
            BybitWsMessage::Subscription(sub_msg) => {
                let pending_topics = self.subscriptions.pending_subscribe_topics();
                match sub_msg.op {
                    BybitWsOperation::Subscribe => {
                        if sub_msg.success {
                            for topic in pending_topics {
                                self.subscriptions.confirm_subscribe(&topic);
                                tracing::debug!(topic = topic, "Subscription confirmed");
                            }
                        } else {
                            for topic in pending_topics {
                                self.subscriptions.mark_failure(&topic);
                                tracing::warn!(
                                    topic = topic,
                                    error = ?sub_msg.ret_msg,
                                    "Subscription failed, will retry on reconnect"
                                );
                            }
                        }
                    }
                    BybitWsOperation::Unsubscribe => {
                        let pending_unsub = self.subscriptions.pending_unsubscribe_topics();
                        if sub_msg.success {
                            for topic in pending_unsub {
                                self.subscriptions.confirm_unsubscribe(&topic);
                                tracing::debug!(topic = topic, "Unsubscription confirmed");
                            }
                        } else {
                            for topic in pending_unsub {
                                tracing::warn!(
                                    topic = topic,
                                    error = ?sub_msg.ret_msg,
                                    "Unsubscription failed"
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        result
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::BYBIT_WS_TOPIC_DELIMITER;

    fn create_test_handler() -> FeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let auth_tracker = AuthTracker::new();
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);
        let funding_cache = Arc::new(RwLock::new(AHashMap::new()));

        FeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            None,
            Arc::new(AtomicU8::new(0)),
            auth_tracker,
            subscriptions,
            funding_cache,
        )
    }

    #[rstest]
    fn test_generate_unique_request_id_returns_different_ids() {
        let handler = create_test_handler();

        let id1 = handler.generate_unique_request_id();
        let id2 = handler.generate_unique_request_id();
        let id3 = handler.generate_unique_request_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[rstest]
    fn test_generate_unique_request_id_produces_valid_uuids() {
        let handler = create_test_handler();

        let id1 = handler.generate_unique_request_id();
        let id2 = handler.generate_unique_request_id();

        assert!(UUID4::from(id1.as_str()).to_string() == id1);
        assert!(UUID4::from(id2.as_str()).to_string() == id2);
    }

    #[rstest]
    fn test_multiple_place_orders_use_different_request_ids() {
        let handler = create_test_handler();

        let req_id_1 = handler.generate_unique_request_id();
        let req_id_2 = handler.generate_unique_request_id();
        let req_id_3 = handler.generate_unique_request_id();

        assert_ne!(req_id_1, req_id_2);
        assert_ne!(req_id_2, req_id_3);
        assert_ne!(req_id_1, req_id_3);
    }

    #[rstest]
    fn test_multiple_amends_use_different_request_ids() {
        let handler = create_test_handler();

        // Verifies fix for "Duplicate reqId" errors when amending same order multiple times
        let req_id_1 = handler.generate_unique_request_id();
        let req_id_2 = handler.generate_unique_request_id();
        let req_id_3 = handler.generate_unique_request_id();

        assert_ne!(
            req_id_1, req_id_2,
            "Multiple amends should generate different request IDs to avoid 'Duplicate reqId' errors"
        );
        assert_ne!(
            req_id_2, req_id_3,
            "Multiple amends should generate different request IDs to avoid 'Duplicate reqId' errors"
        );
    }

    #[rstest]
    fn test_multiple_cancels_use_different_request_ids() {
        let handler = create_test_handler();

        let req_id_1 = handler.generate_unique_request_id();
        let req_id_2 = handler.generate_unique_request_id();

        assert_ne!(
            req_id_1, req_id_2,
            "Multiple cancels should generate different request IDs"
        );
    }

    #[rstest]
    fn test_concurrent_request_id_generation() {
        let handler = create_test_handler();

        let mut ids = std::collections::HashSet::new();
        for _ in 0..100 {
            let id = handler.generate_unique_request_id();
            assert!(
                ids.insert(id.clone()),
                "Generated duplicate request ID: {}",
                id
            );
        }
        assert_eq!(ids.len(), 100);
    }
}

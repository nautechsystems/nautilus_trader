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

//! WebSocket message handler for Deribit.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_common::cache::fifo::FifoCacheMap;
use nautilus_core::{AtomicSet, AtomicTime, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, CustomData, Data, DataType, InstrumentStatus},
    enums::{MarketStatusAction, OrderSide, OrderType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderFilled, OrderModifyRejected,
        OrderRejected,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use rust_decimal::Decimal;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{DeribitBookMsgType, DeribitHeartbeatType, DeribitWsChannel, DeribitWsMethod},
    error::DeribitWsError,
    messages::{
        DeribitAuthResult, DeribitBookMsg, DeribitCancelAllByInstrumentParams, DeribitCancelParams,
        DeribitChartMsg, DeribitEditParams, DeribitHeartbeatParams, DeribitInstrumentStateMsg,
        DeribitJsonRpcRequest, DeribitOrderMsg, DeribitOrderParams, DeribitOrderResponse,
        DeribitPerpetualMsg, DeribitPortfolioMsg, DeribitQuoteMsg, DeribitSubscribeParams,
        DeribitTickerMsg, DeribitTradeMsg, DeribitUserTradeMsg, DeribitVolatilityIndexMsg,
        DeribitWsMessage, NautilusWsMessage, parse_raw_message,
    },
    parse::{
        OrderEventType, determine_order_event_type, parse_book_msg, parse_chart_msg,
        parse_deribit_order_type, parse_order_accepted_with_client_order_id,
        parse_order_canceled_with_client_order_id, parse_order_expired_with_client_order_id,
        parse_order_updated_with_client_order_id, parse_perpetual_to_funding_rate, parse_quote_msg,
        parse_ticker_to_index_price, parse_ticker_to_mark_price, parse_ticker_to_option_greeks,
        parse_trades_data, parse_user_order_msg, parse_user_trade_msg, resolution_to_bar_type,
    },
};
use crate::{
    common::{
        consts::{DERIBIT_POST_ONLY_ERROR_CODE, DERIBIT_RATE_LIMIT_KEY_ORDER, DERIBIT_VENUE},
        enums::DeribitInstrumentState,
        parse::{parse_portfolio_to_account_state, use_cost_for_bar_volume},
    },
    data_types::DeribitVolatilityIndex,
};

/// Type of pending request for request ID correlation.
#[derive(Debug, Clone)]
pub enum PendingRequestType {
    /// Authentication request.
    Authenticate,
    /// Subscribe request with requested channels.
    Subscribe { channels: Vec<String> },
    /// Unsubscribe request with requested channels.
    Unsubscribe { channels: Vec<String> },
    /// Set heartbeat request.
    SetHeartbeat,
    /// Test/ping request (heartbeat response).
    Test,
    /// Buy order request.
    Buy {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        order_type: OrderType,
    },
    /// Sell order request.
    Sell {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        order_type: OrderType,
    },
    /// Edit order request.
    Edit {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Cancel order request.
    Cancel {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Cancel all orders by instrument request.
    CancelAllByInstrument { instrument_id: InstrumentId },
    /// Get order state request.
    GetOrderState {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
}

/// Commands sent from the client to the handler.
#[allow(missing_debug_implementations)]
pub enum HandlerCommand {
    /// Set the active WebSocket client.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket.
    Disconnect,
    /// Authenticate with credentials.
    Authenticate {
        /// Serialized auth params (DeribitAuthParams or DeribitRefreshTokenParams).
        auth_params: serde_json::Value,
    },
    /// Enable heartbeat with interval.
    SetHeartbeat { interval: u64 },
    /// Initialize the instrument cache.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(Box<InstrumentAny>),
    /// Subscribe to channels.
    Subscribe { channels: Vec<String> },
    /// Unsubscribe from channels.
    Unsubscribe { channels: Vec<String> },
    /// Submit a buy order.
    Buy {
        params: DeribitOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Submit a sell order.
    Sell {
        params: DeribitOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Edit an existing order.
    Edit {
        params: DeribitEditParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Cancel an existing order.
    Cancel {
        params: DeribitCancelParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Cancel all orders by instrument.
    CancelAllByInstrument {
        params: DeribitCancelAllByInstrumentParams,
        instrument_id: InstrumentId,
    },
    /// Get order state.
    GetOrderState {
        order_id: String,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
}

/// Context for an order submitted via this handler.
///
/// Stores the submitted order identity for routing live order and trade updates.
#[derive(Debug, Clone)]
pub struct OrderContext {
    pub client_order_id: ClientOrderId,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub accepted: bool,
    pub last_order_signature: Option<OrderSignature>,
}

/// Order fields used to identify a venue amendment.
pub type OrderSignature = (Decimal, Option<Decimal>, Option<Decimal>);

/// Deribit WebSocket feed handler.
///
/// Runs in a dedicated Tokio task, processing commands and raw WebSocket messages.
#[allow(missing_debug_implementations)]
pub struct DeribitWsFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions_state: SubscriptionState,
    retry_manager: RetryManager<DeribitWsError>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    option_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    mark_price_subs: Arc<AtomicSet<InstrumentId>>,
    index_price_subs: Arc<AtomicSet<InstrumentId>>,
    request_id_counter: AtomicU64,
    pending_requests: AHashMap<u64, PendingRequestType>,
    account_id: Option<AccountId>,
    order_contexts: AHashMap<VenueOrderId, OrderContext>,
    submitted_order_contexts: FifoCacheMap<ClientOrderId, OrderContext, 10_000>,

    // Retain recent terminal identities so late trade frames stay on the order-event path
    terminal_order_contexts: FifoCacheMap<VenueOrderId, OrderContext, 10_000>,
    pending_bars: AHashMap<String, Bar>,
    bars_timestamp_on_close: bool,
    last_account_states: AHashMap<String, AccountState>,
    book_sequence: AHashMap<Ustr, u64>,
    pending_book_resync: Vec<String>,
    pending_outgoing: VecDeque<NautilusWsMessage>,
    subscribe_errors: Arc<Mutex<Vec<String>>>,
}

impl DeribitWsFeedHandler {
    /// Creates a new feed handler.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
        option_greeks_subs: Arc<AtomicSet<InstrumentId>>,
        mark_price_subs: Arc<AtomicSet<InstrumentId>>,
        index_price_subs: Arc<AtomicSet<InstrumentId>>,
        account_id: Option<AccountId>,
        bars_timestamp_on_close: bool,
        subscribe_errors: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions_state,
            retry_manager: create_websocket_retry_manager(),
            instruments_cache: AHashMap::new(),
            option_greeks_subs,
            mark_price_subs,
            index_price_subs,
            request_id_counter: AtomicU64::new(1),
            pending_requests: AHashMap::new(),
            account_id,
            order_contexts: AHashMap::new(),
            submitted_order_contexts: FifoCacheMap::new(),
            terminal_order_contexts: FifoCacheMap::new(),
            pending_bars: AHashMap::new(),
            bars_timestamp_on_close,
            last_account_states: AHashMap::new(),
            book_sequence: AHashMap::new(),
            pending_book_resync: Vec::new(),
            pending_outgoing: VecDeque::new(),
            subscribe_errors,
        }
    }

    /// Sets the account ID for order/fill reports.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    fn clear_state(&mut self) {
        let pending_count = self.pending_requests.len();
        let bars_count = self.pending_bars.len();
        let account_count = self.last_account_states.len();
        let book_count = self.book_sequence.len();
        let outgoing_count = self.pending_outgoing.len();

        self.pending_requests.clear();
        self.pending_bars.clear();
        self.last_account_states.clear();
        self.book_sequence.clear();
        self.pending_book_resync.clear();
        self.pending_outgoing.clear();

        log::debug!(
            "Reset state: pending_requests={pending_count}, pending_bars={bars_count}, \
            account_states={account_count}, book_sequence={book_count}, \
            pending_outgoing={outgoing_count}"
        );
    }

    /// Generates a unique request ID.
    fn next_request_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Returns the current timestamp.
    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    async fn send_tracked_request(
        &mut self,
        request_id: u64,
        payload: Result<String, DeribitWsError>,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), DeribitWsError> {
        let payload = match payload {
            Ok(p) => p,
            Err(e) => {
                self.pending_requests.remove(&request_id);
                return Err(e);
            }
        };
        self.send_with_retry(payload, rate_limit_keys).await
    }

    /// Sends a message over the WebSocket with retry logic.
    async fn send_with_retry(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), DeribitWsError> {
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
                                .map_err(|e| DeribitWsError::Send(e.to_string()))
                        }
                    },
                    |e| matches!(e, DeribitWsError::Send(_)),
                    DeribitWsError::Timeout,
                )
                .await
        } else {
            Err(DeribitWsError::NotConnected)
        }
    }

    /// Handles a subscribe command.
    ///
    /// Note: The client has already called `mark_subscribe` before sending this command.
    async fn handle_subscribe(&mut self, channels: Vec<String>) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests.insert(
            request_id,
            PendingRequestType::Subscribe {
                channels: channels.clone(),
            },
        );

        // Deribit requires private/subscribe for authenticated channels
        let method = if channels
            .iter()
            .any(|ch| DeribitWsChannel::requires_auth(ch))
        {
            DeribitWsMethod::PrivateSubscribe
        } else {
            DeribitWsMethod::PublicSubscribe
        };

        let request = DeribitJsonRpcRequest::new(
            request_id,
            method.as_method_str(),
            DeribitSubscribeParams {
                channels: channels.clone(),
            },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Subscribing to channels: request_id={request_id}, channels={channels:?}");
        self.send_tracked_request(request_id, payload, None).await
    }

    /// Handles an unsubscribe command.
    async fn handle_unsubscribe(&mut self, channels: Vec<String>) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests.insert(
            request_id,
            PendingRequestType::Unsubscribe {
                channels: channels.clone(),
            },
        );

        let method = if channels
            .iter()
            .any(|ch| DeribitWsChannel::requires_auth(ch))
        {
            DeribitWsMethod::PrivateUnsubscribe
        } else {
            DeribitWsMethod::PublicUnsubscribe
        };

        let request = DeribitJsonRpcRequest::new(
            request_id,
            method.as_method_str(),
            DeribitSubscribeParams {
                channels: channels.clone(),
            },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Unsubscribing from channels: request_id={request_id}, channels={channels:?}");
        self.send_tracked_request(request_id, payload, None).await
    }

    /// Handles enabling heartbeat.
    async fn handle_set_heartbeat(&mut self, interval: u64) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests
            .insert(request_id, PendingRequestType::SetHeartbeat);

        let request = DeribitJsonRpcRequest::new(
            request_id,
            DeribitWsMethod::SetHeartbeat.as_method_str(),
            DeribitHeartbeatParams { interval },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!(
            "Enabling heartbeat with interval: request_id={request_id}, interval={interval} seconds"
        );
        self.send_tracked_request(request_id, payload, None).await
    }

    /// Responds to a heartbeat test_request.
    async fn handle_heartbeat_test_request(&mut self) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests
            .insert(request_id, PendingRequestType::Test);

        let request = DeribitJsonRpcRequest::new(
            request_id,
            DeribitWsMethod::Test.as_method_str(),
            serde_json::json!({}),
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::trace!("Responding to heartbeat test_request: request_id={request_id}");
        self.send_tracked_request(request_id, payload, None).await
    }

    /// Handles a buy order command.
    async fn handle_buy(
        &mut self,
        params: DeribitOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();
        let order_type = parse_deribit_order_type(&params.order_type);
        let order_signature = (params.amount, params.price, params.trigger_price);

        self.submitted_order_contexts.insert(
            client_order_id,
            OrderContext {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                order_side: OrderSide::Buy,
                order_type,
                accepted: false,
                last_order_signature: Some(order_signature),
            },
        );

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Buy {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                order_side: OrderSide::Buy,
                order_type,
            },
        );

        let request =
            DeribitJsonRpcRequest::new(request_id, DeribitWsMethod::Buy.as_method_str(), params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Sending buy order: request_id={request_id}");
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Handles a sell order command.
    async fn handle_sell(
        &mut self,
        params: DeribitOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();
        let order_type = parse_deribit_order_type(&params.order_type);
        let order_signature = (params.amount, params.price, params.trigger_price);

        self.submitted_order_contexts.insert(
            client_order_id,
            OrderContext {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                order_side: OrderSide::Sell,
                order_type,
                accepted: false,
                last_order_signature: Some(order_signature),
            },
        );

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Sell {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                order_side: OrderSide::Sell,
                order_type,
            },
        );

        let request =
            DeribitJsonRpcRequest::new(request_id, DeribitWsMethod::Sell.as_method_str(), params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Sending sell order: request_id={request_id}");
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Handles an edit order command.
    async fn handle_edit(
        &mut self,
        params: DeribitEditParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();
        let order_id = params.order_id.clone();

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Edit {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let request =
            DeribitJsonRpcRequest::new(request_id, DeribitWsMethod::Edit.as_method_str(), params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Sending edit order: request_id={request_id}, order_id={order_id}");
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Handles a cancel order command.
    async fn handle_cancel(
        &mut self,
        params: DeribitCancelParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();
        let order_id = params.order_id.clone();

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Cancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let request =
            DeribitJsonRpcRequest::new(request_id, DeribitWsMethod::Cancel.as_method_str(), params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Sending cancel order: request_id={request_id}, order_id={order_id}");
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Handles cancel all orders by instrument command.
    async fn handle_cancel_all_by_instrument(
        &mut self,
        params: DeribitCancelAllByInstrumentParams,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();
        let instrument_name = params.instrument_name.clone();

        // Track this request for response correlation
        self.pending_requests.insert(
            request_id,
            PendingRequestType::CancelAllByInstrument { instrument_id },
        );

        let request = DeribitJsonRpcRequest::new(
            request_id,
            DeribitWsMethod::CancelAllByInstrument.as_method_str(),
            params,
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!(
            "Sending cancel_all_by_instrument: request_id={request_id}, instrument={instrument_name}"
        );
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Handles get order state command.
    async fn handle_get_order_state(
        &mut self,
        order_id: String,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests.insert(
            request_id,
            PendingRequestType::GetOrderState {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let params = serde_json::json!({
            "order_id": order_id
        });

        let request = DeribitJsonRpcRequest::new(
            request_id,
            DeribitWsMethod::GetOrderState.as_method_str(),
            params,
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()));

        log::debug!("Sending get_order_state: request_id={request_id}, order_id={order_id}");
        self.send_tracked_request(
            request_id,
            payload,
            Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()),
        )
        .await
    }

    /// Processes a command from the client.
    async fn process_command(&mut self, cmd: HandlerCommand) {
        match cmd {
            HandlerCommand::SetClient(client) => {
                log::debug!("Setting WebSocket client");
                self.inner = Some(client);
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnecting WebSocket");

                if let Some(client) = self.inner.take() {
                    client.disconnect().await;
                }
            }
            HandlerCommand::Authenticate { auth_params } => {
                let request_id = self.next_request_id();
                log::debug!("Authenticating: request_id={request_id}");

                // Track this request for response correlation
                self.pending_requests
                    .insert(request_id, PendingRequestType::Authenticate);

                let request = DeribitJsonRpcRequest::new(
                    request_id,
                    DeribitWsMethod::PublicAuth.as_method_str(),
                    auth_params,
                );

                match serde_json::to_string(&request) {
                    Ok(payload) => {
                        if let Err(e) = self.send_with_retry(payload, None).await {
                            self.pending_requests.remove(&request_id);
                            log::error!("Authentication send failed: {e}");
                            self.auth_tracker.fail(format!("Send failed: {e}"));
                        }
                    }
                    Err(e) => {
                        self.pending_requests.remove(&request_id);
                        log::error!("Failed to serialize auth request: {e}");
                        self.auth_tracker.fail(format!("Serialization failed: {e}"));
                    }
                }
            }
            HandlerCommand::SetHeartbeat { interval } => {
                if let Err(e) = self.handle_set_heartbeat(interval).await {
                    log::error!("Set heartbeat failed: {e}");
                }
            }
            HandlerCommand::InitializeInstruments(instruments) => {
                log::debug!("Handler received {} instruments", instruments.len());
                self.instruments_cache.clear();
                for inst in instruments {
                    self.instruments_cache
                        .insert(inst.raw_symbol().inner(), inst);
                }
            }
            HandlerCommand::UpdateInstrument(instrument) => {
                log::trace!("Updating instrument: {}", instrument.raw_symbol());
                self.instruments_cache
                    .insert(instrument.raw_symbol().inner(), *instrument);
            }
            HandlerCommand::Subscribe { channels } => {
                if let Err(e) = self.handle_subscribe(channels).await {
                    log::error!("Subscribe failed: {e}");
                }
            }
            HandlerCommand::Unsubscribe { channels } => {
                // User-initiated unsubscribe cancels any pending book resync
                // for these channels so we don't re-subscribe against user intent
                self.pending_book_resync.retain(|ch| !channels.contains(ch));

                if let Err(e) = self.handle_unsubscribe(channels).await {
                    log::error!("Unsubscribe failed: {e}");
                }
            }
            HandlerCommand::Buy {
                params,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_buy(
                        params,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!("Buy order failed: {e}");
                }
            }
            HandlerCommand::Sell {
                params,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_sell(
                        params,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!("Sell order failed: {e}");
                }
            }
            HandlerCommand::Edit {
                params,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_edit(
                        params,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!("Edit order failed: {e}");
                }
            }
            HandlerCommand::Cancel {
                params,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_cancel(
                        params,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!("Cancel order failed: {e}");
                }
            }
            HandlerCommand::CancelAllByInstrument {
                params,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_cancel_all_by_instrument(params, instrument_id)
                    .await
                {
                    log::error!("Cancel all by instrument failed: {e}");
                }
            }
            HandlerCommand::GetOrderState {
                order_id,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                if let Err(e) = self
                    .handle_get_order_state(
                        order_id,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!("Get order state failed: {e}");
                }
            }
        }
    }

    /// Processes a raw WebSocket message.
    async fn process_raw_message(&mut self, text: &str) -> Option<NautilusWsMessage> {
        if text == RECONNECTED {
            log::info!("Received reconnection signal");

            self.auth_tracker.invalidate();
            self.clear_state();

            return Some(NautilusWsMessage::Reconnected);
        }

        // Parse the JSON-RPC message
        let ws_msg = match parse_raw_message(text) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!("Failed to parse message: {e}");
                return None;
            }
        };

        let ts_init = self.ts_init();

        match ws_msg {
            DeribitWsMessage::Response(response) => {
                // Look up the request type by ID for explicit correlation
                if let Some(request_id) = response.id
                    && let Some(request_type) = self.pending_requests.remove(&request_id)
                {
                    match request_type {
                        PendingRequestType::Authenticate => {
                            if let Some(error) = &response.error {
                                let reason = format!(
                                    "Authentication error code={}: {}",
                                    error.code, error.message
                                );
                                log::error!(
                                    "Authentication failed: code={}, message={}, request_id={}",
                                    error.code,
                                    error.message,
                                    request_id
                                );
                                self.auth_tracker.fail(reason.clone());
                                return Some(NautilusWsMessage::AuthenticationFailed(reason));
                            } else if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitAuthResult>(result.clone()) {
                                    Ok(auth_result) => {
                                        self.auth_tracker.succeed();
                                        log::debug!(
                                            "WebSocket authenticated successfully (request_id={}, scope={}, expires_in={}s)",
                                            request_id,
                                            auth_result.scope,
                                            auth_result.expires_in
                                        );
                                        return Some(NautilusWsMessage::Authenticated(Box::new(
                                            auth_result,
                                        )));
                                    }
                                    Err(e) => {
                                        let reason = format!("Failed to parse auth result: {e}");
                                        log::error!("{reason}: request_id={request_id}");
                                        self.auth_tracker.fail(reason.clone());
                                        return Some(NautilusWsMessage::AuthenticationFailed(
                                            reason,
                                        ));
                                    }
                                }
                            }
                        }
                        PendingRequestType::Subscribe { channels } => {
                            if let Some(error) = &response.error {
                                log::error!(
                                    "Subscribe failed: code={}, message={}, channels={:?}, request_id={}",
                                    error.code,
                                    error.message,
                                    channels,
                                    request_id
                                );

                                if let Ok(mut errors) = self.subscribe_errors.lock() {
                                    errors.push(format!(
                                        "Subscribe rejected: code={}, message={}",
                                        error.code, error.message,
                                    ));
                                }
                            } else {
                                // Confirm each channel in the subscription
                                for ch in &channels {
                                    self.subscriptions_state.confirm_subscribe(ch);
                                    log::debug!("Subscription confirmed: {ch}");
                                }
                            }
                        }
                        PendingRequestType::Unsubscribe { channels } => {
                            if let Some(error) = &response.error {
                                log::error!(
                                    "Unsubscribe failed: code={}, message={}, channels={:?}, request_id={}",
                                    error.code,
                                    error.message,
                                    channels,
                                    request_id
                                );
                            } else {
                                for ch in &channels {
                                    self.subscriptions_state.confirm_unsubscribe(ch);
                                    log::debug!("Unsubscription confirmed: {ch}");
                                }
                            }

                            // Resubscribe channels pending book resync (kept in
                            // pending_book_resync until a fresh snapshot arrives)
                            if !self.pending_book_resync.is_empty() {
                                let resync: Vec<String> = channels
                                    .iter()
                                    .filter(|ch| self.pending_book_resync.contains(ch))
                                    .cloned()
                                    .collect();

                                if !resync.is_empty() {
                                    let _ = self.handle_subscribe(resync).await;
                                }
                            }
                        }
                        PendingRequestType::SetHeartbeat => {
                            if let Some(error) = &response.error {
                                log::error!(
                                    "Set heartbeat failed: code={}, message={}, request_id={}",
                                    error.code,
                                    error.message,
                                    request_id
                                );
                            } else {
                                log::debug!("Heartbeat enabled (request_id={request_id})");
                            }
                        }
                        PendingRequestType::Test => {
                            if let Some(error) = &response.error {
                                log::warn!(
                                    "Heartbeat test failed: code={}, message={}, request_id={}",
                                    error.code,
                                    error.message,
                                    request_id
                                );
                            } else {
                                log::trace!(
                                    "Heartbeat test acknowledged (request_id={request_id})"
                                );
                            }
                        }
                        PendingRequestType::Cancel {
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                        } => {
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitOrderMsg>(result.clone()) {
                                    Ok(order_msg) => {
                                        let venue_order_id =
                                            VenueOrderId::new(order_msg.order_id.as_str());
                                        log::debug!(
                                            "Cancel confirmed: venue_order_id={venue_order_id}, \
                                            client_order_id={client_order_id}, state={}",
                                            order_msg.order_state
                                        );

                                        // Emit OrderCanceled from the response path so we
                                        // do not lose the event during a reconnection gap.
                                        // Both paths check terminal context to suppress
                                        // duplicates regardless of which arrives first.
                                        if order_msg.order_state == "cancelled"
                                            && !self
                                                .terminal_order_contexts
                                                .contains_key(&venue_order_id)
                                        {
                                            let instrument_name_ustr =
                                                Ustr::from(order_msg.instrument_name.as_str());

                                            if let Some(instrument) =
                                                self.instruments_cache.get(&instrument_name_ustr)
                                                && let Some(account_id) = self.account_id
                                            {
                                                let event =
                                                    parse_order_canceled_with_client_order_id(
                                                        &order_msg,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        client_order_id,
                                                        ts_init,
                                                    );
                                                let context = self
                                                    .find_order_context(
                                                        venue_order_id,
                                                        Some(client_order_id),
                                                    )
                                                    .unwrap_or(OrderContext {
                                                        client_order_id,
                                                        trader_id,
                                                        strategy_id,
                                                        instrument_id,
                                                        order_side: match order_msg
                                                            .direction
                                                            .as_str()
                                                        {
                                                            "buy" => OrderSide::Buy,
                                                            "sell" => OrderSide::Sell,
                                                            _ => OrderSide::NoOrderSide,
                                                        },
                                                        order_type: parse_deribit_order_type(
                                                            &order_msg.order_type,
                                                        ),
                                                        accepted: true,
                                                        last_order_signature: Some(
                                                            Self::order_signature(&order_msg),
                                                        ),
                                                    });
                                                self.finish_order_context(venue_order_id, &context);
                                                return Some(NautilusWsMessage::OrderCanceled(
                                                    event,
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse cancel response: request_id={request_id}, error={e}"
                                        );
                                    }
                                }
                            } else if let Some(error) = &response.error {
                                log::error!(
                                    "Cancel rejected: code={}, message={}, client_order_id={}",
                                    error.code,
                                    error.message,
                                    client_order_id
                                );
                                return Some(NautilusWsMessage::OrderCancelRejected(
                                    OrderCancelRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        ustr::ustr(&format!(
                                            "code={}: {}",
                                            error.code, error.message
                                        )),
                                        UUID4::new(),
                                        ts_init,
                                        ts_init,
                                        false,
                                        None, // venue_order_id not available in error response
                                        self.account_id,
                                    ),
                                ));
                            }
                        }
                        PendingRequestType::CancelAllByInstrument { instrument_id } => {
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<u64>(result.clone()) {
                                    Ok(count) => {
                                        log::debug!(
                                            "Cancelled {count} orders for instrument {instrument_id}"
                                        );
                                        // Individual order status updates come via user.orders subscription
                                    }
                                    Err(e) => {
                                        log::warn!("Failed to parse cancel_all response: {e}");
                                    }
                                }
                            } else if let Some(error) = &response.error {
                                log::error!(
                                    "Cancel all by instrument rejected: code={}, message={}, instrument_id={}",
                                    error.code,
                                    error.message,
                                    instrument_id
                                );
                            }
                        }
                        PendingRequestType::Buy {
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                            order_side,
                            order_type,
                        }
                        | PendingRequestType::Sell {
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                            order_side,
                            order_type,
                        } => {
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitOrderResponse>(result.clone())
                                {
                                    Ok(order_response) => {
                                        let venue_order_id_str = &order_response.order.order_id;
                                        let venue_order_id =
                                            VenueOrderId::new(venue_order_id_str.as_str());
                                        let order_state = &order_response.order.order_state;
                                        log::debug!(
                                            "Order response: venue_order_id={venue_order_id}, client_order_id={client_order_id}, state={order_state}"
                                        );

                                        let mut context = self
                                            .find_order_context(
                                                venue_order_id,
                                                Some(client_order_id),
                                            )
                                            .unwrap_or(OrderContext {
                                                client_order_id,
                                                trader_id,
                                                strategy_id,
                                                instrument_id,
                                                order_side,
                                                order_type,
                                                accepted: false,
                                                last_order_signature: Some(Self::order_signature(
                                                    &order_response.order,
                                                )),
                                            });
                                        context.last_order_signature =
                                            Some(Self::order_signature(&order_response.order));
                                        self.bind_order_context(venue_order_id, context.clone());

                                        if !order_response.trades.is_empty() {
                                            let outgoing = self
                                                .route_user_trades(&order_response.trades, ts_init);
                                            self.pending_outgoing.extend(outgoing);
                                        } else if order_state == "filled" {
                                            log::debug!(
                                                "Deferring acceptance for fast-filled order until its trade arrives: venue_order_id={venue_order_id}, client_order_id={client_order_id}"
                                            );
                                            self.finish_order_context(venue_order_id, &context);
                                        } else if context.accepted {
                                            log::trace!(
                                                "Skipping duplicate OrderAccepted response: venue_order_id={venue_order_id}"
                                            );
                                        } else {
                                            let instrument_name_ustr = Ustr::from(
                                                order_response.order.instrument_name.as_str(),
                                            );

                                            if let Some(instrument) =
                                                self.instruments_cache.get(&instrument_name_ustr)
                                            {
                                                if let Some(account_id) = self.account_id {
                                                    let event =
                                                        parse_order_accepted_with_client_order_id(
                                                            &order_response.order,
                                                            instrument,
                                                            account_id,
                                                            trader_id,
                                                            strategy_id,
                                                            client_order_id,
                                                            ts_init,
                                                        );
                                                    context.accepted = true;
                                                    self.bind_order_context(
                                                        venue_order_id,
                                                        context,
                                                    );
                                                    return Some(NautilusWsMessage::OrderAccepted(
                                                        event,
                                                    ));
                                                } else {
                                                    log::warn!(
                                                        "Cannot create OrderAccepted: account_id not set"
                                                    );
                                                }
                                            } else {
                                                log::warn!(
                                                    "Instrument {instrument_name_ustr} not found in cache for order response"
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse order response: request_id={request_id}, error={e}"
                                        );
                                    }
                                }
                            } else if let Some(error) = &response.error {
                                let due_post_only = error.code == DERIBIT_POST_ONLY_ERROR_CODE;
                                let reason = if let Some(data) = &error.data {
                                    format!(
                                        "code={}: {} (data: {})",
                                        error.code, error.message, data
                                    )
                                } else {
                                    format!("code={}: {}", error.code, error.message)
                                };

                                log::debug!(
                                    "Order rejected: {reason}, client_order_id={client_order_id}"
                                );
                                self.submitted_order_contexts.remove(&client_order_id);
                                return Some(NautilusWsMessage::OrderRejected(OrderRejected::new(
                                    trader_id,
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    self.account_id.unwrap_or(AccountId::new("DERIBIT-UNKNOWN")),
                                    ustr::ustr(&reason),
                                    UUID4::new(),
                                    ts_init,
                                    ts_init,
                                    false,
                                    due_post_only,
                                )));
                            }
                        }
                        PendingRequestType::Edit {
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
                        } => {
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitOrderResponse>(result.clone())
                                {
                                    Ok(order_response) => {
                                        let venue_order_id =
                                            VenueOrderId::new(&order_response.order.order_id);
                                        log::debug!(
                                            "Order updated: venue_order_id={}, client_order_id={}, state={}",
                                            venue_order_id,
                                            client_order_id,
                                            order_response.order.order_state
                                        );

                                        let instrument_name_ustr = Ustr::from(
                                            order_response.order.instrument_name.as_str(),
                                        );

                                        if let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name_ustr)
                                        {
                                            if let Some(account_id) = self.account_id {
                                                let Some(mut context) = self.find_order_context(
                                                    venue_order_id,
                                                    Some(client_order_id),
                                                ) else {
                                                    let report = parse_user_order_msg(
                                                        &order_response.order,
                                                        instrument,
                                                        account_id,
                                                        ts_init,
                                                    );
                                                    let outgoing = self.route_user_trades(
                                                        &order_response.trades,
                                                        ts_init,
                                                    );
                                                    self.pending_outgoing.extend(outgoing);
                                                    return report
                                                    .map(|report| {
                                                        NautilusWsMessage::OrderStatusReports(vec![
                                                            report,
                                                        ])
                                                    })
                                                    .map_err(|e| {
                                                        log::warn!(
                                                            "Failed to parse external edit response: {e}"
                                                        );
                                                    })
                                                    .ok();
                                                };
                                                let was_terminal = self
                                                    .terminal_order_contexts
                                                    .contains_key(&venue_order_id);
                                                let signature =
                                                    Self::order_signature(&order_response.order);
                                                let duplicate_update = was_terminal
                                                    || context.last_order_signature
                                                        == Some(signature);
                                                let event = (!duplicate_update).then(|| {
                                                    parse_order_updated_with_client_order_id(
                                                        &order_response.order,
                                                        instrument,
                                                        account_id,
                                                        context.trader_id,
                                                        context.strategy_id,
                                                        context.client_order_id,
                                                        ts_init,
                                                    )
                                                });
                                                context.accepted = true;
                                                context.last_order_signature = Some(signature);

                                                if was_terminal {
                                                    self.finish_order_context(
                                                        venue_order_id,
                                                        &context,
                                                    );
                                                } else {
                                                    self.bind_order_context(
                                                        venue_order_id,
                                                        context,
                                                    );
                                                }
                                                let outgoing = self.route_user_trades(
                                                    &order_response.trades,
                                                    ts_init,
                                                );
                                                self.pending_outgoing.extend(outgoing);
                                                return event.map(NautilusWsMessage::OrderUpdated);
                                            } else {
                                                log::warn!(
                                                    "Cannot create OrderUpdated: account_id not set"
                                                );
                                            }
                                        } else {
                                            log::warn!(
                                                "Instrument {instrument_name_ustr} not found in cache for edit response"
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse edit response: request_id={request_id}, error={e}"
                                        );
                                    }
                                }
                            } else if let Some(error) = &response.error {
                                log::error!(
                                    "Order modify rejected: code={}, message={}, client_order_id={}",
                                    error.code,
                                    error.message,
                                    client_order_id
                                );
                                return Some(NautilusWsMessage::OrderModifyRejected(
                                    OrderModifyRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        ustr::ustr(&format!(
                                            "code={}: {}",
                                            error.code, error.message
                                        )),
                                        UUID4::new(),
                                        ts_init,
                                        ts_init,
                                        false,
                                        None, // venue_order_id not available
                                        self.account_id,
                                    ),
                                ));
                            }
                        }
                        PendingRequestType::GetOrderState {
                            client_order_id,
                            trader_id: _,
                            strategy_id: _,
                            instrument_id: _,
                        } => {
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitOrderMsg>(result.clone()) {
                                    Ok(order_msg) => {
                                        log::debug!(
                                            "Order state received: venue_order_id={}, client_order_id={}, state={}",
                                            order_msg.order_id,
                                            client_order_id,
                                            order_msg.order_state
                                        );

                                        // Convert to OrderStatusReport
                                        let instrument_name_ustr = order_msg.instrument_name;

                                        if let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name_ustr)
                                        {
                                            if let Some(account_id) = self.account_id {
                                                match parse_user_order_msg(
                                                    &order_msg, instrument, account_id, ts_init,
                                                ) {
                                                    Ok(report) => {
                                                        return Some(
                                                            NautilusWsMessage::OrderStatusReports(
                                                                vec![report],
                                                            ),
                                                        );
                                                    }
                                                    Err(e) => {
                                                        log::warn!(
                                                            "Failed to parse get_order_state response to report: {e}"
                                                        );
                                                    }
                                                }
                                            } else {
                                                log::warn!(
                                                    "Cannot create OrderStatusReport: account_id not set"
                                                );
                                            }
                                        } else {
                                            log::warn!(
                                                "Instrument {instrument_name_ustr} not found in cache for get_order_state response"
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse get_order_state response: request_id={request_id}, error={e}"
                                        );
                                    }
                                }
                            } else if let Some(error) = &response.error {
                                log::error!(
                                    "Get order state failed: code={}, message={}, client_order_id={}",
                                    error.code,
                                    error.message,
                                    client_order_id
                                );
                            }
                        }
                    }
                } else if let Some(request_id) = response.id {
                    // Response with ID but no matching pending request
                    if let Some(error) = &response.error {
                        // Log orphaned error response with all available context
                        log::error!(
                            "Deribit error for unknown request: code={}, message={}, request_id={}, data={:?}",
                            error.code,
                            error.message,
                            request_id,
                            error.data
                        );
                        return Some(NautilusWsMessage::Error(DeribitWsError::DeribitError {
                            code: error.code,
                            message: error.message.clone(),
                        }));
                    } else {
                        // Success response but no pending request - likely already processed
                        log::debug!(
                            "Received response for unknown request_id={}, result present: {}",
                            request_id,
                            response.result.is_some()
                        );
                    }
                } else if let Some(error) = &response.error {
                    // Error response with no ID (shouldn't happen in JSON-RPC 2.0, but handle it)
                    log::error!(
                        "Deribit error with no request_id: code={}, message={}, data={:?}",
                        error.code,
                        error.message,
                        error.data
                    );
                    return Some(NautilusWsMessage::Error(DeribitWsError::DeribitError {
                        code: error.code,
                        message: error.message.clone(),
                    }));
                }
                None
            }
            DeribitWsMessage::Notification(notification) => {
                let channel = &notification.params.channel;
                let data = &notification.params.data;

                // Determine channel type and parse accordingly
                if let Some(channel_type) = DeribitWsChannel::from_channel_string(channel) {
                    match channel_type {
                        DeribitWsChannel::Trades => {
                            // Parse trade messages
                            match serde_json::from_value::<Vec<DeribitTradeMsg>>(data.clone()) {
                                Ok(trades) => {
                                    log::debug!("Received {} trades", trades.len());
                                    let data_vec = parse_trades_data(
                                        &trades,
                                        &self.instruments_cache,
                                        ts_init,
                                    );

                                    if data_vec.is_empty() && !trades.is_empty() {
                                        let missing: Vec<&Ustr> = trades
                                            .iter()
                                            .map(|t| &t.instrument_name)
                                            .filter(|name| {
                                                !self.instruments_cache.contains_key(name)
                                            })
                                            .collect();

                                        if missing.is_empty() {
                                            log::warn!(
                                                "Received {} trades but parsed 0 (parse failures); cache size: {}",
                                                trades.len(),
                                                self.instruments_cache.len()
                                            );
                                        } else {
                                            log::warn!(
                                                "Trade message received but instrument(s) not found in cache: {:?} (cache size: {})",
                                                missing,
                                                self.instruments_cache.len()
                                            );
                                        }
                                    } else if !data_vec.is_empty() {
                                        log::debug!("Parsed {} trade ticks", data_vec.len());
                                        return Some(NautilusWsMessage::Data(data_vec));
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to deserialize trades: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::Book => {
                            // Parse order book messages
                            match serde_json::from_value::<DeribitBookMsg>(data.clone()) {
                                Ok(book_msg) => {
                                    if let Some(instrument) =
                                        self.instruments_cache.get(&book_msg.instrument_name)
                                    {
                                        let inst_name = book_msg.instrument_name.to_string();
                                        let awaiting_resync =
                                            self.pending_book_resync.iter().any(|ch| {
                                                ch.starts_with("book.")
                                                    && ch
                                                        .split('.')
                                                        .nth(1)
                                                        .is_some_and(|s| s == inst_name)
                                            });

                                        if awaiting_resync
                                            && book_msg.msg_type == DeribitBookMsgType::Change
                                        {
                                            // Drop deltas while awaiting resync snapshot
                                        } else if awaiting_resync
                                            && book_msg.msg_type == DeribitBookMsgType::Snapshot
                                        {
                                            self.pending_book_resync.retain(|ch| {
                                                !(ch.starts_with("book.")
                                                    && ch
                                                        .split('.')
                                                        .nth(1)
                                                        .is_some_and(|s| s == inst_name))
                                            });
                                            self.book_sequence.insert(
                                                book_msg.instrument_name,
                                                book_msg.change_id,
                                            );

                                            match parse_book_msg(&book_msg, instrument, ts_init) {
                                                Ok(deltas) => {
                                                    return Some(NautilusWsMessage::Deltas(deltas));
                                                }
                                                Err(e) => {
                                                    log::warn!("Failed to parse book message: {e}");
                                                }
                                            }
                                        } else if book_msg.msg_type == DeribitBookMsgType::Change
                                            && let Some(prev_id) = book_msg.prev_change_id
                                            && let Some(&last_id) =
                                                self.book_sequence.get(&book_msg.instrument_name)
                                            && prev_id != last_id
                                        {
                                            log::error!(
                                                "Book sequence gap for {}: expected prev_change_id={}, was {} \
                                                - dropping delta, forcing resync",
                                                book_msg.instrument_name,
                                                last_id,
                                                prev_id
                                            );
                                            self.book_sequence.remove(&book_msg.instrument_name);

                                            let book_channels: Vec<String> = self
                                                .subscriptions_state
                                                .all_topics()
                                                .into_iter()
                                                .filter(|t| {
                                                    t.starts_with("book.")
                                                        && t.split('.')
                                                            .nth(1)
                                                            .is_some_and(|s| s == inst_name)
                                                })
                                                .collect();

                                            if !book_channels.is_empty() {
                                                for ch in &book_channels {
                                                    self.subscriptions_state.mark_failure(ch);
                                                }
                                                // Defer resubscribe until unsubscribe ack
                                                self.pending_book_resync
                                                    .extend(book_channels.clone());
                                                let _ =
                                                    self.handle_unsubscribe(book_channels).await;
                                            }
                                        } else {
                                            self.book_sequence.insert(
                                                book_msg.instrument_name,
                                                book_msg.change_id,
                                            );

                                            match parse_book_msg(&book_msg, instrument, ts_init) {
                                                Ok(deltas) => {
                                                    return Some(NautilusWsMessage::Deltas(deltas));
                                                }
                                                Err(e) => {
                                                    log::warn!("Failed to parse book message: {e}");
                                                }
                                            }
                                        }
                                    } else {
                                        log::warn!(
                                            "Book message received but instrument '{}' not found in cache (cache size: {})",
                                            book_msg.instrument_name,
                                            self.instruments_cache.len()
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to deserialize book message: {e}, channel: {channel}"
                                    );
                                }
                            }
                        }
                        DeribitWsChannel::Ticker => {
                            match serde_json::from_value::<DeribitTickerMsg>(data.clone()) {
                                Ok(ticker_msg) => {
                                    if let Some(instrument) =
                                        self.instruments_cache.get(&ticker_msg.instrument_name)
                                    {
                                        // Emit OptionGreeks only if subscribed
                                        if self.option_greeks_subs.contains(&instrument.id())
                                            && let Some(option_greeks) =
                                                parse_ticker_to_option_greeks(
                                                    &ticker_msg,
                                                    instrument,
                                                    ts_init,
                                                )
                                        {
                                            let _ = self.out_tx.send(
                                                NautilusWsMessage::OptionGreeks(option_greeks),
                                            );
                                        }

                                        let instrument_id = instrument.id();
                                        let mut data_vec = Vec::new();

                                        // Emit MarkPriceUpdate only if subscribed
                                        if self.mark_price_subs.contains(&instrument_id) {
                                            match parse_ticker_to_mark_price(
                                                &ticker_msg,
                                                instrument,
                                                ts_init,
                                            ) {
                                                Ok(mark_price) => {
                                                    data_vec
                                                        .push(Data::MarkPriceUpdate(mark_price));
                                                }
                                                Err(e) => {
                                                    log::warn!("Failed to parse mark price: {e}");
                                                }
                                            }
                                        }

                                        // Emit IndexPriceUpdate only if subscribed
                                        if self.index_price_subs.contains(&instrument_id) {
                                            match parse_ticker_to_index_price(
                                                &ticker_msg,
                                                instrument,
                                                ts_init,
                                            ) {
                                                Ok(index_price) => {
                                                    data_vec
                                                        .push(Data::IndexPriceUpdate(index_price));
                                                }
                                                Err(e) => {
                                                    log::warn!("Failed to parse index price: {e}");
                                                }
                                            }
                                        }

                                        if !data_vec.is_empty() {
                                            return Some(NautilusWsMessage::Data(data_vec));
                                        }
                                    } else {
                                        log::warn!(
                                            "Ticker message received but instrument '{}' not found in cache (cache size: {})",
                                            ticker_msg.instrument_name,
                                            self.instruments_cache.len()
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to deserialize ticker message: {e}, channel: {channel}"
                                    );
                                }
                            }
                        }
                        DeribitWsChannel::Perpetual => {
                            // Parse perpetual channel for funding rate updates
                            // This channel is dedicated to perpetual instruments and provides
                            // the interest (funding) rate
                            match serde_json::from_value::<DeribitPerpetualMsg>(data.clone()) {
                                Ok(perpetual_msg) => {
                                    // Extract instrument name from channel: perpetual.{instrument}.{interval}
                                    let parts: Vec<&str> = channel.split('.').collect();
                                    if parts.len() >= 2 {
                                        let instrument_name = Ustr::from(parts[1]);

                                        if let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name)
                                        {
                                            let funding_rate = parse_perpetual_to_funding_rate(
                                                &perpetual_msg,
                                                instrument,
                                                ts_init,
                                            );
                                            return Some(NautilusWsMessage::FundingRates(vec![
                                                funding_rate,
                                            ]));
                                        } else {
                                            log::warn!(
                                                "Instrument {} not found in cache (cache size: {})",
                                                instrument_name,
                                                self.instruments_cache.len()
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to deserialize perpetual message: {e}, data: {data}"
                                    );
                                }
                            }
                        }
                        DeribitWsChannel::Quote => {
                            // Parse quote messages
                            match serde_json::from_value::<DeribitQuoteMsg>(data.clone()) {
                                Ok(quote_msg) => {
                                    if let Some(instrument) =
                                        self.instruments_cache.get(&quote_msg.instrument_name)
                                    {
                                        match parse_quote_msg(&quote_msg, instrument, ts_init) {
                                            Ok(quote) => {
                                                return Some(NautilusWsMessage::Data(vec![
                                                    Data::Quote(quote),
                                                ]));
                                            }
                                            Err(e) => {
                                                log::warn!("Failed to parse quote message: {e}");
                                            }
                                        }
                                    } else {
                                        log::warn!(
                                            "Quote message received but instrument '{}' not found in cache (cache size: {})",
                                            quote_msg.instrument_name,
                                            self.instruments_cache.len()
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to deserialize quote message: {e}, channel: {channel}"
                                    );
                                }
                            }
                        }
                        DeribitWsChannel::VolatilityIndex => {
                            match serde_json::from_value::<DeribitVolatilityIndexMsg>(data.clone())
                            {
                                Ok(msg) => {
                                    let ts_event = UnixNanos::from(msg.timestamp * 1_000_000);
                                    let mut metadata = nautilus_core::Params::new();
                                    metadata.insert(
                                        "index_name".to_string(),
                                        serde_json::Value::String(msg.index_name.clone()),
                                    );
                                    let data_type = DataType::new(
                                        "DeribitVolatilityIndex",
                                        Some(metadata),
                                        None,
                                    );

                                    let dvol = DeribitVolatilityIndex::new(
                                        msg.index_name,
                                        msg.volatility,
                                        ts_event,
                                        ts_init,
                                    );

                                    return Some(NautilusWsMessage::Data(vec![Data::Custom(
                                        CustomData::new(Arc::new(dvol), data_type),
                                    )]));
                                }
                                Err(e) => {
                                    log::warn!("Failed to deserialize volatility index: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::InstrumentState => {
                            match serde_json::from_value::<DeribitInstrumentStateMsg>(data.clone())
                            {
                                Ok(state_msg) => {
                                    log::debug!(
                                        "Instrument state change: {} -> {} (timestamp: {})",
                                        state_msg.instrument_name,
                                        state_msg.state,
                                        state_msg.timestamp
                                    );

                                    let instrument_id = if let Some(instrument) =
                                        self.instruments_cache.get(&state_msg.instrument_name)
                                    {
                                        instrument.id()
                                    } else {
                                        log::debug!(
                                            "Instrument '{}' not in cache, constructing ID",
                                            state_msg.instrument_name
                                        );
                                        InstrumentId::new(
                                            Symbol::new(state_msg.instrument_name),
                                            *DERIBIT_VENUE,
                                        )
                                    };

                                    let action = MarketStatusAction::from(state_msg.state);
                                    let is_trading =
                                        Some(state_msg.state == DeribitInstrumentState::Started);
                                    let ts_event = UnixNanos::from(state_msg.timestamp * 1_000_000);
                                    let status = InstrumentStatus::new(
                                        instrument_id,
                                        action,
                                        ts_event,
                                        ts_init,
                                        None,
                                        None,
                                        is_trading,
                                        None,
                                        None,
                                    );
                                    return Some(NautilusWsMessage::InstrumentStatus(status));
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse instrument status message: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::ChartTrades => {
                            // Parse chart.trades messages into Bar objects using emit-on-next pattern.
                            // Deribit sends updates for the current bar as it builds. We only emit
                            // a bar when we receive a bar with a different timestamp, confirming
                            // the previous bar is closed.
                            if let Ok(chart_msg) =
                                serde_json::from_value::<DeribitChartMsg>(data.clone())
                            {
                                // Extract instrument and resolution from channel
                                // Channel format: chart.trades.{instrument}.{resolution}
                                let parts: Vec<&str> = channel.split('.').collect();
                                if parts.len() >= 4 {
                                    let instrument_name = Ustr::from(parts[2]);
                                    let resolution = parts[3];

                                    if let Some(instrument) =
                                        self.instruments_cache.get(&instrument_name)
                                    {
                                        let instrument_id = instrument.id();

                                        match resolution_to_bar_type(instrument_id, resolution) {
                                            Ok(bar_type) => {
                                                let price_precision = instrument.price_precision();
                                                let size_precision = instrument.size_precision();
                                                let use_cost_for_volume =
                                                    use_cost_for_bar_volume(instrument);

                                                match parse_chart_msg(
                                                    &chart_msg,
                                                    bar_type,
                                                    price_precision,
                                                    size_precision,
                                                    use_cost_for_volume,
                                                    self.bars_timestamp_on_close,
                                                    ts_init,
                                                ) {
                                                    Ok(new_bar) => {
                                                        // Check if we have a pending bar for this channel
                                                        let channel_key = channel.clone();

                                                        if let Some(pending_bar) =
                                                            self.pending_bars.get(&channel_key)
                                                        {
                                                            // If new bar has different timestamp, the pending bar is closed
                                                            if new_bar.ts_event
                                                                != pending_bar.ts_event
                                                            {
                                                                let closed_bar = *pending_bar;
                                                                self.pending_bars
                                                                    .insert(channel_key, new_bar);
                                                                log::debug!(
                                                                    "Emitting closed bar: {closed_bar:?}"
                                                                );
                                                                return Some(
                                                                    NautilusWsMessage::Data(vec![
                                                                        Data::Bar(closed_bar),
                                                                    ]),
                                                                );
                                                            }
                                                            // Same timestamp - update pending bar with latest values
                                                            self.pending_bars
                                                                .insert(channel_key, new_bar);
                                                        } else {
                                                            // First bar for this channel - store as pending
                                                            self.pending_bars
                                                                .insert(channel_key, new_bar);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        log::warn!(
                                                            "Failed to parse chart message to bar: {e}"
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                log::warn!(
                                                    "Failed to create BarType from resolution {resolution}: {e}"
                                                );
                                            }
                                        }
                                    } else {
                                        log::warn!(
                                            "Instrument {instrument_name} not found in cache for chart data"
                                        );
                                    }
                                }
                            }
                        }
                        DeribitWsChannel::UserOrders => {
                            // Handle both array and single object responses
                            let orders_result =
                                serde_json::from_value::<Vec<DeribitOrderMsg>>(data.clone())
                                    .or_else(|_| {
                                        serde_json::from_value::<DeribitOrderMsg>(data.clone())
                                            .map(|order| vec![order])
                                    });

                            match orders_result {
                                Ok(orders) => {
                                    log::debug!("Received {} user order updates", orders.len());

                                    // Require account_id for parsing
                                    let Some(account_id) = self.account_id else {
                                        log::warn!("Cannot parse user orders: account_id not set");
                                        return Some(NautilusWsMessage::Raw(data.clone()));
                                    };

                                    let mut outgoing = Vec::new();

                                    // Process each order and emit appropriate events
                                    for order in &orders {
                                        let venue_order_id_str = &order.order_id;
                                        let venue_order_id =
                                            VenueOrderId::new(venue_order_id_str.as_str());
                                        let instrument_name = order.instrument_name;

                                        let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name)
                                        else {
                                            log::warn!(
                                                "Instrument {instrument_name} not found in cache"
                                            );
                                            continue;
                                        };

                                        let label_client_order_id = order
                                            .label
                                            .as_ref()
                                            .filter(|l| !l.is_empty())
                                            .map(ClientOrderId::new);
                                        let was_terminal = self
                                            .terminal_order_contexts
                                            .contains_key(&venue_order_id);
                                        let Some(mut context) = self.find_order_context(
                                            venue_order_id,
                                            label_client_order_id,
                                        ) else {
                                            match parse_user_order_msg(
                                                order, instrument, account_id, ts_init,
                                            ) {
                                                Ok(report) => outgoing.push(
                                                    NautilusWsMessage::OrderStatusReports(vec![
                                                        report,
                                                    ]),
                                                ),
                                                Err(e) => log::warn!(
                                                    "Failed to parse external order update: {e}"
                                                ),
                                            }
                                            continue;
                                        };

                                        let signature = Self::order_signature(order);

                                        // Determine event type based on order state
                                        let event_type = determine_order_event_type(
                                            &order.order_state,
                                            !context.accepted,
                                            order.replaced
                                                && context.last_order_signature != Some(signature),
                                        );
                                        context.last_order_signature = Some(signature);

                                        let trader_id = context.trader_id;
                                        let strategy_id = context.strategy_id;
                                        let client_order_id = context.client_order_id;

                                        match event_type {
                                            OrderEventType::Accepted => {
                                                // Skip if order already reached terminal state (race condition)
                                                if self
                                                    .terminal_order_contexts
                                                    .contains_key(&venue_order_id)
                                                {
                                                    log::debug!(
                                                        "Skipping OrderAccepted for terminal order: client_order_id={client_order_id}"
                                                    );
                                                    continue;
                                                }

                                                let event =
                                                    parse_order_accepted_with_client_order_id(
                                                        order,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        client_order_id,
                                                        ts_init,
                                                    );
                                                context.accepted = true;
                                                self.bind_order_context(venue_order_id, context);

                                                log::debug!(
                                                    "Emitting OrderAccepted: venue_order_id={venue_order_id}"
                                                );
                                                outgoing
                                                    .push(NautilusWsMessage::OrderAccepted(event));
                                            }
                                            OrderEventType::Canceled => {
                                                // Skip if already emitted from the cancel
                                                // response path
                                                if self
                                                    .terminal_order_contexts
                                                    .contains_key(&venue_order_id)
                                                {
                                                    log::trace!(
                                                        "Skipping duplicate OrderCanceled: client_order_id={client_order_id}"
                                                    );
                                                    continue;
                                                }

                                                if !context.accepted {
                                                    outgoing
                                                        .push(NautilusWsMessage::OrderAccepted(
                                                        parse_order_accepted_with_client_order_id(
                                                            order,
                                                            instrument,
                                                            account_id,
                                                            trader_id,
                                                            strategy_id,
                                                            client_order_id,
                                                            ts_init,
                                                        ),
                                                    ));
                                                    context.accepted = true;
                                                }

                                                let event =
                                                    parse_order_canceled_with_client_order_id(
                                                        order,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        client_order_id,
                                                        ts_init,
                                                    );
                                                log::debug!(
                                                    "Emitting OrderCanceled: venue_order_id={venue_order_id}"
                                                );
                                                self.finish_order_context(venue_order_id, &context);
                                                outgoing
                                                    .push(NautilusWsMessage::OrderCanceled(event));
                                            }
                                            OrderEventType::Expired => {
                                                if self
                                                    .terminal_order_contexts
                                                    .contains_key(&venue_order_id)
                                                {
                                                    log::trace!(
                                                        "Skipping duplicate OrderExpired: client_order_id={client_order_id}"
                                                    );
                                                    continue;
                                                }

                                                if !context.accepted {
                                                    outgoing
                                                        .push(NautilusWsMessage::OrderAccepted(
                                                        parse_order_accepted_with_client_order_id(
                                                            order,
                                                            instrument,
                                                            account_id,
                                                            trader_id,
                                                            strategy_id,
                                                            client_order_id,
                                                            ts_init,
                                                        ),
                                                    ));
                                                    context.accepted = true;
                                                }

                                                let event =
                                                    parse_order_expired_with_client_order_id(
                                                        order,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        client_order_id,
                                                        ts_init,
                                                    );
                                                log::debug!(
                                                    "Emitting OrderExpired: venue_order_id={venue_order_id}"
                                                );
                                                self.finish_order_context(venue_order_id, &context);
                                                outgoing
                                                    .push(NautilusWsMessage::OrderExpired(event));
                                            }
                                            OrderEventType::Updated => {
                                                if was_terminal {
                                                    log::trace!(
                                                        "Skipping amendment for terminal order: venue_order_id={venue_order_id}"
                                                    );
                                                    continue;
                                                }

                                                if !context.accepted {
                                                    outgoing
                                                        .push(NautilusWsMessage::OrderAccepted(
                                                        parse_order_accepted_with_client_order_id(
                                                            order,
                                                            instrument,
                                                            account_id,
                                                            trader_id,
                                                            strategy_id,
                                                            client_order_id,
                                                            ts_init,
                                                        ),
                                                    ));
                                                    context.accepted = true;
                                                }

                                                let event =
                                                    parse_order_updated_with_client_order_id(
                                                        order,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        client_order_id,
                                                        ts_init,
                                                    );
                                                self.bind_order_context(venue_order_id, context);
                                                log::debug!(
                                                    "Emitting OrderUpdated: venue_order_id={venue_order_id}"
                                                );
                                                outgoing
                                                    .push(NautilusWsMessage::OrderUpdated(event));
                                            }
                                            OrderEventType::None => {
                                                // Fills handled via user.trades, track terminal state
                                                // for race condition prevention
                                                if order.order_state == "filled" {
                                                    log::debug!(
                                                        "Recording terminal order: venue_order_id={venue_order_id}, state={}",
                                                        order.order_state
                                                    );
                                                    self.finish_order_context(
                                                        venue_order_id,
                                                        &context,
                                                    );
                                                } else if order.order_state == "rejected" {
                                                    log::debug!(
                                                        "Recording rejected order: venue_order_id={venue_order_id}"
                                                    );
                                                    self.finish_order_context(
                                                        venue_order_id,
                                                        &context,
                                                    );
                                                } else if was_terminal {
                                                    self.finish_order_context(
                                                        venue_order_id,
                                                        &context,
                                                    );
                                                } else {
                                                    log::trace!(
                                                        "No event to emit for order {}, state={}",
                                                        venue_order_id,
                                                        order.order_state
                                                    );
                                                    self.bind_order_context(
                                                        venue_order_id,
                                                        context,
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    if !outgoing.is_empty() {
                                        self.pending_outgoing.extend(outgoing);
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to deserialize user orders: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::UserTrades => {
                            // Handle both array and single object responses
                            let trades_result =
                                serde_json::from_value::<Vec<DeribitUserTradeMsg>>(data.clone())
                                    .or_else(|_| {
                                        serde_json::from_value::<DeribitUserTradeMsg>(data.clone())
                                            .map(|trade| vec![trade])
                                    });

                            match trades_result {
                                Ok(trades) => {
                                    log::debug!("Received {} user trade updates", trades.len());
                                    if self.account_id.is_none() {
                                        log::warn!("Cannot parse user trades: account_id not set");
                                        return Some(NautilusWsMessage::Raw(data.clone()));
                                    }
                                    let outgoing = self.route_user_trades(&trades, ts_init);
                                    if !outgoing.is_empty() {
                                        self.pending_outgoing.extend(outgoing);
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to deserialize user trades: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::UserPortfolio => {
                            match serde_json::from_value::<DeribitPortfolioMsg>(data.clone()) {
                                Ok(portfolio) => {
                                    // Skip zero-balance currencies (common with cross-collateral)
                                    // Only check equity and balance - initial_margin can be non-zero
                                    // for all currencies when cross-collateral is enabled
                                    if portfolio.equity.is_zero() && portfolio.balance.is_zero() {
                                        log::trace!(
                                            "Skipping zero-balance portfolio for {}",
                                            portfolio.currency
                                        );
                                        return None;
                                    }

                                    // Require account_id for parsing
                                    let Some(account_id) = self.account_id else {
                                        log::warn!("Cannot parse portfolio: account_id not set");
                                        return None;
                                    };

                                    match parse_portfolio_to_account_state(
                                        &portfolio, account_id, ts_init,
                                    ) {
                                        Ok(account_state) => {
                                            // Check for duplicate per currency
                                            let currency_key = portfolio.currency.clone();

                                            if let Some(last) =
                                                self.last_account_states.get(&currency_key)
                                                && account_state.has_same_balances_and_margins(last)
                                            {
                                                log::trace!(
                                                    "Skipping duplicate portfolio update for {}",
                                                    portfolio.currency
                                                );
                                                return None;
                                            }

                                            self.last_account_states
                                                .insert(currency_key, account_state.clone());
                                            return Some(NautilusWsMessage::AccountState(
                                                account_state,
                                            ));
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Failed to parse portfolio to AccountState: {e}"
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to deserialize portfolio: {e}");
                                }
                            }
                        }
                        _ => {
                            // Unhandled channel - return raw
                            log::trace!("Unhandled channel: {channel}");
                            return Some(NautilusWsMessage::Raw(data.clone()));
                        }
                    }
                } else {
                    log::trace!("Unknown channel: {channel}");
                    return Some(NautilusWsMessage::Raw(data.clone()));
                }
                None
            }
            DeribitWsMessage::Heartbeat(heartbeat) => {
                match heartbeat.heartbeat_type {
                    DeribitHeartbeatType::TestRequest => {
                        log::trace!(
                            "Received heartbeat test_request - responding with public/test"
                        );

                        if let Err(e) = self.handle_heartbeat_test_request().await {
                            log::error!("Failed to respond to heartbeat test_request: {e}");

                            // Return error to signal connection may be unhealthy
                            return Some(NautilusWsMessage::Error(DeribitWsError::Send(format!(
                                "Heartbeat response failed: {e}"
                            ))));
                        }
                    }
                    DeribitHeartbeatType::Heartbeat => {
                        log::trace!("Received heartbeat acknowledgment");
                    }
                }
                None
            }
            DeribitWsMessage::Error(err) => {
                log::error!("Deribit error {}: {}", err.code, err.message);
                Some(NautilusWsMessage::Error(DeribitWsError::DeribitError {
                    code: err.code,
                    message: err.message,
                }))
            }
            DeribitWsMessage::Reconnected => Some(NautilusWsMessage::Reconnected),
        }
    }

    fn order_signature(order: &DeribitOrderMsg) -> OrderSignature {
        (order.amount, order.price, order.trigger_price)
    }

    fn find_order_context(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
    ) -> Option<OrderContext> {
        self.order_contexts
            .get(&venue_order_id)
            .or_else(|| self.terminal_order_contexts.get(&venue_order_id))
            .cloned()
            .or_else(|| {
                client_order_id.and_then(|client_order_id| {
                    self.submitted_order_contexts.get(&client_order_id).cloned()
                })
            })
    }

    fn bind_order_context(&mut self, venue_order_id: VenueOrderId, context: OrderContext) {
        self.submitted_order_contexts
            .remove(&context.client_order_id);
        self.terminal_order_contexts.remove(&venue_order_id);
        self.order_contexts.insert(venue_order_id, context);
    }

    fn finish_order_context(&mut self, venue_order_id: VenueOrderId, context: &OrderContext) {
        self.order_contexts.remove(&venue_order_id);
        self.submitted_order_contexts
            .remove(&context.client_order_id);
        self.terminal_order_contexts
            .insert(venue_order_id, context.clone());
    }

    fn route_user_trades(
        &mut self,
        trades: &[DeribitUserTradeMsg],
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some(account_id) = self.account_id else {
            log::warn!("Cannot parse user trades: account_id not set");
            return Vec::new();
        };

        let mut outgoing = Vec::with_capacity(trades.len() + 1);
        let mut reports = Vec::new();

        for trade in trades {
            let instrument_name = trade.instrument_name;
            let Some((report, quote_currency)) =
                self.instruments_cache
                    .get(&instrument_name)
                    .map(|instrument| {
                        (
                            parse_user_trade_msg(trade, instrument, account_id, ts_init),
                            instrument.quote_currency(),
                        )
                    })
            else {
                log::warn!("Instrument {instrument_name} not found in cache");
                continue;
            };

            let report = match report {
                Ok(report) => report,
                Err(e) => {
                    log::warn!("Failed to parse trade {}: {e}", trade.trade_id);
                    continue;
                }
            };
            let venue_order_id = report.venue_order_id;
            let was_terminal = self.terminal_order_contexts.contains_key(&venue_order_id);
            let Some(mut context) = self.find_order_context(venue_order_id, report.client_order_id)
            else {
                log::debug!(
                    "Parsed external fill report: {} @ {}",
                    report.trade_id,
                    report.last_px
                );
                reports.push(report);
                continue;
            };

            if !context.accepted {
                outgoing.push(NautilusWsMessage::OrderAccepted(OrderAccepted::new(
                    context.trader_id,
                    context.strategy_id,
                    context.instrument_id,
                    context.client_order_id,
                    venue_order_id,
                    account_id,
                    UUID4::new(),
                    report.ts_event,
                    report.ts_init,
                    false,
                )));
                context.accepted = true;
            }

            log::debug!(
                "Parsed tracked fill event: {} @ {}",
                report.trade_id,
                report.last_px
            );
            outgoing.push(NautilusWsMessage::OrderFilled(OrderFilled::new(
                context.trader_id,
                context.strategy_id,
                context.instrument_id,
                context.client_order_id,
                venue_order_id,
                account_id,
                report.trade_id,
                context.order_side,
                context.order_type,
                report.last_qty,
                report.last_px,
                quote_currency,
                report.liquidity_side,
                UUID4::new(),
                report.ts_event,
                report.ts_init,
                false,
                report.venue_position_id,
                Some(report.commission),
                None,
            )));

            if was_terminal || trade.state == "filled" {
                self.finish_order_context(venue_order_id, &context);
            } else {
                self.bind_order_context(venue_order_id, context);
            }
        }

        if !reports.is_empty() {
            outgoing.push(NautilusWsMessage::FillReports(reports));
        }
        outgoing
    }

    /// Main message processing loop.
    ///
    /// Returns `None` when the handler should stop.
    /// Messages that need client-side handling (e.g., Reconnected) are returned.
    /// Data messages are sent directly to `out_tx` for the user stream.
    pub async fn next(&mut self) -> Option<NautilusWsMessage> {
        loop {
            if let Some(msg) = self.pending_outgoing.pop_front() {
                match msg {
                    NautilusWsMessage::Reconnected
                    | NautilusWsMessage::Authenticated(_)
                    | NautilusWsMessage::AuthenticationFailed(_) => {
                        return Some(msg);
                    }
                    _ => {
                        let _ = self.out_tx.send(msg);
                        continue;
                    }
                }
            }

            tokio::select! {
                // Process commands from client
                Some(cmd) = self.cmd_rx.recv() => {
                    self.process_command(cmd).await;
                }
                // Process raw WebSocket messages
                Some(msg) = self.raw_rx.recv() => {
                    match msg {
                        Message::Text(text) => {
                            if let Some(nautilus_msg) = self.process_raw_message(&text).await {
                                // Send data messages to user stream
                                match &nautilus_msg {
                                    NautilusWsMessage::Data(_)
                                    | NautilusWsMessage::Deltas(_)
                                    | NautilusWsMessage::Instrument(_)
                                    | NautilusWsMessage::InstrumentStatus(_)
                                    | NautilusWsMessage::OptionGreeks(_)
                                    | NautilusWsMessage::Raw(_)
                                    | NautilusWsMessage::Error(_) => {
                                        let _ = self.out_tx.send(nautilus_msg);
                                    }
                                    NautilusWsMessage::FundingRates(rates) => {
                                        let msg_to_send =
                                            NautilusWsMessage::FundingRates(rates.clone());

                                        if let Err(e) = self.out_tx.send(msg_to_send) {
                                            log::error!("Failed to send funding rates: {e}");
                                        }
                                    }
                                    NautilusWsMessage::OrderStatusReports(_)
                                    | NautilusWsMessage::FillReports(_)
                                    | NautilusWsMessage::OrderFilled(_)
                                    | NautilusWsMessage::OrderAccepted(_)
                                    | NautilusWsMessage::OrderCanceled(_)
                                    | NautilusWsMessage::OrderExpired(_)
                                    | NautilusWsMessage::OrderUpdated(_)
                                    | NautilusWsMessage::OrderRejected(_)
                                    | NautilusWsMessage::OrderCancelRejected(_)
                                    | NautilusWsMessage::OrderModifyRejected(_)
                                    | NautilusWsMessage::AccountState(_) => {
                                        let _ = self.out_tx.send(nautilus_msg);
                                    }
                                    // Return messages that need client-side handling
                                    NautilusWsMessage::Reconnected
                                    | NautilusWsMessage::Authenticated(_)
                                    | NautilusWsMessage::AuthenticationFailed(_) => {
                                        return Some(nautilus_msg);
                                    }
                                }
                            }
                        }
                        Message::Ping(data) => {
                            // Respond to ping with pong
                            if let Some(client) = &self.inner {
                                let _ = client.send_pong(data.to_vec()).await;
                            }
                        }
                        Message::Close(_) => {
                            log::debug!("Received close frame");
                        }
                        _ => {}
                    }
                }
                // Check for stop signal
                () = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::LiquiditySide, instruments::Instrument, types::Money};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{parse::parse_deribit_instrument_any, testing::load_test_json},
        http::models::{DeribitInstrument, DeribitJsonRpcResponse},
    };

    fn routing_test_handler() -> DeribitWsFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = DeribitWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            SubscriptionState::new('.'),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Some(AccountId::from("DERIBIT-001")),
            true,
            Arc::new(Mutex::new(Vec::new())),
        );
        let json = load_test_json("http_get_instruments.json");
        let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
            serde_json::from_str(&json).unwrap();
        let instrument = parse_deribit_instrument_any(
            &response.result.unwrap()[0],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap()
        .unwrap();
        handler
            .instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument);
        handler
    }

    fn order_data(order_state: &str, replaced: bool) -> serde_json::Value {
        serde_json::json!({
            "order_id": "ETH-584830574",
            "label": "O-19700101-000000-001-001-1",
            "instrument_name": "BTC-PERPETUAL",
            "direction": "buy",
            "order_type": "market",
            "order_state": order_state,
            "replaced": replaced,
            "price": 203.8,
            "amount": 2.0,
            "filled_amount": if order_state == "filled" { 2.0 } else { 0.0 },
            "average_price": 203.8,
            "creation_timestamp": 1_590_480_712_700_u64,
            "last_update_timestamp": 1_590_480_712_800_u64,
            "time_in_force": "good_til_cancelled",
            "commission": 0.00073602,
            "post_only": false,
            "reduce_only": false,
            "trigger_price": null,
            "trigger": null,
            "max_show": null,
            "api": true,
            "reject_reason": null,
            "cancel_reason": null
        })
    }

    fn trade_data() -> serde_json::Value {
        serde_json::json!({
            "trade_id": "ETH-2696068",
            "order_id": "ETH-584830574",
            "instrument_name": "BTC-PERPETUAL",
            "direction": "buy",
            "price": 203.8,
            "amount": 2.0,
            "fee": 0.00073602,
            "fee_currency": "USDT",
            "timestamp": 1_590_480_712_800_u64,
            "trade_seq": 1_966_042_u64,
            "liquidity": "T",
            "order_type": "market",
            "index_price": 203.89,
            "mark_price": 203.78,
            "tick_direction": 3,
            "state": "filled",
            "label": "O-19700101-000000-001-001-1",
            "reduce_only": false,
            "post_only": false,
            "liquidation": null,
            "profit_loss": null
        })
    }

    fn subscription(channel: &str, data: &serde_json::Value) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": { "channel": channel, "data": data }
        })
        .to_string()
    }

    #[rstest]
    #[tokio::test]
    async fn tracked_fast_fill_synthesizes_accepted_before_filled() {
        let mut handler = routing_test_handler();
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
        handler.submitted_order_contexts.insert(
            client_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id,
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: false,
                last_order_signature: None,
            },
        );
        handler.pending_requests.insert(
            42,
            PendingRequestType::Buy {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id,
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
            },
        );
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("filled", false), "trades": [] }
        });

        handler.process_raw_message(&response.to_string()).await;
        assert!(handler.pending_outgoing.is_empty());
        assert!(
            !handler
                .terminal_order_contexts
                .get(&VenueOrderId::from("ETH-584830574"))
                .unwrap()
                .accepted
        );

        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade_data()]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderAccepted(event)
                if event.client_order_id == client_order_id
        ));
        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
                    && event.trade_id.to_string() == "ETH-2696068"
                    && event.order_side == OrderSide::Buy
                    && event.order_type == OrderType::Market
                    && event.last_qty.to_string() == "2"
                    && event.last_px.to_string() == "203.8"
                    && event.liquidity_side == LiquiditySide::Taker
                    && event.commission == Some(Money::from("0.00073602 USDT"))
        ));
        assert!(handler.pending_outgoing.is_empty());

        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade_data()]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn submit_response_trades_use_tracked_event_path() {
        let mut handler = routing_test_handler();
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        let context = OrderContext {
            client_order_id,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            accepted: false,
            last_order_signature: None,
        };
        handler
            .submitted_order_contexts
            .insert(client_order_id, context.clone());
        handler.pending_requests.insert(
            42,
            PendingRequestType::Buy {
                client_order_id,
                trader_id: context.trader_id,
                strategy_id: context.strategy_id,
                instrument_id: context.instrument_id,
                order_side: context.order_side,
                order_type: context.order_type,
            },
        );
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("filled", false), "trades": [trade_data()] }
        });

        handler.process_raw_message(&response.to_string()).await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderAccepted(event)
                if event.client_order_id == client_order_id
        ));
        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
                    && event.trade_id.to_string() == "ETH-2696068"
        ));
        assert!(handler.pending_outgoing.is_empty());

        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade_data()]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn reconnect_preserves_submit_identity_before_response() {
        let mut handler = routing_test_handler();
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.submitted_order_contexts.insert(
            client_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: false,
                last_order_signature: None,
            },
        );
        handler
            .pending_requests
            .insert(42, PendingRequestType::Test);

        handler.clear_state();
        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", false)]),
            ))
            .await;

        assert!(handler.pending_requests.is_empty());
        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderAccepted(event)
                if event.client_order_id == client_order_id
        ));
        assert!(
            handler
                .order_contexts
                .get(&VenueOrderId::from("ETH-584830574"))
                .unwrap()
                .accepted
        );
    }

    #[rstest]
    #[case("cancelled")]
    #[case("expired")]
    #[tokio::test]
    async fn tracked_terminal_order_synthesizes_accepted_first(#[case] order_state: &str) {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: false,
                last_order_signature: None,
            },
        );

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data(order_state, false)]),
            ))
            .await;

        let accepted = handler.pending_outgoing.pop_front().unwrap();
        let terminal = handler.pending_outgoing.pop_front().unwrap();
        assert!(matches!(
            accepted,
            NautilusWsMessage::OrderAccepted(event)
                if event.client_order_id == client_order_id
        ));
        assert!(
            matches!(
                (order_state, terminal),
                ("cancelled", NautilusWsMessage::OrderCanceled(_))
                    | ("expired", NautilusWsMessage::OrderExpired(_))
            ),
            "unexpected terminal message for {order_state}",
        );
        assert!(handler.pending_outgoing.is_empty());
        assert!(!handler.order_contexts.contains_key(&venue_order_id));
    }

    #[rstest]
    #[case("filled")]
    #[case("open")]
    #[tokio::test]
    async fn late_fill_after_cancel_stays_on_tracked_event_path(#[case] trade_state: &str) {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: true,
                last_order_signature: None,
            },
        );

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("cancelled", false)]),
            ))
            .await;
        let mut trade = trade_data();
        trade["state"] = serde_json::Value::String(trade_state.to_string());
        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderCanceled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
        assert!(!handler.order_contexts.contains_key(&venue_order_id));
        assert!(
            handler
                .terminal_order_contexts
                .contains_key(&venue_order_id)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn tracked_order_uses_stored_client_id_when_label_changes() {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("ORIGINAL-CLIENT-ID");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: false,
                last_order_signature: None,
            },
        );
        let mut order = order_data("open", false);
        order["label"] = serde_json::Value::Null;

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderAccepted(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn subscription_accept_before_submit_response_is_not_duplicated() {
        let mut handler = routing_test_handler();
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.submitted_order_contexts.insert(
            client_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: false,
                last_order_signature: None,
            },
        );
        handler.pending_requests.insert(
            42,
            PendingRequestType::Buy {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
            },
        );

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", false)]),
            ))
            .await;
        let accepted = handler.pending_outgoing.pop_front().unwrap();
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("open", false), "trades": [] }
        });
        let duplicate = handler.process_raw_message(&response.to_string()).await;
        handler.clear_state();
        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", false)]),
            ))
            .await;

        assert!(matches!(accepted, NautilusWsMessage::OrderAccepted(_)));
        assert!(duplicate.is_none());
        assert!(handler.pending_outgoing.is_empty());
        assert!(
            handler
                .order_contexts
                .get(&VenueOrderId::from("ETH-584830574"))
                .unwrap()
                .accepted
        );
    }

    #[rstest]
    #[tokio::test]
    async fn untracked_live_order_and_fill_use_report_paths() {
        let mut handler = routing_test_handler();

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", false)]),
            ))
            .await;
        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade_data()]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderStatusReports(reports) if reports.len() == 1
        ));
        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::FillReports(reports) if reports.len() == 1
        ));
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn edit_response_without_tracked_context_uses_report_path() {
        let mut handler = routing_test_handler();
        handler.pending_requests.insert(
            42,
            PendingRequestType::Edit {
                client_order_id: ClientOrderId::from("UNKNOWN-CLIENT-ID"),
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            },
        );
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("open", true), "trades": [trade_data()] }
        });

        let message = handler.process_raw_message(&response.to_string()).await;
        let fill = handler.pending_outgoing.pop_front().unwrap();

        assert!(matches!(
            message,
            Some(NautilusWsMessage::OrderStatusReports(reports)) if reports.len() == 1
        ));
        assert!(matches!(
            fill,
            NautilusWsMessage::FillReports(reports) if reports.len() == 1
        ));
        assert!(handler.order_contexts.is_empty());
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn tracked_edit_response_routes_fill_and_deduplicates_subscription_echo() {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: true,
                last_order_signature: None,
            },
        );
        handler.pending_requests.insert(
            42,
            PendingRequestType::Edit {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            },
        );
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("open", true), "trades": [trade_data()] }
        });

        let updated = handler.process_raw_message(&response.to_string()).await;
        let filled = handler.pending_outgoing.pop_front().unwrap();
        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", true)]),
            ))
            .await;

        assert!(matches!(
            updated,
            Some(NautilusWsMessage::OrderUpdated(event))
                if event.client_order_id == client_order_id
        ));
        assert!(matches!(
            filled,
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
        assert!(!handler.order_contexts.contains_key(&venue_order_id));
        assert!(
            handler
                .terminal_order_contexts
                .get(&venue_order_id)
                .unwrap()
                .last_order_signature
                .is_some(),
        );
    }

    #[rstest]
    #[tokio::test]
    async fn subscription_edit_echo_before_response_is_not_duplicated() {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: true,
                last_order_signature: None,
            },
        );
        handler.pending_requests.insert(
            42,
            PendingRequestType::Edit {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            },
        );
        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", true)]),
            ))
            .await;
        let updated = handler.pending_outgoing.pop_front().unwrap();
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("open", true), "trades": [trade_data()] }
        });

        let duplicate = handler.process_raw_message(&response.to_string()).await;
        let filled = handler.pending_outgoing.pop_front().unwrap();

        assert!(matches!(
            updated,
            NautilusWsMessage::OrderUpdated(event)
                if event.client_order_id == client_order_id
        ));
        assert!(duplicate.is_none());
        assert!(matches!(
            filled,
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn delayed_edit_response_keeps_partial_fill_terminal() {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: true,
                last_order_signature: None,
            },
        );
        handler
            .process_raw_message(&subscription(
                "user.trades.any.any.raw",
                &serde_json::json!([trade_data()]),
            ))
            .await;
        handler.pending_outgoing.clear();
        handler.pending_requests.insert(
            42,
            PendingRequestType::Edit {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            },
        );
        let mut partial_trade = trade_data();
        partial_trade["state"] = serde_json::Value::String("open".to_string());
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "result": { "order": order_data("open", true), "trades": [partial_trade] }
        });

        let updated = handler.process_raw_message(&response.to_string()).await;
        let filled = handler.pending_outgoing.pop_front().unwrap();

        assert!(updated.is_none());
        assert!(matches!(
            filled,
            NautilusWsMessage::OrderFilled(event)
                if event.client_order_id == client_order_id
        ));
        assert!(!handler.order_contexts.contains_key(&venue_order_id));
        assert!(
            handler
                .terminal_order_contexts
                .contains_key(&venue_order_id)
        );
        assert!(handler.pending_outgoing.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn tracked_replaced_order_emits_updated_event() {
        let mut handler = routing_test_handler();
        let venue_order_id = VenueOrderId::from("ETH-584830574");
        let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        handler.order_contexts.insert(
            venue_order_id,
            OrderContext {
                client_order_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Market,
                accepted: true,
                last_order_signature: None,
            },
        );

        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([order_data("open", true)]),
            ))
            .await;

        assert!(matches!(
            handler.pending_outgoing.pop_front().unwrap(),
            NautilusWsMessage::OrderUpdated(event)
                if event.client_order_id == client_order_id
        ));

        let mut partial_fill = order_data("open", true);
        partial_fill["filled_amount"] = serde_json::json!(1.0);
        partial_fill["last_update_timestamp"] = serde_json::json!(1_590_480_712_900_u64);
        handler
            .process_raw_message(&subscription(
                "user.orders.any.any.raw",
                &serde_json::json!([partial_fill]),
            ))
            .await;

        assert!(handler.pending_outgoing.is_empty());
    }
}

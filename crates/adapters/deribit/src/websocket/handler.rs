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
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{AtomicTime, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, Data},
    events::{AccountState, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{DeribitBookMsgType, DeribitHeartbeatType, DeribitWsChannel},
    error::DeribitWsError,
    messages::{
        DeribitAuthResult, DeribitBookMsg, DeribitCancelAllByInstrumentParams, DeribitCancelParams,
        DeribitChartMsg, DeribitEditParams, DeribitHeartbeatParams, DeribitInstrumentStateMsg,
        DeribitJsonRpcRequest, DeribitOrderMsg, DeribitOrderParams, DeribitOrderResponse,
        DeribitPerpetualMsg, DeribitPortfolioMsg, DeribitQuoteMsg, DeribitSubscribeParams,
        DeribitTickerMsg, DeribitTradeMsg, DeribitUserTradeMsg, DeribitWsMessage,
        NautilusWsMessage, parse_raw_message,
    },
    parse::{
        OrderEventType, determine_order_event_type, parse_book_msg, parse_chart_msg,
        parse_order_accepted, parse_order_canceled, parse_order_expired, parse_order_updated,
        parse_perpetual_to_funding_rate, parse_quote_msg, parse_ticker_to_index_price,
        parse_ticker_to_mark_price, parse_trades_data, parse_user_order_msg, parse_user_trade_msg,
        resolution_to_bar_type,
    },
};
use crate::common::{
    consts::{DERIBIT_POST_ONLY_ERROR_CODE, DERIBIT_RATE_LIMIT_KEY_ORDER},
    parse::parse_portfolio_to_account_state,
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
    },
    /// Sell order request.
    Sell {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
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
/// Stores the original trader/strategy/client IDs from the buy/sell command
/// so they can be used when processing user.orders subscription updates.
#[derive(Debug, Clone)]
pub struct OrderContext {
    pub client_order_id: ClientOrderId,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
}

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
    request_id_counter: AtomicU64,
    pending_requests: AHashMap<u64, PendingRequestType>,
    account_id: Option<AccountId>,
    order_contexts: AHashMap<VenueOrderId, OrderContext>,
    emitted_accepted: FifoCache<VenueOrderId, 10_000>,
    terminal_orders: FifoCache<ClientOrderId, 10_000>,
    pending_bars: AHashMap<String, Bar>,
    bars_timestamp_on_close: bool,
    last_account_states: AHashMap<String, AccountState>,
    book_sequence: AHashMap<Ustr, u64>,
    pending_outgoing: VecDeque<NautilusWsMessage>,
}

impl DeribitWsFeedHandler {
    /// Creates a new feed handler.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
        account_id: Option<AccountId>,
        bars_timestamp_on_close: bool,
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
            request_id_counter: AtomicU64::new(1),
            pending_requests: AHashMap::new(),
            account_id,
            order_contexts: AHashMap::new(),
            emitted_accepted: FifoCache::new(),
            terminal_orders: FifoCache::new(),
            pending_bars: AHashMap::new(),
            bars_timestamp_on_close,
            last_account_states: AHashMap::new(),
            book_sequence: AHashMap::new(),
            pending_outgoing: VecDeque::new(),
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
        let emitted_count = self.emitted_accepted.len();
        let bars_count = self.pending_bars.len();
        let account_count = self.last_account_states.len();
        let book_count = self.book_sequence.len();
        let outgoing_count = self.pending_outgoing.len();

        self.pending_requests.clear();
        self.emitted_accepted.clear();
        self.pending_bars.clear();
        self.last_account_states.clear();
        self.book_sequence.clear();
        self.pending_outgoing.clear();

        log::debug!(
            "Reset state: pending_requests={pending_count}, emitted_accepted={emitted_count}, \
            pending_bars={bars_count}, account_states={account_count}, book_sequence={book_count}, \
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

    /// Checks if there's a pending buy/sell request for the given client_order_id.
    ///
    /// This is used to avoid emitting duplicate OrderAccepted events from the
    /// user.orders subscription when the response path will also emit an event.
    fn is_pending_order(&self, client_order_id: &ClientOrderId) -> bool {
        self.pending_requests.values().any(|req| match req {
            PendingRequestType::Buy {
                client_order_id: id,
                ..
            }
            | PendingRequestType::Sell {
                client_order_id: id,
                ..
            } => id == client_order_id,
            _ => false,
        })
    }

    /// Gets the OrderContext from a pending buy/sell request by client_order_id.
    ///
    /// Returns None if no pending request found.
    fn get_pending_order_context(&self, client_order_id: &ClientOrderId) -> Option<OrderContext> {
        for req in self.pending_requests.values() {
            match req {
                PendingRequestType::Buy {
                    client_order_id: id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                }
                | PendingRequestType::Sell {
                    client_order_id: id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                } => {
                    if id == client_order_id {
                        return Some(OrderContext {
                            client_order_id: *id,
                            trader_id: *trader_id,
                            strategy_id: *strategy_id,
                            instrument_id: *instrument_id,
                        });
                    }
                }
                _ => {}
            }
        }
        None
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

        let request = DeribitJsonRpcRequest::new(
            request_id,
            "public/subscribe",
            DeribitSubscribeParams {
                channels: channels.clone(),
            },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Subscribing to channels: request_id={request_id}, channels={channels:?}");
        self.send_with_retry(payload, None).await
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

        let request = DeribitJsonRpcRequest::new(
            request_id,
            "public/unsubscribe",
            DeribitSubscribeParams {
                channels: channels.clone(),
            },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Unsubscribing from channels: request_id={request_id}, channels={channels:?}");
        self.send_with_retry(payload, None).await
    }

    /// Handles enabling heartbeat.
    async fn handle_set_heartbeat(&mut self, interval: u64) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests
            .insert(request_id, PendingRequestType::SetHeartbeat);

        let request = DeribitJsonRpcRequest::new(
            request_id,
            "public/set_heartbeat",
            DeribitHeartbeatParams { interval },
        );

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!(
            "Enabling heartbeat with interval: request_id={request_id}, interval={interval} seconds"
        );
        self.send_with_retry(payload, None).await
    }

    /// Responds to a heartbeat test_request.
    async fn handle_heartbeat_test_request(&mut self) -> Result<(), DeribitWsError> {
        let request_id = self.next_request_id();

        // Track this request for response correlation
        self.pending_requests
            .insert(request_id, PendingRequestType::Test);

        let request = DeribitJsonRpcRequest::new(request_id, "public/test", serde_json::json!({}));

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::trace!("Responding to heartbeat test_request: request_id={request_id}");
        self.send_with_retry(payload, None).await
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

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Buy {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let request = DeribitJsonRpcRequest::new(request_id, "private/buy", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Sending buy order: request_id={request_id}");
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

        self.pending_requests.insert(
            request_id,
            PendingRequestType::Sell {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let request = DeribitJsonRpcRequest::new(request_id, "private/sell", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Sending sell order: request_id={request_id}");
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

        let request = DeribitJsonRpcRequest::new(request_id, "private/edit", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Sending edit order: request_id={request_id}, order_id={order_id}");
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

        let request = DeribitJsonRpcRequest::new(request_id, "private/cancel", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Sending cancel order: request_id={request_id}, order_id={order_id}");
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

        let request =
            DeribitJsonRpcRequest::new(request_id, "private/cancel_all_by_instrument", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!(
            "Sending cancel_all_by_instrument: request_id={request_id}, instrument={instrument_name}"
        );
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

        let request = DeribitJsonRpcRequest::new(request_id, "private/get_order_state", params);

        let payload =
            serde_json::to_string(&request).map_err(|e| DeribitWsError::Json(e.to_string()))?;

        log::debug!("Sending get_order_state: request_id={request_id}, order_id={order_id}");
        self.send_with_retry(payload, Some(DERIBIT_RATE_LIMIT_KEY_ORDER.as_slice()))
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

                let request = DeribitJsonRpcRequest::new(request_id, "public/auth", auth_params);
                match serde_json::to_string(&request) {
                    Ok(payload) => {
                        if let Err(e) = self.send_with_retry(payload, None).await {
                            log::error!("Authentication send failed: {e}");
                            self.auth_tracker.fail(format!("Send failed: {e}"));
                        }
                    }
                    Err(e) => {
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
                log::info!("Handler received {} instruments", instruments.len());
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
                                log::error!(
                                    "Authentication failed: code={}, message={}, request_id={}",
                                    error.code,
                                    error.message,
                                    request_id
                                );
                                self.auth_tracker.fail(format!(
                                    "Authentication error code={}: {}",
                                    error.code, error.message
                                ));
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
                                        log::error!(
                                            "Failed to parse auth result: request_id={request_id}, error={e}"
                                        );
                                        self.auth_tracker
                                            .fail(format!("Failed to parse auth result: {e}"));
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
                                // Mark channels as failed so they can be retried
                                for ch in &channels {
                                    self.subscriptions_state.confirm_unsubscribe(ch);
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
                                // Confirm each channel in the unsubscription
                                for ch in &channels {
                                    self.subscriptions_state.confirm_unsubscribe(ch);
                                    log::debug!("Unsubscription confirmed: {ch}");
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
                                        // Cancel confirmed - don't emit or remove context here.
                                        // Let user.orders stream handle the cancel event to avoid
                                        // duplicates. The stream will use the context for correct
                                        // trader/strategy IDs and then remove it.
                                        log::debug!(
                                            "Cancel confirmed: venue_order_id={}, client_order_id={}, state={} (waiting for user.orders)",
                                            order_msg.order_id,
                                            client_order_id,
                                            order_msg.order_state
                                        );
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
                                        log::info!(
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
                        }
                        | PendingRequestType::Sell {
                            client_order_id,
                            trader_id,
                            strategy_id,
                            instrument_id,
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

                                        self.order_contexts.insert(
                                            venue_order_id,
                                            OrderContext {
                                                client_order_id,
                                                trader_id,
                                                strategy_id,
                                                instrument_id,
                                            },
                                        );

                                        // Skip OrderAccepted if order already reached terminal state
                                        if self.terminal_orders.contains(&client_order_id) {
                                            log::debug!(
                                                "Skipping OrderAccepted for terminal order: client_order_id={client_order_id}"
                                            );
                                            self.emitted_accepted.add(venue_order_id);
                                        } else if order_state == "filled" {
                                            // Order went directly Submitted -> Filled (e.g., market orders)
                                            log::debug!(
                                                "Skipping OrderAccepted for already filled order: venue_order_id={venue_order_id}, client_order_id={client_order_id}"
                                            );
                                            self.terminal_orders.add(client_order_id);
                                            self.emitted_accepted.add(venue_order_id);
                                        } else {
                                            let instrument_name_ustr = Ustr::from(
                                                order_response.order.instrument_name.as_str(),
                                            );
                                            if let Some(instrument) =
                                                self.instruments_cache.get(&instrument_name_ustr)
                                            {
                                                if let Some(account_id) = self.account_id {
                                                    let event = parse_order_accepted(
                                                        &order_response.order,
                                                        instrument,
                                                        account_id,
                                                        trader_id,
                                                        strategy_id,
                                                        ts_init,
                                                    );
                                                    // Mark OrderAccepted as emitted to prevent duplicate from subscription
                                                    self.emitted_accepted.add(venue_order_id);
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
                                        return Some(NautilusWsMessage::OrderRejected(
                                            OrderRejected::new(
                                                trader_id,
                                                strategy_id,
                                                instrument_id,
                                                client_order_id,
                                                self.account_id
                                                    .unwrap_or(AccountId::new("DERIBIT-UNKNOWN")),
                                                ustr::ustr(&format!(
                                                    "Failed to parse response: {e}"
                                                )),
                                                UUID4::new(),
                                                ts_init,
                                                ts_init,
                                                false,
                                                false,
                                            ),
                                        ));
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
                                        log::info!(
                                            "Order updated: venue_order_id={}, client_order_id={}, state={}",
                                            venue_order_id,
                                            client_order_id,
                                            order_response.order.order_state
                                        );

                                        self.order_contexts.insert(
                                            venue_order_id,
                                            OrderContext {
                                                client_order_id,
                                                trader_id,
                                                strategy_id,
                                                instrument_id,
                                            },
                                        );

                                        let instrument_name_ustr = Ustr::from(
                                            order_response.order.instrument_name.as_str(),
                                        );
                                        if let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name_ustr)
                                        {
                                            if let Some(account_id) = self.account_id {
                                                let event = parse_order_updated(
                                                    &order_response.order,
                                                    instrument,
                                                    account_id,
                                                    trader_id,
                                                    strategy_id,
                                                    ts_init,
                                                );
                                                return Some(NautilusWsMessage::OrderUpdated(
                                                    event,
                                                ));
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
                                        return Some(NautilusWsMessage::OrderModifyRejected(
                                            OrderModifyRejected::new(
                                                trader_id,
                                                strategy_id,
                                                instrument_id,
                                                client_order_id,
                                                ustr::ustr(&format!(
                                                    "Failed to parse response: {e}"
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
                                        log::info!(
                                            "Order state received: venue_order_id={}, client_order_id={}, state={}",
                                            order_msg.order_id,
                                            client_order_id,
                                            order_msg.order_state
                                        );

                                        // Convert to OrderStatusReport
                                        let instrument_name_ustr =
                                            Ustr::from(order_msg.instrument_name.as_str());
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
                                    let data_vec =
                                        parse_trades_data(trades, &self.instruments_cache, ts_init);
                                    if data_vec.is_empty() {
                                        log::debug!(
                                            "No trades parsed - instrument cache size: {}",
                                            self.instruments_cache.len()
                                        );
                                    } else {
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
                                        if book_msg.msg_type == DeribitBookMsgType::Change
                                            && let Some(prev_id) = book_msg.prev_change_id
                                            && let Some(&last_id) =
                                                self.book_sequence.get(&book_msg.instrument_name)
                                            && prev_id != last_id
                                        {
                                            log::warn!(
                                                "Book sequence gap for {}: expected prev_change_id={}, was {}",
                                                book_msg.instrument_name,
                                                last_id,
                                                prev_id
                                            );
                                        }
                                        self.book_sequence
                                            .insert(book_msg.instrument_name, book_msg.change_id);

                                        match parse_book_msg(&book_msg, instrument, ts_init) {
                                            Ok(deltas) => {
                                                return Some(NautilusWsMessage::Deltas(deltas));
                                            }
                                            Err(e) => {
                                                log::warn!("Failed to parse book message: {e}");
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
                            // Parse ticker to emit both MarkPrice and IndexPrice
                            // When subscribed to either mark_prices or index_prices, we emit both
                            // as traders typically need both for analysis
                            if let Ok(ticker_msg) =
                                serde_json::from_value::<DeribitTickerMsg>(data.clone())
                                && let Some(instrument) =
                                    self.instruments_cache.get(&ticker_msg.instrument_name)
                            {
                                let mark_price =
                                    parse_ticker_to_mark_price(&ticker_msg, instrument, ts_init);
                                let index_price =
                                    parse_ticker_to_index_price(&ticker_msg, instrument, ts_init);

                                return Some(NautilusWsMessage::Data(vec![
                                    Data::MarkPriceUpdate(mark_price),
                                    Data::IndexPriceUpdate(index_price),
                                ]));
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
                            if let Ok(quote_msg) =
                                serde_json::from_value::<DeribitQuoteMsg>(data.clone())
                                && let Some(instrument) =
                                    self.instruments_cache.get(&quote_msg.instrument_name)
                            {
                                match parse_quote_msg(&quote_msg, instrument, ts_init) {
                                    Ok(quote) => {
                                        return Some(NautilusWsMessage::Data(vec![Data::Quote(
                                            quote,
                                        )]));
                                    }
                                    Err(e) => {
                                        log::warn!("Failed to parse quote message: {e}");
                                    }
                                }
                            }
                        }
                        DeribitWsChannel::InstrumentState => {
                            // Parse instrument state lifecycle notifications
                            match serde_json::from_value::<DeribitInstrumentStateMsg>(data.clone())
                            {
                                Ok(state_msg) => {
                                    log::info!(
                                        "Instrument state change: {} -> {} (timestamp: {})",
                                        state_msg.instrument_name,
                                        state_msg.state,
                                        state_msg.timestamp
                                    );
                                    // Return raw data for consumers to handle state changes
                                    // TODO: Optionally emit instrument updates when instrument transitions to 'started'
                                    return Some(NautilusWsMessage::Raw(data.clone()));
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse instrument state message: {e}");
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

                                                match parse_chart_msg(
                                                    &chart_msg,
                                                    bar_type,
                                                    price_precision,
                                                    size_precision,
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
                                        let instrument_name =
                                            Ustr::from(order.instrument_name.as_str());

                                        let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name)
                                        else {
                                            log::warn!(
                                                "Instrument {instrument_name} not found in cache"
                                            );
                                            continue;
                                        };

                                        // Look up OrderContext for this order
                                        // First check order_contexts (for orders whose response has been processed)
                                        // Then check pending_requests (for orders whose response hasn't arrived yet)
                                        // If neither found, this is a true external order
                                        let context =
                                            self.order_contexts.get(&venue_order_id).cloned();

                                        // Extract client_order_id from order label for pending check
                                        let label_client_order_id = order
                                            .label
                                            .as_ref()
                                            .filter(|l| !l.is_empty())
                                            .map(ClientOrderId::new);

                                        // Check for pending request if not in order_contexts
                                        let pending_context = if context.is_none() {
                                            if let Some(client_id) = &label_client_order_id {
                                                self.get_pending_order_context(client_id)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };

                                        // Check if order has a pending request for context resolution
                                        let has_pending_request =
                                            if let Some(client_id) = &label_client_order_id {
                                                self.is_pending_order(client_id)
                                            } else {
                                                false
                                            };

                                        let effective_context = context.or(pending_context);
                                        let is_known_order =
                                            effective_context.is_some() || has_pending_request;

                                        // Determine event type based on order state
                                        let event_type = determine_order_event_type(
                                            &order.order_state,
                                            !is_known_order, // is_new if we don't know about it
                                            false,           // not from edit response
                                        );

                                        let (trader_id, strategy_id, client_order_id) =
                                            if let Some(ctx) = effective_context {
                                                (
                                                    ctx.trader_id,
                                                    ctx.strategy_id,
                                                    ctx.client_order_id,
                                                )
                                            } else {
                                                // External order - use default values
                                                // Note: These won't match any strategy, which is correct
                                                (
                                                    TraderId::new("EXTERNAL-000"),
                                                    StrategyId::new("EXTERNAL"),
                                                    ClientOrderId::new(venue_order_id_str),
                                                )
                                            };

                                        match event_type {
                                            OrderEventType::Accepted => {
                                                // Skip if order already reached terminal state (race condition)
                                                if self.terminal_orders.contains(&client_order_id) {
                                                    log::debug!(
                                                        "Skipping OrderAccepted for terminal order: client_order_id={client_order_id}"
                                                    );
                                                    continue;
                                                }

                                                // Check if we already emitted OrderAccepted for this order
                                                // This prevents duplicates from both response and subscription paths
                                                if self.emitted_accepted.contains(&venue_order_id) {
                                                    log::trace!(
                                                        "Skipping duplicate OrderAccepted: venue_order_id={venue_order_id}"
                                                    );
                                                    continue;
                                                }

                                                let event = parse_order_accepted(
                                                    order,
                                                    instrument,
                                                    account_id,
                                                    trader_id,
                                                    strategy_id,
                                                    ts_init,
                                                );

                                                // Mark OrderAccepted as emitted
                                                self.emitted_accepted.add(venue_order_id);

                                                log::debug!(
                                                    "Emitting OrderAccepted: venue_order_id={venue_order_id}, is_known={is_known_order}"
                                                );
                                                outgoing
                                                    .push(NautilusWsMessage::OrderAccepted(event));
                                            }
                                            OrderEventType::Canceled => {
                                                let event = parse_order_canceled(
                                                    order,
                                                    instrument,
                                                    account_id,
                                                    trader_id,
                                                    strategy_id,
                                                    ts_init,
                                                );
                                                log::debug!(
                                                    "Emitting OrderCanceled: venue_order_id={venue_order_id}"
                                                );
                                                self.terminal_orders.add(client_order_id);
                                                self.order_contexts.remove(&venue_order_id);
                                                self.emitted_accepted.remove(&venue_order_id);
                                                outgoing
                                                    .push(NautilusWsMessage::OrderCanceled(event));
                                            }
                                            OrderEventType::Expired => {
                                                let event = parse_order_expired(
                                                    order,
                                                    instrument,
                                                    account_id,
                                                    trader_id,
                                                    strategy_id,
                                                    ts_init,
                                                );
                                                log::debug!(
                                                    "Emitting OrderExpired: venue_order_id={venue_order_id}"
                                                );
                                                self.terminal_orders.add(client_order_id);
                                                self.order_contexts.remove(&venue_order_id);
                                                self.emitted_accepted.remove(&venue_order_id);
                                                outgoing
                                                    .push(NautilusWsMessage::OrderExpired(event));
                                            }
                                            OrderEventType::Updated => {
                                                // Emit OrderStatusReport for updates
                                                // This includes quantity/price changes from modify
                                                match parse_user_order_msg(
                                                    order, instrument, account_id, ts_init,
                                                ) {
                                                    Ok(report) => {
                                                        log::debug!(
                                                            "Emitting OrderStatusReport (updated): venue_order_id={venue_order_id}"
                                                        );
                                                        outgoing.push(
                                                            NautilusWsMessage::OrderStatusReports(
                                                                vec![report],
                                                            ),
                                                        );
                                                    }
                                                    Err(e) => {
                                                        log::warn!(
                                                            "Failed to parse order update: {e}"
                                                        );
                                                    }
                                                }
                                            }
                                            OrderEventType::None => {
                                                // Fills handled via user.trades, track terminal state
                                                // for race condition prevention
                                                if matches!(
                                                    order.order_state.as_str(),
                                                    "filled" | "rejected"
                                                ) {
                                                    log::debug!(
                                                        "Recording terminal order: venue_order_id={venue_order_id}, state={}",
                                                        order.order_state
                                                    );
                                                    self.terminal_orders.add(client_order_id);
                                                    self.order_contexts.remove(&venue_order_id);
                                                    self.emitted_accepted.remove(&venue_order_id);
                                                } else {
                                                    log::trace!(
                                                        "No event to emit for order {}, state={}",
                                                        venue_order_id,
                                                        order.order_state
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

                                    let Some(account_id) = self.account_id else {
                                        log::warn!("Cannot parse user trades: account_id not set");
                                        return Some(NautilusWsMessage::Raw(data.clone()));
                                    };

                                    let mut reports = Vec::with_capacity(trades.len());
                                    for trade in &trades {
                                        let instrument_name =
                                            Ustr::from(trade.instrument_name.as_str());
                                        if let Some(instrument) =
                                            self.instruments_cache.get(&instrument_name)
                                        {
                                            match parse_user_trade_msg(
                                                trade, instrument, account_id, ts_init,
                                            ) {
                                                Ok(report) => {
                                                    log::debug!(
                                                        "Parsed fill report: {} @ {}",
                                                        report.trade_id,
                                                        report.last_px
                                                    );
                                                    reports.push(report);
                                                }
                                                Err(e) => {
                                                    log::warn!(
                                                        "Failed to parse trade {}: {e}",
                                                        trade.trade_id
                                                    );
                                                }
                                            }
                                        } else {
                                            log::warn!(
                                                "Instrument {instrument_name} not found in cache"
                                            );
                                        }
                                    }

                                    if !reports.is_empty() {
                                        return Some(NautilusWsMessage::FillReports(reports));
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

    /// Main message processing loop.
    ///
    /// Returns `None` when the handler should stop.
    /// Messages that need client-side handling (e.g., Reconnected) are returned.
    /// Data messages are sent directly to `out_tx` for the user stream.
    pub async fn next(&mut self) -> Option<NautilusWsMessage> {
        loop {
            if let Some(msg) = self.pending_outgoing.pop_front() {
                match msg {
                    NautilusWsMessage::Reconnected | NautilusWsMessage::Authenticated(_) => {
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
                                    | NautilusWsMessage::Authenticated(_) => {
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
                            log::info!("Received close frame");
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

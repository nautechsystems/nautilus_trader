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

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use ahash::AHashMap;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::Data,
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
    enums::{DeribitHeartbeatType, DeribitWsChannel},
    error::DeribitWsError,
    messages::{
        DeribitAuthResult, DeribitBookMsg, DeribitChartMsg, DeribitHeartbeatParams,
        DeribitInstrumentStateMsg, DeribitJsonRpcRequest, DeribitPerpetualMsg, DeribitQuoteMsg,
        DeribitSubscribeParams, DeribitTickerMsg, DeribitTradeMsg, DeribitWsMessage,
        NautilusWsMessage, parse_raw_message,
    },
    parse::{
        parse_book_msg, parse_chart_msg, parse_perpetual_to_funding_rate, parse_quote_msg,
        parse_ticker_to_index_price, parse_ticker_to_mark_price, parse_trades_data,
        resolution_to_bar_type,
    },
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
}

/// Deribit WebSocket feed handler.
///
/// Runs in a dedicated Tokio task, processing commands and raw WebSocket messages.
#[allow(missing_debug_implementations)]
#[allow(dead_code)] // Fields reserved for future features
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
    /// Pending requests awaiting response, keyed by request ID.
    pending_requests: AHashMap<u64, PendingRequestType>,
}

impl DeribitWsFeedHandler {
    /// Creates a new feed handler.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
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
        }
    }

    /// Generates a unique request ID.
    fn next_request_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Returns the current timestamp.
    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Sends a message over the WebSocket with retry logic.
    async fn send_with_retry(
        &self,
        payload: String,
        rate_limit_keys: Option<Vec<String>>,
    ) -> Result<(), DeribitWsError> {
        if let Some(client) = &self.inner {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || async {
                        client
                            .send_text(payload.clone(), rate_limit_keys.clone())
                            .await
                            .map_err(|e| DeribitWsError::Send(e.to_string()))
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

        tracing::debug!(request_id, "Subscribing to channels: {:?}", channels);
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

        tracing::debug!(request_id, "Unsubscribing from channels: {:?}", channels);
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

        tracing::debug!(
            request_id,
            "Enabling heartbeat with interval: {} seconds",
            interval
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

        tracing::trace!(request_id, "Responding to heartbeat test_request");
        self.send_with_retry(payload, None).await
    }

    /// Processes a command from the client.
    async fn process_command(&mut self, cmd: HandlerCommand) {
        match cmd {
            HandlerCommand::SetClient(client) => {
                tracing::debug!("Setting WebSocket client");
                self.inner = Some(client);
            }
            HandlerCommand::Disconnect => {
                tracing::debug!("Disconnecting WebSocket");
                if let Some(client) = self.inner.take() {
                    client.disconnect().await;
                }
            }
            HandlerCommand::Authenticate { auth_params } => {
                let request_id = self.next_request_id();
                tracing::debug!(request_id, "Authenticating...");

                // Track this request for response correlation
                self.pending_requests
                    .insert(request_id, PendingRequestType::Authenticate);

                let request = DeribitJsonRpcRequest::new(request_id, "public/auth", auth_params);
                match serde_json::to_string(&request) {
                    Ok(payload) => {
                        if let Err(e) = self.send_with_retry(payload, None).await {
                            tracing::error!("Authentication send failed: {e}");
                            self.auth_tracker.fail(format!("Send failed: {e}"));
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to serialize auth request: {e}");
                        self.auth_tracker.fail(format!("Serialization failed: {e}"));
                    }
                }
            }
            HandlerCommand::SetHeartbeat { interval } => {
                if let Err(e) = self.handle_set_heartbeat(interval).await {
                    tracing::error!("Set heartbeat failed: {e}");
                }
            }
            HandlerCommand::InitializeInstruments(instruments) => {
                tracing::debug!("Initializing {} instruments", instruments.len());
                self.instruments_cache.clear();
                for inst in instruments {
                    self.instruments_cache
                        .insert(inst.raw_symbol().inner(), inst);
                }
            }
            HandlerCommand::UpdateInstrument(instrument) => {
                tracing::trace!("Updating instrument: {}", instrument.raw_symbol());
                self.instruments_cache
                    .insert(instrument.raw_symbol().inner(), *instrument);
            }
            HandlerCommand::Subscribe { channels } => {
                if let Err(e) = self.handle_subscribe(channels).await {
                    tracing::error!("Subscribe failed: {e}");
                }
            }
            HandlerCommand::Unsubscribe { channels } => {
                if let Err(e) = self.handle_unsubscribe(channels).await {
                    tracing::error!("Unsubscribe failed: {e}");
                }
            }
        }
    }

    /// Processes a raw WebSocket message.
    async fn process_raw_message(&mut self, text: &str) -> Option<NautilusWsMessage> {
        // Check for reconnection signal
        if text == RECONNECTED {
            tracing::info!("Received reconnection signal");
            return Some(NautilusWsMessage::Reconnected);
        }

        // Parse the JSON-RPC message
        let ws_msg = match parse_raw_message(text) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("Failed to parse message: {e}");
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
                            // Parse authentication result
                            if let Some(result) = &response.result {
                                match serde_json::from_value::<DeribitAuthResult>(result.clone()) {
                                    Ok(auth_result) => {
                                        self.auth_tracker.succeed();
                                        tracing::info!(
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
                                        tracing::error!(
                                            request_id,
                                            "Failed to parse auth result: {e}"
                                        );
                                        self.auth_tracker
                                            .fail(format!("Failed to parse auth result: {e}"));
                                    }
                                }
                            }
                        }
                        PendingRequestType::Subscribe { channels } => {
                            // Confirm each channel in the subscription
                            for ch in &channels {
                                self.subscriptions_state.confirm_subscribe(ch);
                                tracing::debug!("Subscription confirmed: {ch}");
                            }
                        }
                        PendingRequestType::Unsubscribe { channels } => {
                            // Confirm each channel in the unsubscription
                            for ch in &channels {
                                self.subscriptions_state.confirm_unsubscribe(ch);
                                tracing::debug!("Unsubscription confirmed: {ch}");
                            }
                        }
                        PendingRequestType::SetHeartbeat => {
                            tracing::debug!("Heartbeat enabled (request_id={})", request_id);
                        }
                        PendingRequestType::Test => {
                            tracing::trace!(
                                "Heartbeat test acknowledged (request_id={})",
                                request_id
                            );
                        }
                    }
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
                                    tracing::debug!("Received {} trades", trades.len());
                                    let data_vec =
                                        parse_trades_data(trades, &self.instruments_cache, ts_init);
                                    if !data_vec.is_empty() {
                                        tracing::debug!("Parsed {} trade ticks", data_vec.len());
                                        return Some(NautilusWsMessage::Data(data_vec));
                                    } else {
                                        tracing::debug!(
                                            "No trades parsed - instrument cache size: {}",
                                            self.instruments_cache.len()
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to deserialize trades: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::Book => {
                            // Parse order book messages
                            if let Ok(book_msg) =
                                serde_json::from_value::<DeribitBookMsg>(data.clone())
                                && let Some(instrument) =
                                    self.instruments_cache.get(&book_msg.instrument_name)
                            {
                                match parse_book_msg(&book_msg, instrument, ts_init) {
                                    Ok(deltas) => {
                                        return Some(NautilusWsMessage::Deltas(deltas));
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to parse book message: {e}");
                                    }
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
                                            if let Some(funding_rate) =
                                                parse_perpetual_to_funding_rate(
                                                    &perpetual_msg,
                                                    instrument,
                                                    ts_init,
                                                )
                                            {
                                                return Some(NautilusWsMessage::FundingRates(
                                                    vec![funding_rate],
                                                ));
                                            } else {
                                                tracing::warn!(
                                                    "Failed to create funding rate from perpetual msg"
                                                );
                                            }
                                        } else {
                                            tracing::warn!(
                                                "Instrument {} not found in cache (cache size: {})",
                                                instrument_name,
                                                self.instruments_cache.len()
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
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
                                        tracing::warn!("Failed to parse quote message: {e}");
                                    }
                                }
                            }
                        }
                        DeribitWsChannel::InstrumentState => {
                            // Parse instrument state lifecycle notifications
                            match serde_json::from_value::<DeribitInstrumentStateMsg>(data.clone())
                            {
                                Ok(state_msg) => {
                                    tracing::info!(
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
                                    tracing::warn!("Failed to parse instrument state message: {e}");
                                }
                            }
                        }
                        DeribitWsChannel::ChartTrades => {
                            // Parse chart.trades messages into Bar objects
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

                                        // Create BarType from resolution and instrument
                                        match resolution_to_bar_type(instrument_id, resolution) {
                                            Ok(bar_type) => {
                                                let price_precision = instrument.price_precision();
                                                let size_precision = instrument.size_precision();

                                                match parse_chart_msg(
                                                    &chart_msg,
                                                    bar_type,
                                                    price_precision,
                                                    size_precision,
                                                    ts_init,
                                                ) {
                                                    Ok(bar) => {
                                                        tracing::debug!("Parsed bar: {:?}", bar);
                                                        return Some(NautilusWsMessage::Data(
                                                            vec![Data::Bar(bar)],
                                                        ));
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Failed to parse chart message to bar: {e}"
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to create BarType from resolution {}: {e}",
                                                    resolution
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            "Instrument {} not found in cache for chart data",
                                            instrument_name
                                        );
                                    }
                                }
                            }
                        }
                        _ => {
                            // Unhandled channel - return raw
                            tracing::trace!("Unhandled channel: {channel}");
                            return Some(NautilusWsMessage::Raw(data.clone()));
                        }
                    }
                } else {
                    tracing::trace!("Unknown channel: {channel}");
                    return Some(NautilusWsMessage::Raw(data.clone()));
                }
                None
            }
            DeribitWsMessage::Heartbeat(heartbeat) => {
                match heartbeat.heartbeat_type {
                    DeribitHeartbeatType::TestRequest => {
                        tracing::trace!(
                            "Received heartbeat test_request - responding with public/test"
                        );
                        if let Err(e) = self.handle_heartbeat_test_request().await {
                            tracing::error!("Failed to respond to heartbeat test_request: {e}");
                        }
                    }
                    DeribitHeartbeatType::Heartbeat => {
                        tracing::trace!("Received heartbeat acknowledgment");
                    }
                }
                None
            }
            DeribitWsMessage::Error(err) => {
                tracing::error!("Deribit error {}: {}", err.code, err.message);
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
                                            tracing::error!("Failed to send funding rates: {e}");
                                        }
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
                            tracing::info!("Received close frame");
                        }
                        _ => {}
                    }
                }
                // Check for stop signal
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }
}

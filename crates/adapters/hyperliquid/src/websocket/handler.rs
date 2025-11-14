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

//! WebSocket message handler for Hyperliquid.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::BarType,
    identifiers::AccountId,
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
    error::HyperliquidWsError,
    messages::{
        ExecutionReport, HyperliquidWsMessage, HyperliquidWsRequest, NautilusWsMessage,
        SubscriptionRequest, WsUserEventData,
    },
    parse::{
        parse_ws_candle, parse_ws_fill_report, parse_ws_order_book_deltas,
        parse_ws_order_status_report, parse_ws_quote_tick, parse_ws_trade_tick,
    },
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Subscribe to the given subscriptions.
    Subscribe {
        subscriptions: Vec<SubscriptionRequest>,
    },
    /// Unsubscribe from the given subscriptions.
    Unsubscribe {
        subscriptions: Vec<SubscriptionRequest>,
    },
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
    /// Add a bar type mapping for candle parsing.
    AddBarType { key: String, bar_type: BarType },
    /// Remove a bar type mapping.
    RemoveBarType { key: String },
}

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    _auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    retry_manager: RetryManager<HyperliquidWsError>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    bar_types_cache: AHashMap<String, BarType>,
    account_id: Option<AccountId>,
    message_buffer: Vec<NautilusWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: Option<AccountId>,
        _auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            _auth_tracker,
            subscriptions,
            retry_manager: create_websocket_retry_manager(),
            instruments_cache: AHashMap::new(),
            bar_types_cache: AHashMap::new(),
            account_id,
            message_buffer: Vec::new(),
        }
    }

    /// Send a message to the output channel.
    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), String> {
        self.out_tx
            .send(msg)
            .map_err(|e| format!("Failed to send message: {e}"))
    }

    /// Check if the handler has received a stop signal.
    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    /// Sends a WebSocket message with retry logic.
    async fn send_with_retry(&self, payload: String) -> anyhow::Result<()> {
        if let Some(client) = &self.client {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client.send_text(payload, None).await.map_err(|e| {
                                HyperliquidWsError::ClientError(format!("Send failed: {e}"))
                            })
                        }
                    },
                    should_retry_hyperliquid_error,
                    create_hyperliquid_timeout_error,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            Err(anyhow::anyhow!("No WebSocket client available"))
        }
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        if !self.message_buffer.is_empty() {
            return Some(self.message_buffer.remove(0));
        }

        let clock = get_atomic_clock_realtime();

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            tracing::debug!("Setting WebSocket client in handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            tracing::debug!("Handler received disconnect command");
                            if let Some(ref client) = self.client {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                        HandlerCommand::Subscribe { subscriptions } => {
                            for subscription in subscriptions {
                                let key = subscription_to_key(&subscription);
                                self.subscriptions.mark_subscribe(&key);

                                let request = HyperliquidWsRequest::Subscribe { subscription };
                                match serde_json::to_string(&request) {
                                    Ok(payload) => {
                                        tracing::debug!("Sending subscribe payload: {}", payload);
                                        if let Err(e) = self.send_with_retry(payload).await {
                                            tracing::error!("Error subscribing to {key}: {e}");
                                            self.subscriptions.mark_failure(&key);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Error serializing subscription for {key}: {e}");
                                        self.subscriptions.mark_failure(&key);
                                    }
                                }
                            }
                        }
                        HandlerCommand::Unsubscribe { subscriptions } => {
                            for subscription in subscriptions {
                                let key = subscription_to_key(&subscription);
                                self.subscriptions.mark_unsubscribe(&key);

                                let request = HyperliquidWsRequest::Unsubscribe { subscription };
                                match serde_json::to_string(&request) {
                                    Ok(payload) => {
                                        tracing::debug!("Sending unsubscribe payload: {}", payload);
                                        if let Err(e) = self.send_with_retry(payload).await {
                                            tracing::error!("Error unsubscribing from {key}: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Error serializing unsubscription for {key}: {e}");
                                    }
                                }
                            }
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                // Store by full symbol only (primary key - guarantees uniqueness)
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            let full_symbol = inst.symbol().inner();
                            let coin = inst.raw_symbol().inner();

                            // Store by full symbol (primary key)
                            self.instruments_cache.insert(full_symbol, inst.clone());

                            // Also store by coin for WebSocket message lookups
                            // Check for collision (different instrument already mapped to this coin)
                            if let Some(existing) = self.instruments_cache.get(&coin)
                                && existing.id() != inst.id()
                            {
                                tracing::warn!(
                                    "Coin '{}' mapping changed from {} to {} - Hyperliquid WebSocket messages \
                                    only include coin identifiers, so subscribing to both spot and perp for the \
                                    same coin is not supported. Last subscription wins.",
                                    coin,
                                    existing.id(),
                                    inst.id()
                                );
                            }
                            self.instruments_cache.insert(coin, inst);
                        }
                        HandlerCommand::AddBarType { key, bar_type } => {
                            self.bar_types_cache.insert(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.bar_types_cache.remove(&key);
                        }
                    }
                    continue;
                }

                Some(raw_msg) = self.raw_rx.recv() => {
                    match raw_msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                tracing::info!("Received RECONNECTED sentinel");
                                return Some(NautilusWsMessage::Reconnected);
                            }

                            match serde_json::from_str::<HyperliquidWsMessage>(&text) {
                                Ok(msg) => {
                                    let ts_init = clock.get_time_ns();
                                    let nautilus_messages = Self::parse_to_nautilus_messages(
                                        msg,
                                        &self.instruments_cache,
                                        &self.bar_types_cache,
                                        self.account_id,
                                        ts_init,
                                    );

                                    if !nautilus_messages.is_empty() {
                                        let mut iter = nautilus_messages.into_iter();
                                        let first = iter.next().unwrap();
                                        self.message_buffer.extend(iter);
                                        return Some(first);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing WebSocket message: {e}, text: {text}");
                                }
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await {
                                tracing::error!("Error sending pong: {e}");
                            }
                        }
                        Message::Close(_) => {
                            tracing::info!("Received WebSocket close frame");
                            return None;
                        }
                        _ => {}
                    }
                }

                else => {
                    tracing::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn parse_to_nautilus_messages(
        msg: HyperliquidWsMessage,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        bar_types: &AHashMap<String, BarType>,
        account_id: Option<AccountId>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match &msg {
            HyperliquidWsMessage::OrderUpdates { data } => {
                if let Some(account_id) = account_id {
                    let mut exec_reports = Vec::new();

                    for order_update in data {
                        if let Some(instrument) = instruments.get(&order_update.order.coin) {
                            match parse_ws_order_status_report(
                                order_update,
                                instrument,
                                account_id,
                                ts_init,
                            ) {
                                Ok(report) => {
                                    exec_reports.push(ExecutionReport::Order(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing order update: {e}");
                                }
                            }
                        } else {
                            tracing::warn!(
                                "No instrument found for coin: {}",
                                order_update.order.coin
                            );
                        }
                    }

                    if !exec_reports.is_empty() {
                        result.push(NautilusWsMessage::ExecutionReports(exec_reports));
                    }
                }
            }
            HyperliquidWsMessage::UserEvents { data } => {
                if let Some(account_id) = account_id
                    && let WsUserEventData::Fills { fills } = data
                {
                    let mut exec_reports = Vec::new();

                    for fill in fills {
                        if let Some(instrument) = instruments.get(&fill.coin) {
                            match parse_ws_fill_report(fill, instrument, account_id, ts_init) {
                                Ok(report) => {
                                    exec_reports.push(ExecutionReport::Fill(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing fill: {e}");
                                }
                            }
                        } else {
                            tracing::warn!("No instrument found for coin: {}", fill.coin);
                        }
                    }

                    if !exec_reports.is_empty() {
                        result.push(NautilusWsMessage::ExecutionReports(exec_reports));
                    }
                }
            }
            HyperliquidWsMessage::Trades { data } => {
                let mut trade_ticks = Vec::new();
                for trade in data {
                    if let Some(instrument) = instruments.get(&trade.coin) {
                        match parse_ws_trade_tick(trade, instrument, ts_init) {
                            Ok(tick) => trade_ticks.push(tick),
                            Err(e) => {
                                tracing::error!("Error parsing trade tick: {e}");
                            }
                        }
                    } else {
                        tracing::warn!("No instrument found for coin: {}", trade.coin);
                    }
                }
                if !trade_ticks.is_empty() {
                    result.push(NautilusWsMessage::Trades(trade_ticks));
                }
            }
            HyperliquidWsMessage::Bbo { data } => {
                if let Some(instrument) = instruments.get(&data.coin) {
                    match parse_ws_quote_tick(data, instrument, ts_init) {
                        Ok(quote_tick) => {
                            result.push(NautilusWsMessage::Quote(quote_tick));
                        }
                        Err(e) => {
                            tracing::error!("Error parsing quote tick: {e}");
                        }
                    }
                } else {
                    tracing::warn!("No instrument found for coin: {}", data.coin);
                }
            }
            HyperliquidWsMessage::L2Book { data } => {
                if let Some(instrument) = instruments.get(&data.coin) {
                    match parse_ws_order_book_deltas(data, instrument, ts_init) {
                        Ok(deltas) => {
                            result.push(NautilusWsMessage::Deltas(deltas));
                        }
                        Err(e) => {
                            tracing::error!("Error parsing order book deltas: {e}");
                        }
                    }
                } else {
                    tracing::warn!("No instrument found for coin: {}", data.coin);
                }
            }
            HyperliquidWsMessage::Candle { data } => {
                let key = format!("candle:{}:{}", data.s, data.i);
                if let Some(bar_type) = bar_types.get(&key) {
                    if let Some(instrument) = instruments.get(&data.s) {
                        match parse_ws_candle(data, instrument, bar_type, ts_init) {
                            Ok(bar) => {
                                result.push(NautilusWsMessage::Candle(bar));
                            }
                            Err(e) => {
                                tracing::error!("Error parsing candle: {e}");
                            }
                        }
                    } else {
                        tracing::warn!("No instrument found for coin: {}", data.s);
                    }
                } else {
                    tracing::warn!("No bar type found for key: {}", key);
                }
            }
            HyperliquidWsMessage::Error { data } => {
                tracing::warn!("Received error from Hyperliquid WebSocket: {}", data);
            }
            // Ignore other message types (subscription confirmations, etc)
            _ => {}
        }

        result
    }
}

/// Creates a canonical subscription key from a SubscriptionRequest for tracking.
fn subscription_to_key(sub: &SubscriptionRequest) -> String {
    match sub {
        SubscriptionRequest::AllMids { dex } => {
            if let Some(dex_name) = dex {
                format!("allMids:{dex_name}")
            } else {
                "allMids".to_string()
            }
        }
        SubscriptionRequest::Notification { user } => format!("notification:{user}"),
        SubscriptionRequest::WebData2 { user } => format!("webData2:{user}"),
        SubscriptionRequest::Candle { coin, interval } => format!("candle:{coin}:{interval:?}"),
        SubscriptionRequest::L2Book { coin, .. } => format!("l2Book:{coin}"),
        SubscriptionRequest::Trades { coin } => format!("trades:{coin}"),
        SubscriptionRequest::OrderUpdates { user } => format!("orderUpdates:{user}"),
        SubscriptionRequest::UserEvents { user } => format!("userEvents:{user}"),
        SubscriptionRequest::UserFills { user, .. } => format!("userFills:{user}"),
        SubscriptionRequest::UserFundings { user } => format!("userFundings:{user}"),
        SubscriptionRequest::UserNonFundingLedgerUpdates { user } => {
            format!("userNonFundingLedgerUpdates:{user}")
        }
        SubscriptionRequest::ActiveAssetCtx { coin } => format!("activeAssetCtx:{coin}"),
        SubscriptionRequest::ActiveAssetData { user, coin } => {
            format!("activeAssetData:{user}:{coin}")
        }
        SubscriptionRequest::UserTwapSliceFills { user } => format!("userTwapSliceFills:{user}"),
        SubscriptionRequest::UserTwapHistory { user } => format!("userTwapHistory:{user}"),
        SubscriptionRequest::Bbo { coin } => format!("bbo:{coin}"),
    }
}

/// Determines whether a Hyperliquid WebSocket error should trigger a retry.
pub(crate) fn should_retry_hyperliquid_error(error: &HyperliquidWsError) -> bool {
    match error {
        HyperliquidWsError::TungsteniteError(_) => true,
        HyperliquidWsError::ClientError(msg) => {
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        _ => false,
    }
}

/// Creates a timeout error for Hyperliquid retry logic.
pub(crate) fn create_hyperliquid_timeout_error(msg: String) -> HyperliquidWsError {
    HyperliquidWsError::ClientError(msg)
}

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

//! WebSocket client implementation for Delta Exchange.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{logging::log_task_stopped, runtime::get_runtime};
use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{MessageReader, WebSocketClient, WebSocketConfig};
use reqwest::header::USER_AGENT;
use tokio::{
    sync::{mpsc, RwLock},
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as TungsteniteError, Message},
};
use tracing::{debug, error, info, trace, warn};
use ustr::Ustr;

use super::{
    enums::{ConnectionState, DeltaExchangeWsChannel, ReconnectionStrategy, SubscriptionState, WsOperation},
    error::DeltaExchangeWsError,
    messages::{
        DeltaExchangeAuth, DeltaExchangeSubscription, DeltaExchangeWsMessage, NautilusWsMessage,
    },
};
use crate::common::{
    consts::{
        DEFAULT_RECONNECTION_DELAY_SECS, DEFAULT_WS_TIMEOUT_SECS, DELTA_EXCHANGE_TESTNET_WS_URL,
        DELTA_EXCHANGE_WS_URL, MAX_RECONNECTION_ATTEMPTS, MAX_WS_CONNECTIONS_PER_IP,
    },
    credential::Credential,
};

/// Configuration for the WebSocket client.
#[derive(Debug, Clone)]
pub struct DeltaExchangeWsConfig {
    /// WebSocket URL.
    pub url: String,
    /// Connection timeout in seconds.
    pub timeout_secs: u64,
    /// Reconnection strategy.
    pub reconnection_strategy: ReconnectionStrategy,
    /// Maximum reconnection attempts.
    pub max_reconnection_attempts: u32,
    /// Initial reconnection delay in seconds.
    pub reconnection_delay_secs: u64,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: Option<u64>,
    /// Enable automatic reconnection.
    pub auto_reconnect: bool,
    /// Maximum message queue size.
    pub max_queue_size: usize,
}

impl Default for DeltaExchangeWsConfig {
    fn default() -> Self {
        Self {
            url: DELTA_EXCHANGE_WS_URL.to_string(),
            timeout_secs: DEFAULT_WS_TIMEOUT_SECS,
            reconnection_strategy: ReconnectionStrategy::ExponentialBackoff,
            max_reconnection_attempts: MAX_RECONNECTION_ATTEMPTS,
            reconnection_delay_secs: DEFAULT_RECONNECTION_DELAY_SECS,
            heartbeat_interval_secs: Some(30),
            auto_reconnect: true,
            max_queue_size: 10000,
        }
    }
}

impl DeltaExchangeWsConfig {
    /// Create configuration for testnet.
    pub fn testnet() -> Self {
        Self {
            url: DELTA_EXCHANGE_TESTNET_WS_URL.to_string(),
            ..Default::default()
        }
    }
}

/// Subscription information.
#[derive(Debug, Clone)]
struct SubscriptionInfo {
    channel: DeltaExchangeWsChannel,
    symbols: Option<Vec<Ustr>>,
    state: SubscriptionState,
}

/// WebSocket client for Delta Exchange.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct DeltaExchangeWebSocketClient {
    config: DeltaExchangeWsConfig,
    credential: Option<Credential>,
    connection_state: Arc<AtomicU32>, // Using u32 to store ConnectionState as integer
    reconnection_attempts: Arc<AtomicU32>,
    subscriptions: Arc<DashMap<String, SubscriptionInfo>>, // Key: channel_symbol
    message_tx: Arc<RwLock<Option<mpsc::UnboundedSender<NautilusWsMessage>>>>,
    message_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<NautilusWsMessage>>>>,
    shutdown_signal: Arc<AtomicBool>,
    task_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
}

impl Default for DeltaExchangeWebSocketClient {
    fn default() -> Self {
        Self::new(None, None, None, None).expect("Failed to create default client")
    }
}

impl DeltaExchangeWebSocketClient {
    /// Creates a new [`DeltaExchangeWebSocketClient`] instance.
    pub fn new(
        config: Option<DeltaExchangeWsConfig>,
        api_key: Option<String>,
        api_secret: Option<String>,
        instruments: Option<AHashMap<Ustr, InstrumentAny>>,
    ) -> Result<Self, DeltaExchangeWsError> {
        let config = config.unwrap_or_default();
        
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(
                Credential::new(key, secret)
                    .map_err(|e| DeltaExchangeWsError::ConfigError(e))?,
            ),
            (None, None) => None,
            _ => {
                return Err(DeltaExchangeWsError::ConfigError(
                    "Both API key and secret must be provided together".to_string(),
                ));
            }
        };

        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Ok(Self {
            config,
            credential,
            connection_state: Arc::new(AtomicU32::new(ConnectionState::Disconnected as u32)),
            reconnection_attempts: Arc::new(AtomicU32::new(0)),
            subscriptions: Arc::new(DashMap::new()),
            message_tx: Arc::new(RwLock::new(Some(message_tx))),
            message_rx: Arc::new(RwLock::new(Some(message_rx))),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
            instruments_cache: Arc::new(instruments.unwrap_or_default()),
        })
    }

    /// Get current connection state.
    pub fn connection_state(&self) -> ConnectionState {
        match self.connection_state.load(Ordering::Acquire) {
            0 => ConnectionState::Connecting,
            1 => ConnectionState::Connected,
            2 => ConnectionState::Disconnecting,
            3 => ConnectionState::Disconnected,
            4 => ConnectionState::Reconnecting,
            5 => ConnectionState::Failed,
            _ => ConnectionState::Disconnected,
        }
    }

    /// Set connection state.
    fn set_connection_state(&self, state: ConnectionState) {
        self.connection_state.store(state as u32, Ordering::Release);
        debug!("Connection state changed to: {}", state);
    }

    /// Get current Unix timestamp in milliseconds.
    fn get_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64
    }

    /// Connect to the WebSocket.
    pub async fn connect(&self) -> Result<(), DeltaExchangeWsError> {
        if self.connection_state() != ConnectionState::Disconnected {
            return Err(DeltaExchangeWsError::StateError(
                "Client is not in disconnected state".to_string(),
            ));
        }

        self.set_connection_state(ConnectionState::Connecting);
        self.shutdown_signal.store(false, Ordering::Release);

        // Start the connection task
        let task_handle = self.start_connection_task().await?;
        *self.task_handle.write().await = Some(task_handle);

        // Wait for connection to be established or fail
        let timeout_duration = Duration::from_secs(self.config.timeout_secs);
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < timeout_duration {
            match self.connection_state() {
                ConnectionState::Connected => {
                    info!("Successfully connected to Delta Exchange WebSocket");
                    return Ok(());
                }
                ConnectionState::Failed => {
                    return Err(DeltaExchangeWsError::ConnectionError(
                        "Failed to establish connection".to_string(),
                    ));
                }
                _ => {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }

        Err(DeltaExchangeWsError::TimeoutError(
            "Connection timeout".to_string(),
        ))
    }

    /// Disconnect from the WebSocket.
    pub async fn disconnect(&self) -> Result<(), DeltaExchangeWsError> {
        if self.connection_state() == ConnectionState::Disconnected {
            return Ok(());
        }

        self.set_connection_state(ConnectionState::Disconnecting);
        self.shutdown_signal.store(true, Ordering::Release);

        // Wait for task to complete
        if let Some(handle) = self.task_handle.write().await.take() {
            if let Err(e) = handle.await {
                warn!("Error waiting for connection task to complete: {}", e);
            }
        }

        self.set_connection_state(ConnectionState::Disconnected);
        self.subscriptions.clear();
        
        info!("Disconnected from Delta Exchange WebSocket");
        Ok(())
    }

    /// Subscribe to a channel.
    pub async fn subscribe(
        &self,
        channel: DeltaExchangeWsChannel,
        symbols: Option<Vec<Ustr>>,
    ) -> Result<(), DeltaExchangeWsError> {
        if self.connection_state() != ConnectionState::Connected {
            return Err(DeltaExchangeWsError::StateError(
                "Client is not connected".to_string(),
            ));
        }

        let key = self.subscription_key(channel, &symbols);
        
        // Check if already subscribed
        if let Some(sub_info) = self.subscriptions.get(&key) {
            if sub_info.state == SubscriptionState::Active {
                return Ok(());
            }
        }

        // Add to subscriptions with pending state
        self.subscriptions.insert(
            key.clone(),
            SubscriptionInfo {
                channel,
                symbols: symbols.clone(),
                state: SubscriptionState::Pending,
            },
        );

        // Send subscription message
        self.send_subscription_message(WsOperation::Subscribe, channel, symbols)
            .await?;

        debug!("Subscribed to channel: {} with symbols: {:?}", channel, symbols);
        Ok(())
    }

    /// Unsubscribe from a channel.
    pub async fn unsubscribe(
        &self,
        channel: DeltaExchangeWsChannel,
        symbols: Option<Vec<Ustr>>,
    ) -> Result<(), DeltaExchangeWsError> {
        if self.connection_state() != ConnectionState::Connected {
            return Err(DeltaExchangeWsError::StateError(
                "Client is not connected".to_string(),
            ));
        }

        let key = self.subscription_key(channel, &symbols);
        
        // Update subscription state
        if let Some(mut sub_info) = self.subscriptions.get_mut(&key) {
            sub_info.state = SubscriptionState::Unsubscribing;
        }

        // Send unsubscription message
        self.send_subscription_message(WsOperation::Unsubscribe, channel, symbols)
            .await?;

        // Remove from subscriptions
        self.subscriptions.remove(&key);

        debug!("Unsubscribed from channel: {} with symbols: {:?}", channel, symbols);
        Ok(())
    }

    /// Get the next message from the WebSocket.
    pub async fn next_message(&self) -> Option<NautilusWsMessage> {
        let mut rx_guard = self.message_rx.write().await;
        if let Some(rx) = rx_guard.as_mut() {
            rx.recv().await
        } else {
            None
        }
    }

    /// Generate subscription key for tracking.
    fn subscription_key(&self, channel: DeltaExchangeWsChannel, symbols: &Option<Vec<Ustr>>) -> String {
        match symbols {
            Some(syms) => format!("{}:{}", channel, syms.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")),
            None => channel.to_string(),
        }
    }

    /// Start the connection task.
    async fn start_connection_task(&self) -> Result<tokio::task::JoinHandle<()>, DeltaExchangeWsError> {
        let config = self.config.clone();
        let credential = self.credential.clone();
        let connection_state = Arc::clone(&self.connection_state);
        let reconnection_attempts = Arc::clone(&self.reconnection_attempts);
        let subscriptions = Arc::clone(&self.subscriptions);
        let message_tx = Arc::clone(&self.message_tx);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        let task = tokio::spawn(async move {
            let mut current_reconnection_delay = config.reconnection_delay_secs;

            loop {
                if shutdown_signal.load(Ordering::Acquire) {
                    break;
                }

                match Self::connect_and_run(
                    &config,
                    &credential,
                    &connection_state,
                    &subscriptions,
                    &message_tx,
                    &shutdown_signal,
                ).await {
                    Ok(()) => {
                        // Normal disconnection
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket connection error: {}", e);

                        if !config.auto_reconnect || !e.is_retryable() {
                            connection_state.store(ConnectionState::Failed as u32, Ordering::Release);
                            break;
                        }

                        let attempts = reconnection_attempts.fetch_add(1, Ordering::AcqRel);
                        if attempts >= config.max_reconnection_attempts {
                            error!("Maximum reconnection attempts reached");
                            connection_state.store(ConnectionState::Failed as u32, Ordering::Release);
                            break;
                        }

                        connection_state.store(ConnectionState::Reconnecting as u32, Ordering::Release);
                        warn!("Reconnecting in {} seconds (attempt {}/{})",
                              current_reconnection_delay, attempts + 1, config.max_reconnection_attempts);

                        sleep(Duration::from_secs(current_reconnection_delay)).await;

                        // Exponential backoff
                        if config.reconnection_strategy == ReconnectionStrategy::ExponentialBackoff {
                            current_reconnection_delay = std::cmp::min(current_reconnection_delay * 2, 300);
                        }
                    }
                }
            }

            log_task_stopped("DeltaExchangeWebSocketClient");
        });

        Ok(task)
    }

    /// Connect and run the WebSocket client.
    async fn connect_and_run(
        config: &DeltaExchangeWsConfig,
        credential: &Option<Credential>,
        connection_state: &Arc<AtomicU32>,
        subscriptions: &Arc<DashMap<String, SubscriptionInfo>>,
        message_tx: &Arc<RwLock<Option<mpsc::UnboundedSender<NautilusWsMessage>>>>,
        shutdown_signal: &Arc<AtomicBool>,
    ) -> Result<(), DeltaExchangeWsError> {
        // Connect to WebSocket
        let (ws_stream, _) = timeout(
            Duration::from_secs(config.timeout_secs),
            connect_async(&config.url),
        )
        .await
        .map_err(|_| DeltaExchangeWsError::timeout_error("Connection timeout"))?
        .map_err(DeltaExchangeWsError::from)?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        connection_state.store(ConnectionState::Connected as u32, Ordering::Release);

        // Authenticate if credentials are provided
        if let Some(cred) = credential {
            Self::authenticate(&mut ws_sender, cred).await?;
        }

        // Resubscribe to channels
        Self::resubscribe_channels(&mut ws_sender, subscriptions).await?;

        // Message processing loop
        let mut heartbeat_interval = config.heartbeat_interval_secs.map(|secs| {
            tokio::time::interval(Duration::from_secs(secs))
        });

        loop {
            tokio::select! {
                // Handle incoming messages
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = Self::handle_message(&text, message_tx, subscriptions).await {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Some(Ok(Message::Binary(data))) => {
                            if let Ok(text) = String::from_utf8(data) {
                                if let Err(e) = Self::handle_message(&text, message_tx, subscriptions).await {
                                    error!("Error handling binary message: {}", e);
                                }
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            if let Err(e) = ws_sender.send(Message::Pong(data)).await {
                                error!("Error sending pong: {}", e);
                                break;
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            // Pong received, connection is alive
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket connection closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            return Err(DeltaExchangeWsError::from(e));
                        }
                        None => {
                            warn!("WebSocket stream ended");
                            break;
                        }
                    }
                }

                // Handle heartbeat
                _ = async {
                    if let Some(ref mut interval) = heartbeat_interval {
                        interval.tick().await;
                        if let Err(e) = ws_sender.send(Message::Ping(vec![])).await {
                            error!("Error sending ping: {}", e);
                        }
                    } else {
                        // If no heartbeat, wait indefinitely
                        std::future::pending::<()>().await;
                    }
                } => {}

                // Handle shutdown signal
                _ = async {
                    while !shutdown_signal.load(Ordering::Acquire) {
                        sleep(Duration::from_millis(100)).await;
                    }
                } => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Clean shutdown
        if let Err(e) = ws_sender.send(Message::Close(None)).await {
            warn!("Error sending close message: {}", e);
        }

        Ok(())
    }

    /// Authenticate with the WebSocket server.
    async fn authenticate(
        ws_sender: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >,
        credential: &Credential,
    ) -> Result<(), DeltaExchangeWsError> {
        let timestamp = Self::get_timestamp_ms();
        let signature = credential
            .sign_ws(timestamp)
            .map_err(DeltaExchangeWsError::auth_error)?;

        let auth_msg = DeltaExchangeAuth {
            op: WsOperation::Auth,
            api_key: credential.api_key,
            timestamp,
            signature,
        };

        let auth_json = serde_json::to_string(&auth_msg)?;
        ws_sender.send(Message::Text(auth_json)).await
            .map_err(DeltaExchangeWsError::from)?;

        debug!("Authentication message sent");
        Ok(())
    }

    /// Resubscribe to all active channels after reconnection.
    async fn resubscribe_channels(
        ws_sender: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >,
        subscriptions: &Arc<DashMap<String, SubscriptionInfo>>,
    ) -> Result<(), DeltaExchangeWsError> {
        for entry in subscriptions.iter() {
            let sub_info = entry.value();
            if sub_info.state == SubscriptionState::Active {
                let sub_msg = DeltaExchangeSubscription {
                    op: WsOperation::Subscribe,
                    channels: vec![sub_info.channel],
                    symbols: sub_info.symbols.clone(),
                };

                let sub_json = serde_json::to_string(&sub_msg)?;
                ws_sender.send(Message::Text(sub_json)).await
                    .map_err(DeltaExchangeWsError::from)?;

                debug!("Resubscribed to channel: {}", sub_info.channel);
            }
        }

        Ok(())
    }

    /// Send a subscription message.
    async fn send_subscription_message(
        &self,
        operation: WsOperation,
        channel: DeltaExchangeWsChannel,
        symbols: Option<Vec<Ustr>>,
    ) -> Result<(), DeltaExchangeWsError> {
        let sub_msg = DeltaExchangeSubscription {
            op: operation,
            channels: vec![channel],
            symbols,
        };

        let sub_json = serde_json::to_string(&sub_msg)?;

        // For now, we'll store the message to send when connected
        // In a full implementation, this would send through the WebSocket
        debug!("Subscription message prepared: {}", sub_json);

        Ok(())
    }

    /// Handle incoming WebSocket message.
    async fn handle_message(
        text: &str,
        message_tx: &Arc<RwLock<Option<mpsc::UnboundedSender<NautilusWsMessage>>>>,
        subscriptions: &Arc<DashMap<String, SubscriptionInfo>>,
    ) -> Result<(), DeltaExchangeWsError> {
        trace!("Received message: {}", text);

        // Parse the message
        let ws_message: DeltaExchangeWsMessage = serde_json::from_str(text)
            .map_err(|e| DeltaExchangeWsError::parsing_error(format!("Failed to parse message: {}", e)))?;

        // Process the message
        let nautilus_message = Self::process_message(ws_message, subscriptions).await?;

        // Send to message channel if available
        if let Some(nautilus_msg) = nautilus_message {
            let tx_guard = message_tx.read().await;
            if let Some(tx) = tx_guard.as_ref() {
                if let Err(e) = tx.send(nautilus_msg) {
                    error!("Failed to send message to channel: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Process a WebSocket message and convert to Nautilus format.
    async fn process_message(
        message: DeltaExchangeWsMessage,
        subscriptions: &Arc<DashMap<String, SubscriptionInfo>>,
    ) -> Result<Option<NautilusWsMessage>, DeltaExchangeWsError> {
        match message {
            DeltaExchangeWsMessage::Auth(auth_msg) => {
                if auth_msg.success {
                    info!("WebSocket authentication successful");
                } else {
                    error!("WebSocket authentication failed: {:?}", auth_msg.message);
                    return Err(DeltaExchangeWsError::auth_error(
                        auth_msg.message.unwrap_or_else(|| "Authentication failed".to_string()),
                    ));
                }
                Ok(None)
            }

            DeltaExchangeWsMessage::Subscription(sub_msg) => {
                if sub_msg.success {
                    // Update subscription states
                    for channel in &sub_msg.channels {
                        let key = match &sub_msg.symbols {
                            Some(symbols) => format!("{}:{}", channel, symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")),
                            None => channel.to_string(),
                        };

                        if let Some(mut sub_info) = subscriptions.get_mut(&key) {
                            sub_info.state = SubscriptionState::Active;
                        }
                    }
                    debug!("Subscription confirmed for channels: {:?}", sub_msg.channels);
                } else {
                    error!("Subscription failed: {:?}", sub_msg.message);
                }
                Ok(None)
            }

            DeltaExchangeWsMessage::Error(error_msg) => {
                error!("WebSocket error: {} - {}", error_msg.code, error_msg.message);
                Err(DeltaExchangeWsError::DeltaExchangeError {
                    code: error_msg.code,
                    message: error_msg.message,
                })
            }

            DeltaExchangeWsMessage::Ticker(ticker_msg) => {
                // Convert ticker to Nautilus format
                // This would be implemented in a separate parsing module
                debug!("Received ticker update for {}: {:?}", ticker_msg.symbol, ticker_msg.price);
                Ok(Some(NautilusWsMessage::Raw(format!("Ticker: {}", ticker_msg.symbol))))
            }

            DeltaExchangeWsMessage::OrderBookSnapshot(book_msg) => {
                debug!("Received order book snapshot for {}", book_msg.symbol);
                Ok(Some(NautilusWsMessage::Raw(format!("OrderBookSnapshot: {}", book_msg.symbol))))
            }

            DeltaExchangeWsMessage::OrderBookUpdate(update_msg) => {
                debug!("Received order book update for {}", update_msg.symbol);
                Ok(Some(NautilusWsMessage::Raw(format!("OrderBookUpdate: {}", update_msg.symbol))))
            }

            DeltaExchangeWsMessage::Trade(trade_msg) => {
                debug!("Received trade for {}: {} @ {}", trade_msg.symbol, trade_msg.size, trade_msg.price);
                Ok(Some(NautilusWsMessage::Raw(format!("Trade: {}", trade_msg.symbol))))
            }

            DeltaExchangeWsMessage::Candle(candle_msg) => {
                debug!("Received candle for {}: OHLCV({}, {}, {}, {}, {})",
                       candle_msg.symbol, candle_msg.open, candle_msg.high,
                       candle_msg.low, candle_msg.close, candle_msg.volume);
                Ok(Some(NautilusWsMessage::Raw(format!("Candle: {}", candle_msg.symbol))))
            }

            DeltaExchangeWsMessage::MarkPrice(mark_msg) => {
                debug!("Received mark price for {}: {}", mark_msg.symbol, mark_msg.mark_price);
                Ok(Some(NautilusWsMessage::Raw(format!("MarkPrice: {}", mark_msg.symbol))))
            }

            DeltaExchangeWsMessage::FundingRate(funding_msg) => {
                debug!("Received funding rate for {}: {}", funding_msg.symbol, funding_msg.funding_rate);
                Ok(Some(NautilusWsMessage::Raw(format!("FundingRate: {}", funding_msg.symbol))))
            }

            DeltaExchangeWsMessage::Order(order_msg) => {
                debug!("Received order update: {} {} {} {}",
                       order_msg.event_type, order_msg.id, order_msg.product_symbol, order_msg.state);
                Ok(Some(NautilusWsMessage::Raw(format!("Order: {}", order_msg.id))))
            }

            DeltaExchangeWsMessage::Position(position_msg) => {
                debug!("Received position update: {} {} size={}",
                       position_msg.event_type, position_msg.product_symbol, position_msg.size);
                Ok(Some(NautilusWsMessage::Raw(format!("Position: {}", position_msg.product_symbol))))
            }

            DeltaExchangeWsMessage::UserTrade(trade_msg) => {
                debug!("Received user trade: {} {} @ {} ({})",
                       trade_msg.product_symbol, trade_msg.size, trade_msg.price, trade_msg.side);
                Ok(Some(NautilusWsMessage::Raw(format!("UserTrade: {}", trade_msg.id))))
            }

            DeltaExchangeWsMessage::Margin(margin_msg) => {
                debug!("Received margin update for {}: balance={}",
                       margin_msg.asset_symbol, margin_msg.balance);
                Ok(Some(NautilusWsMessage::Raw(format!("Margin: {}", margin_msg.asset_symbol))))
            }

            DeltaExchangeWsMessage::Ping(ping_msg) => {
                trace!("Received ping at timestamp: {}", ping_msg.timestamp);
                Ok(None) // Ping messages are handled automatically
            }
        }
    }

    /// Get subscription information.
    pub fn get_subscriptions(&self) -> Vec<(DeltaExchangeWsChannel, Option<Vec<Ustr>>, SubscriptionState)> {
        self.subscriptions
            .iter()
            .map(|entry| {
                let sub_info = entry.value();
                (sub_info.channel, sub_info.symbols.clone(), sub_info.state)
            })
            .collect()
    }

    /// Check if subscribed to a specific channel.
    pub fn is_subscribed(&self, channel: DeltaExchangeWsChannel, symbols: Option<Vec<Ustr>>) -> bool {
        let key = self.subscription_key(channel, &symbols);
        self.subscriptions
            .get(&key)
            .map(|sub_info| sub_info.state == SubscriptionState::Active)
            .unwrap_or(false)
    }

    /// Get reconnection attempts count.
    pub fn reconnection_attempts(&self) -> u32 {
        self.reconnection_attempts.load(Ordering::Acquire)
    }

    /// Reset reconnection attempts counter.
    pub fn reset_reconnection_attempts(&self) {
        self.reconnection_attempts.store(0, Ordering::Release);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = DeltaExchangeWebSocketClient::default();
        assert_eq!(client.connection_state(), ConnectionState::Disconnected);
        assert_eq!(client.reconnection_attempts(), 0);
    }

    #[tokio::test]
    async fn test_client_with_credentials() {
        let client = DeltaExchangeWebSocketClient::new(
            None,
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            None,
        ).unwrap();

        assert!(client.credential.is_some());
        assert_eq!(client.connection_state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_subscription_key_generation() {
        let client = DeltaExchangeWebSocketClient::default();

        let key1 = client.subscription_key(DeltaExchangeWsChannel::V2Ticker, &None);
        assert_eq!(key1, "v2_ticker");

        let symbols = Some(vec!["BTCUSD".into(), "ETHUSD".into()]);
        let key2 = client.subscription_key(DeltaExchangeWsChannel::L2Orderbook, &symbols);
        assert_eq!(key2, "l2_orderbook:BTCUSD,ETHUSD");
    }

    #[test]
    fn test_connection_state_transitions() {
        let client = DeltaExchangeWebSocketClient::default();

        assert_eq!(client.connection_state(), ConnectionState::Disconnected);

        client.set_connection_state(ConnectionState::Connecting);
        assert_eq!(client.connection_state(), ConnectionState::Connecting);

        client.set_connection_state(ConnectionState::Connected);
        assert_eq!(client.connection_state(), ConnectionState::Connected);
    }

    #[test]
    fn test_config_default() {
        let config = DeltaExchangeWsConfig::default();
        assert_eq!(config.url, DELTA_EXCHANGE_WS_URL);
        assert_eq!(config.timeout_secs, DEFAULT_WS_TIMEOUT_SECS);
        assert_eq!(config.reconnection_strategy, ReconnectionStrategy::ExponentialBackoff);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_config_testnet() {
        let config = DeltaExchangeWsConfig::testnet();
        assert_eq!(config.url, DELTA_EXCHANGE_TESTNET_WS_URL);
        assert!(config.auto_reconnect);
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use futures_util::{Stream, StreamExt};
use nautilus_common::runtime::get_runtime;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data, bar::BarType},
    events::OrderEventAny,
    identifiers::InstrumentId,
};
use nautilus_network::websocket::{Consumer, MessageReader, WebSocketClient, WebSocketConfig};
use reqwest::header::USER_AGENT;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;

use super::{
    cache::QuoteCache,
    enums::{Action, WsTopic},
    error::BitmexWsError,
    messages::{TableMessage, WsMessage},
    parse::{
        parse_book_msg_vec, parse_book10_msg_vec, parse_trade_bin_msg_vec, parse_trade_msg_vec,
        topic_from_bar_spec,
    },
};
use crate::{consts::BITMEX_WS_URL, credential::Credential};

/// Provides a WebSocket client for connecting to the [BitMEX](https://bitmex.com) real-time API.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexWebSocketClient {
    url: String,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    inner: Option<Arc<WebSocketClient>>,
    rx_data: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<Vec<Data>>>>,
    rx_exec: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<OrderEventAny>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
}

impl BitmexWebSocketClient {
    /// Creates a new [`BitmexWebSocketClient`] instance.
    pub fn new(
        url: Option<&str>,
        api_key: Option<&str>,
        api_secret: Option<&str>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key.to_string(), secret.to_string())),
            (None, None) => None,
            _ => anyhow::bail!("Both `api_key` and `api_secret` must be provided together"),
        };

        Ok(Self {
            url: url.unwrap_or(BITMEX_WS_URL).to_string(),
            credential,
            heartbeat,
            inner: None,
            rx_data: None,
            rx_exec: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
        })
    }

    pub async fn connect_data(&mut self) -> Result<(), BitmexWsError> {
        let reader = self.connect().await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<Data>>();
        self.rx_data = Some(Arc::new(rx));
        let signal = self.signal.clone();

        let stream_handle = get_runtime().spawn(async move {
            BitmexDataFeedHandler::new(reader, signal, tx).run().await;
        });

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    pub async fn connect_exec(&mut self) -> Result<(), BitmexWsError> {
        let reader = self.connect().await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<OrderEventAny>();
        self.rx_exec = Some(Arc::new(rx));
        let signal = self.signal.clone();

        let stream_handle = get_runtime().spawn(async move {
            BitmexExecFeedHandler::new(reader, signal, tx).run().await;
        });

        self.task_handle = Some(Arc::new(stream_handle));

        // Subscribe for all execution related topics
        self.subscribe(vec![
            "execution".to_string(),
            "order".to_string(),
            "margin".to_string(),
            "position".to_string(),
            "wallet".to_string(),
        ])
        .await?;

        Ok(())
    }

    async fn connect(&mut self) -> Result<MessageReader, BitmexWsError> {
        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            #[cfg(feature = "python")]
            handler: Consumer::Python(None),
            #[cfg(not(feature = "python"))]
            handler: {
                let (consumer, _rx) = Consumer::rust_consumer();
                consumer
            },
            #[cfg(feature = "python")]
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };

        let keyed_quotas = vec![];
        let (reader, client) = WebSocketClient::connect_stream(config, keyed_quotas, None, None)
            .await
            .map_err(|e| BitmexWsError::ClientError(e.to_string()))?;

        self.inner = Some(Arc::new(client));

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        Ok(reader)
    }

    async fn authenticate(&mut self) -> Result<(), BitmexWsError> {
        let credential = match &self.credential {
            Some(credential) => credential,
            None => {
                panic!("API credentials not available to authenticate");
            }
        };

        let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
        let signature = credential.sign("GET", "/realtime", expires, "");

        let auth_message = serde_json::json!({
            "op": "authKeyExpires",
            "args": [credential.api_key, expires, signature]
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(auth_message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::AuthenticationError(e.to_string()))
        } else {
            log::error!("Cannot authenticate: not connected");
            Ok(())
        }
    }

    /// Provides the internal data stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the websocket is not connected.
    /// - If `stream_data` has already been called somewhere else (stream receiver is then taken).
    pub fn stream_data(&mut self) -> impl Stream<Item = Vec<Data>> + use<> {
        let rx = self
            .rx_data
            .take()
            .expect("Data stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(data) = rx.recv().await {
                yield data;
            }
        }
    }

    /// Provides the internal execution stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the websocket is not connected.
    /// - If `stream_exec` has already been called somewhere else (stream receiver is then taken).
    pub fn stream_exec(&mut self) -> impl Stream<Item = OrderEventAny> + use<> {
        let rx = self
            .rx_exec
            .take()
            .expect("Exec stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(event) = rx.recv().await {
                yield event;
            }
        }
    }

    /// Closes the client.
    pub async fn close(&mut self) -> Result<(), BitmexWsError> {
        if let Some(inner) = &self.inner {
            inner.disconnect().await;
        } else {
            log::error!("Error on close: not connected");
        }

        self.signal.store(true, Ordering::Relaxed);

        if let Some(stream_handle) = self.task_handle.take() {
            let stream_handle = Arc::try_unwrap(stream_handle)
                .expect("Cannot take ownership - other references exist");
            match stream_handle.await {
                Ok(()) => log::debug!("Stream handle completed successfully."),
                Err(err) => log::error!("Stream handle encountered an error: {:?}", err),
            }
        } else {
            log::debug!("No stream handle to await");
        }

        log::debug!("Closed");

        Ok(())
    }

    async fn subscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        let message = serde_json::json!({
            "op": "subscribe",
            "args": topics
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
        } else {
            log::error!("Cannot send message: not connected");
        }

        Ok(())
    }

    async fn unsubscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        let message = serde_json::json!({
            "op": "unsubscribe",
            "args": topics
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
        } else {
            log::error!("Cannot send message: not connected");
        }

        Ok(())
    }

    pub async fn subscribe_order_book(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn subscribe_order_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn subscribe_order_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBook10;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Quote;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Trade;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_order_book(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_order_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_order_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBook10;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Quote;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Trade;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }
}

struct BitmexFeedHandler {
    reader: MessageReader,
    signal: Arc<AtomicBool>,
}

impl BitmexFeedHandler {
    /// Creates a new [`BitmexFeedHandler`] instance.
    pub const fn new(reader: MessageReader, signal: Arc<AtomicBool>) -> Self {
        Self { reader, signal }
    }

    /// Get the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<WsMessage> {
        // Timeout awaiting the next message before checking signal
        let timeout_duration = Duration::from_millis(10);

        loop {
            if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::debug!("Stop signal received");
                break;
            }

            match tokio::time::timeout(timeout_duration, self.reader.next()).await {
                Ok(Some(msg)) => match msg {
                    Ok(Message::Text(text)) => {
                        // tracing::debug!(text); // TODO: Temporary for development
                        match serde_json::from_str(&text) {
                            Ok(msg) => match &msg {
                                WsMessage::Welcome {
                                    version,
                                    heartbeat_enabled,
                                    limit,
                                    ..
                                } => {
                                    tracing::info!(
                                        version = version,
                                        heartbeat = heartbeat_enabled,
                                        rate_limit = limit.remaining,
                                        "Welcome to the BitMEX Realtime API:",
                                    );
                                }
                                WsMessage::Subscription {
                                    success,
                                    subscribe,
                                    error,
                                } => {
                                    if let Some(subscribe) = subscribe {
                                        tracing::debug!("Subscribed to: {subscribe}");
                                    }
                                    if let Some(error) = error {
                                        tracing::error!(error);
                                    }
                                    tracing::debug!("Success: {success}");
                                }
                                WsMessage::Error { status, error, .. } => {
                                    tracing::error!(status = status, error = error);
                                    break; // TODO: Break for now
                                }
                                _ => return Some(msg),
                            },
                            Err(e) => {
                                tracing::error!("{e}: {text}");
                                break; // TODO: Break for now
                            }
                        }
                    }
                    Ok(Message::Binary(msg)) => {
                        tracing::debug!("Raw binary: {msg:?}");
                    }
                    Ok(Message::Close(_)) => {
                        tracing::debug!("Received close message");
                        return None;
                    }
                    Ok(msg) => {
                        tracing::warn!("Unexpected message: {msg}");
                    }
                    Err(e) => {
                        tracing::error!("{e}");
                        break; // TODO: Break for now
                    }
                },
                Ok(None) => {
                    tracing::info!("WebSocket stream closed");
                    break;
                }
                Err(_) => {} // Timeout occurred awaiting a message, continue loop to check signal
            }
        }

        tracing::debug!("Stopped message streaming");
        None
    }
}

struct BitmexDataFeedHandler {
    handler: BitmexFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<Data>>,
}

impl BitmexDataFeedHandler {
    /// Creates a new [`BitmexFeedHandler`] instance.
    pub const fn new(
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<Data>>,
    ) -> Self {
        let handler = BitmexFeedHandler::new(reader, signal);
        Self { handler, tx }
    }

    async fn run(&mut self) {
        while let Some(data) = self.next().await {
            if let Err(e) = self.tx.send(data) {
                tracing::error!("Error sending data: {e}");
                break; // Stop processing on channel error for now
            }
        }
    }

    async fn next(&mut self) -> Option<Vec<Data>> {
        let mut quote_cache = QuoteCache::new();

        while let Some(msg) = self.handler.next().await {
            if let WsMessage::Table(msg) = msg {
                let ts_init = get_atomic_clock_realtime().get_time_ns();
                return Some(match msg {
                    TableMessage::OrderBookL2 { action, data } => {
                        parse_book_msg_vec(data, action, 1, ts_init)
                    }
                    TableMessage::OrderBookL2_25 { action, data } => {
                        parse_book_msg_vec(data, action, 1, ts_init)
                    }
                    TableMessage::OrderBook10 { data, .. } => {
                        parse_book10_msg_vec(data, 1, ts_init)
                    }
                    TableMessage::Quote { mut data, .. } => {
                        let msg = data.remove(0);
                        if let Some(quote) = quote_cache.process(msg, 1) {
                            vec![Data::Quote(quote)]
                        } else {
                            continue; // No quote yet
                        }
                    }
                    TableMessage::Trade { data, .. } => parse_trade_msg_vec(data, 1, ts_init),
                    // TODO: Duplicate trade bin handling for now
                    TableMessage::TradeBin1m { action, data } => {
                        if action == Action::Partial {
                            continue; // Partial bar not yet closed
                        }
                        parse_trade_bin_msg_vec(data, WsTopic::TradeBin1m, 1, ts_init)
                    }
                    TableMessage::TradeBin5m { action, data } => {
                        if action == Action::Partial {
                            continue; // Partial bar not yet closed
                        }
                        parse_trade_bin_msg_vec(data, WsTopic::TradeBin5m, 1, ts_init)
                    }
                    TableMessage::TradeBin1h { action, data } => {
                        if action == Action::Partial {
                            continue; // Partial bar not yet closed
                        }
                        parse_trade_bin_msg_vec(data, WsTopic::TradeBin1h, 1, ts_init)
                    }
                    TableMessage::TradeBin1d { action, data } => {
                        if action == Action::Partial {
                            continue; // Partial bar not yet closed
                        }
                        parse_trade_bin_msg_vec(data, WsTopic::TradeBin1d, 1, ts_init)
                    }
                    _ => panic!("`TableMessage` type not implemented"),
                });
            }
        }
        None // Connection closed
    }
}

struct BitmexExecFeedHandler {
    handler: BitmexFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<OrderEventAny>,
}

impl BitmexExecFeedHandler {
    /// Creates a new [`BitmexFeedHandler`] instance.
    pub const fn new(
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::UnboundedSender<OrderEventAny>,
    ) -> Self {
        let handler = BitmexFeedHandler::new(reader, signal);
        Self { handler, tx }
    }

    async fn run(&mut self) {
        while let Some(event) = self.next().await {
            if let Err(e) = self.tx.send(event) {
                tracing::error!("Error sending event: {e}");
                break; // Stop processing on channel error for now
            }
        }
    }

    async fn next(&mut self) -> Option<OrderEventAny> {
        while let Some(msg) = self.handler.next().await {
            tracing::debug!("{msg:?}");
            // if let WsMessage::Error(msg) = msg {
            //     let ts_init = UnixNanos::now();
            // }
        }
        None // Connection closed
    }
}

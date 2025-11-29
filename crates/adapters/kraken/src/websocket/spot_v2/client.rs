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

//! WebSocket client for the Kraken v2 streaming API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::runtime::get_runtime;
use nautilus_model::{data::BarType, identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    enums::{KrakenWsChannel, KrakenWsMethod},
    handler::{SpotFeedHandler, SpotHandlerCommand},
    messages::{KrakenWsParams, KrakenWsRequest, NautilusWsMessage},
};
use crate::{
    config::KrakenDataClientConfig, http::KrakenSpotHttpClient, websocket::error::KrakenWsError,
};

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct KrakenSpotWebSocketClient {
    url: String,
    config: KrakenDataClientConfig,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<DashMap<String, KrakenWsChannel>>,
    cancellation_token: CancellationToken,
    req_id_counter: Arc<tokio::sync::RwLock<u64>>,
    auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl Clone for KrakenSpotWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            config: self.config.clone(),
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: self.out_rx.clone(),
            task_handle: self.task_handle.clone(),
            subscriptions: self.subscriptions.clone(),
            cancellation_token: self.cancellation_token.clone(),
            req_id_counter: self.req_id_counter.clone(),
            auth_token: self.auth_token.clone(),
        }
    }
}

impl KrakenSpotWebSocketClient {
    pub fn new(config: KrakenDataClientConfig, cancellation_token: CancellationToken) -> Self {
        let url = config.ws_public_url();
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            config,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: Arc::new(DashMap::new()),
            cancellation_token,
            req_id_counter: Arc::new(tokio::sync::RwLock::new(0)),
            auth_token: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn get_next_req_id(&self) -> u64 {
        let mut counter = self.req_id_counter.write().await;
        *counter += 1;
        *counter
    }

    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Connecting to {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            message_handler: Some(raw_handler),
            ping_handler: None,
            heartbeat: self.config.heartbeat_interval_secs,
            heartbeat_msg: Some("ping".to_string()),
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };

        let ws_client = WebSocketClient::connect(
            ws_config,
            None,   // post_reconnection
            vec![], // keyed_quotas
            None,   // default_quota
        )
        .await
        .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        // Share connection state across clones via ArcSwap
        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(SpotHandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        let signal = self.signal.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = SpotFeedHandler::new(signal.clone(), cmd_rx, raw_rx);

            loop {
                match handler.next().await {
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            tracing::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            tracing::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        tracing::debug!("WebSocket connected successfully");
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Disconnecting WebSocket");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self
            .cmd_tx
            .read()
            .await
            .send(SpotHandlerCommand::Disconnect)
        {
            tracing::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    tracing::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => tracing::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => tracing::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            tracing::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                        }
                    }
                }
                Err(arc_handle) => {
                    tracing::debug!(
                        "Cannot take ownership of task handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            tracing::debug!("No task handle to await");
        }

        self.subscriptions.clear();

        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), KrakenWsError> {
        self.disconnect().await
    }

    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), KrakenWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            KrakenWsError::ConnectionError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    pub async fn authenticate(&self) -> Result<(), KrakenWsError> {
        if !self.config.has_api_credentials() {
            return Err(KrakenWsError::AuthenticationError(
                "API credentials required for authentication".to_string(),
            ));
        }

        let api_key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API key".to_string()))?;
        let api_secret =
            self.config.api_secret.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError("Missing API secret".to_string())
            })?;

        let http_client = KrakenSpotHttpClient::with_credentials(
            api_key,
            api_secret,
            self.config.environment,
            Some(self.config.http_base_url()),
            self.config.timeout_secs,
            None,
            None,
            None,
            self.config.http_proxy.clone(),
        )
        .map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to create HTTP client: {e}"))
        })?;

        let ws_token = http_client.get_websockets_token().await.map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to get WebSocket token: {e}"))
        })?;

        tracing::debug!(
            token_length = ws_token.token.len(),
            expires = ws_token.expires,
            "WebSocket authentication token received"
        );

        let mut auth_token = self.auth_token.write().await;
        *auth_token = Some(ws_token.token);

        Ok(())
    }

    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        // Before connect() the handler isn't running; this send will fail and that's expected
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::InitializeInstruments(instruments))
        {
            tracing::debug!("Failed to send instruments to handler: {e}");
        }
    }

    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        // Before connect() the handler isn't running; this send will fail and that's expected
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::UpdateInstrument(instrument))
        {
            tracing::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    pub async fn subscribe(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id().await;

        // Check if channel requires authentication
        let is_private = matches!(
            channel,
            KrakenWsChannel::Executions | KrakenWsChannel::Balances
        );
        let token = if is_private {
            Some(self.auth_token.read().await.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError(
                    "Authentication token required for private channels. Call authenticate() first"
                        .to_string(),
                )
            })?)
        } else {
            None
        };

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols.clone()),
                snapshot: None,
                depth,
                token,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in symbols {
            let key = format!("{:?}:{}", channel, symbol);
            self.subscriptions.insert(key, channel);
        }

        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
    ) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id().await;

        // Check if channel requires authentication
        let is_private = matches!(
            channel,
            KrakenWsChannel::Executions | KrakenWsChannel::Balances
        );
        let token = if is_private {
            Some(self.auth_token.read().await.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError(
                    "Authentication token required for private channels. Call authenticate() first"
                        .to_string(),
                )
            })?)
        } else {
            None
        };

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols.clone()),
                snapshot: None,
                depth: None,
                token,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in symbols {
            let key = format!("{:?}:{}", channel, symbol);
            self.subscriptions.remove(&key);
        }

        Ok(())
    }

    pub async fn send_ping(&self) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id().await;

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Ping,
            params: None,
            req_id: Some(req_id),
        };

        self.send_request(&request).await
    }

    async fn send_request(&self, request: &KrakenWsRequest) -> Result<(), KrakenWsError> {
        let payload =
            serde_json::to_string(request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

        tracing::trace!("Sending message: {payload}");

        self.cmd_tx
            .read()
            .await
            .send(SpotHandlerCommand::SendText { payload })
            .map_err(|e| KrakenWsError::ConnectionError(format!("Failed to send request: {e}")))?;

        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        !ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
    }

    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Relaxed)
    }

    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Relaxed)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn get_subscriptions(&self) -> Vec<String> {
        self.subscriptions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn stream(&mut self) -> impl futures_util::Stream<Item = NautilusWsMessage> + use<> {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or client not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        // Kraken v2 WebSocket expects ISO 4217-A3 format (e.g., "ETH/USD")
        let symbol = instrument_id.symbol.inner();
        self.subscribe(KrakenWsChannel::Book, vec![symbol], depth)
            .await
    }

    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.subscribe(KrakenWsChannel::Ticker, vec![symbol], None)
            .await
    }

    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.subscribe(KrakenWsChannel::Trade, vec![symbol], None)
            .await
    }

    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = bar_type.instrument_id().symbol.inner();
        self.subscribe(KrakenWsChannel::Ohlc, vec![symbol], None)
            .await
    }

    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(KrakenWsChannel::Book, vec![symbol]).await
    }

    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(KrakenWsChannel::Ticker, vec![symbol])
            .await
    }

    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(KrakenWsChannel::Trade, vec![symbol]).await
    }

    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = bar_type.instrument_id().symbol.inner();
        self.unsubscribe(KrakenWsChannel::Ohlc, vec![symbol]).await
    }
}

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

//! WebSocket client for dYdX v4 API.
//!
//! This client provides streaming connectivity to dYdX's WebSocket API for both
//! public market data and private account updates.
//!
//! # Authentication
//!
//! dYdX v4 uses Cosmos SDK wallet-based authentication. Unlike traditional exchanges:
//! - **Public channels** require no authentication.
//! - **Private channels** (subaccounts) only require the wallet address in the subscription message.
//! - No signature or API key is needed for WebSocket connections themselves.
//!
//! # References
//!
//! <https://docs.dydx.trade/developers/indexer/websockets>

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dashmap::DashMap;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::error::{DydxWsError, DydxWsResult};
use crate::common::credential::DydxCredential;

/// WebSocket client for dYdX v4 market data and account streams.
///
/// # Authentication
///
/// dYdX v4 does not require traditional API key signatures for WebSocket connections.
/// Public channels work without any credentials. Private channels (subaccounts) only
/// need the wallet address included in the subscription message.
///
/// The [`DydxCredential`] stored in this client is used for:
/// - Providing the wallet address for private channel subscriptions
/// - Transaction signing (when placing orders via the validator node)
///
/// It is **NOT** used for WebSocket message signing or authentication.
#[derive(Debug)]
#[allow(dead_code)] // TODO: Remove once implementation is complete
pub struct DydxWebSocketClient {
    /// The WebSocket connection URL.
    url: String,
    /// Optional credential for private channels (only wallet address is used).
    credential: Option<DydxCredential>,
    /// Whether authentication is required for this client.
    requires_auth: bool,
    /// Whether the client is currently connected.
    is_connected: Arc<AtomicBool>,
    /// Cached instruments for parsing market data.
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    /// Optional account ID for account message parsing.
    account_id: Option<AccountId>,
    /// Optional heartbeat interval in seconds.
    heartbeat: Option<u64>,
    /// Network-level WebSocket client (to be refactored: handler should own this inside lock-free I/O boundary).
    inner: Arc<RwLock<Option<WebSocketClient>>>,
    /// Inbound decoded dYdX websocket messages receiver.
    rx_inbound: Option<tokio::sync::mpsc::UnboundedReceiver<super::messages::DydxWsMessage>>,
    /// Background reader task handle.
    reader_task: Option<tokio::task::JoinHandle<()>>,
}

impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    #[must_use]
    pub fn new_public(url: String, _heartbeat: Option<u64>) -> Self {
        Self {
            url,
            credential: None,
            requires_auth: false,
            is_connected: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            heartbeat: _heartbeat,
            inner: Arc::new(RwLock::new(None)),
            rx_inbound: None,
            reader_task: None,
        }
    }

    /// Creates a new private WebSocket client for account updates.
    #[must_use]
    pub fn new_private(
        url: String,
        credential: DydxCredential,
        account_id: AccountId,
        _heartbeat: Option<u64>,
    ) -> Self {
        Self {
            url,
            credential: Some(credential),
            requires_auth: true,
            is_connected: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: Some(account_id),
            heartbeat: _heartbeat,
            inner: Arc::new(RwLock::new(None)),
            rx_inbound: None,
            reader_task: None,
        }
    }

    /// Returns the credential associated with this client, if any.
    #[must_use]
    pub fn credential(&self) -> Option<&DydxCredential> {
        self.credential.as_ref()
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same ID will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.id().symbol.inner();
        self.instruments_cache.insert(symbol, instrument);
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same IDs will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.instruments_cache
                .insert(instrument.id().symbol.inner(), instrument);
        }
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Retrieves an instrument from the cache by symbol.
    ///
    /// Returns `None` if the instrument is not found.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).map(|r| r.clone())
    }

    /// Takes ownership of the inbound typed message receiver.
    /// Returns None if the receiver has already been taken or not connected.
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<super::messages::DydxWsMessage>> {
        self.rx_inbound.take()
    }

    /// Connects the websocket client in handler mode with automatic reconnection.
    ///
    /// Spawns a background task to decode raw websocket messages into typed
    /// [`super::messages::DydxWsMessage`] values and forwards them through an internal channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> DydxWsResult<()> {
        if self.is_connected() {
            return Ok(());
        }

        let (message_handler, mut raw_rx) = channel_message_handler();

        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            message_handler: Some(message_handler),
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
        };

        let client = WebSocketClient::connect(cfg, None, vec![], None)
            .await
            .map_err(|e| DydxWsError::Transport(e.to_string()))?;

        // Set inner client
        {
            let mut guard = self.inner.write().await;
            *guard = Some(client);
        }

        // Inbound typed message channel
        let (tx_inbound, rx_inbound) =
            tokio::sync::mpsc::unbounded_channel::<super::messages::DydxWsMessage>();

        // Spawn reader task to decode messages
        let reader_task = tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                match msg {
                    Message::Text(txt) => {
                        if txt == RECONNECTED {
                            let _ = tx_inbound.send(super::messages::DydxWsMessage::Reconnected);
                            continue;
                        }

                        match serde_json::from_str::<serde_json::Value>(&txt) {
                            Ok(val) => {
                                // Attempt to classify message using generic envelope
                                match serde_json::from_value::<super::messages::DydxWsGenericMsg>(
                                    val.clone(),
                                ) {
                                    Ok(meta) => {
                                        let result = if meta.is_connected() {
                                            serde_json::from_value::<
                                                super::messages::DydxWsConnectedMsg,
                                            >(val)
                                            .map(super::messages::DydxWsMessage::Connected)
                                        } else if meta.is_subscribed() {
                                            serde_json::from_value::<
                                                super::messages::DydxWsSubscriptionMsg,
                                            >(val)
                                            .map(super::messages::DydxWsMessage::Subscribed)
                                        } else if meta.is_unsubscribed() {
                                            serde_json::from_value::<
                                                super::messages::DydxWsSubscriptionMsg,
                                            >(val)
                                            .map(super::messages::DydxWsMessage::Unsubscribed)
                                        } else if meta.is_channel_data() {
                                            serde_json::from_value::<
                                                super::messages::DydxWsChannelDataMsg,
                                            >(val)
                                            .map(super::messages::DydxWsMessage::ChannelData)
                                        } else if meta.is_channel_batch_data() {
                                            serde_json::from_value::<
                                                super::messages::DydxWsChannelBatchDataMsg,
                                            >(val)
                                            .map(super::messages::DydxWsMessage::ChannelBatchData)
                                        } else if meta.is_error() {
                                            serde_json::from_value::<super::error::DydxWebSocketError>(val)
                                                .map(super::messages::DydxWsMessage::Error)
                                        } else {
                                            Ok(super::messages::DydxWsMessage::Raw(val))
                                        };

                                        match result {
                                            Ok(msg) => {
                                                let _ = tx_inbound.send(msg);
                                            }
                                            Err(e) => {
                                                let err =
                                                    super::error::DydxWebSocketError::from_message(
                                                        e.to_string(),
                                                    );
                                                let _ = tx_inbound.send(
                                                    super::messages::DydxWsMessage::Error(err),
                                                );
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        // Fallback to raw if generic parse fails
                                        let _ = tx_inbound
                                            .send(super::messages::DydxWsMessage::Raw(val));
                                    }
                                }
                            }
                            Err(e) => {
                                let err =
                                    super::error::DydxWebSocketError::from_message(e.to_string());
                                let _ = tx_inbound.send(super::messages::DydxWsMessage::Error(err));
                            }
                        }
                    }
                    Message::Ping(_data) => {
                        // Handled by lower layers where appropriate
                    }
                    Message::Pong(_data) => {
                        let _ = tx_inbound.send(super::messages::DydxWsMessage::Pong);
                    }
                    Message::Binary(_bin) => {
                        // dYdX uses text frames; ignore binary
                    }
                    Message::Close(_frame) => {
                        break;
                    }
                    Message::Frame(_) => {}
                }
            }
        });

        self.rx_inbound = Some(rx_inbound);
        self.reader_task = Some(reader_task);
        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!("Connected dYdX WebSocket: {}", self.url);
        Ok(())
    }

    /// Disconnects the websocket client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client cannot be accessed.
    pub async fn disconnect(&mut self) -> DydxWsResult<()> {
        // Close inner client if exists
        {
            let guard = self.inner.read().await;
            if let Some(inner) = guard.as_ref() {
                inner.disconnect().await;
            }
        }

        self.is_connected.store(false, Ordering::Relaxed);

        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }

        // Drop receiver to stop any consumers
        self.rx_inbound = None;
        Ok(())
    }

    async fn send_text_inner(&self, text: &str) -> DydxWsResult<()> {
        let guard = self.inner.read().await;
        let client = guard.as_ref().ok_or(DydxWsError::NotConnected)?;
        client
            .send_text(text.to_string(), None)
            .await
            .map_err(DydxWsError::from)
    }

    fn ticker_from_instrument_id(instrument_id: &InstrumentId) -> String {
        let mut s = instrument_id.symbol.as_str().to_string();
        if let Some(stripped) = s.strip_suffix("-PERP") {
            s = stripped.to_string();
        }
        s
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#trades-channel>
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::Trades,
            id: Some(ticker),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::Trades,
            id: Some(ticker),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#orderbook-channel>
    pub async fn subscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::Orderbook,
            id: Some(ticker),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::Orderbook,
            id: Some(ticker),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Subscribes to candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#candles-channel>
    pub async fn subscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let id = format!("{ticker}-{resolution}");
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::Candles,
            id: Some(id),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let id = format!("{ticker}-{resolution}");
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::Candles,
            id: Some(id),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Subscribes to market updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#markets-channel>
    pub async fn subscribe_markets(&self) -> DydxWsResult<()> {
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::Markets,
            id: None,
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from market updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_markets(&self) -> DydxWsResult<()> {
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::Markets,
            id: None,
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Subscribes to subaccount updates (orders, fills, positions, balances).
    ///
    /// This requires authentication and will only work for private WebSocket clients
    /// created with [`Self::new_private`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client was not created with credentials or if the
    /// subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#subaccounts-channel>
    pub async fn subscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        if !self.requires_auth {
            return Err(DydxWsError::Authentication(
                "Subaccount subscriptions require authentication. Use new_private() to create an authenticated client".to_string(),
            ));
        }
        let id = format!("{address}/{subaccount_number}");
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::Subaccounts,
            id: Some(id),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from subaccount updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        let id = format!("{address}/{subaccount_number}");
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::Subaccounts,
            id: Some(id),
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Subscribes to block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#block-height-channel>
    pub async fn subscribe_block_height(&self) -> DydxWsResult<()> {
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Subscribe,
            channel: super::enums::DydxWsChannel::BlockHeight,
            id: None,
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }

    /// Unsubscribes from block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_block_height(&self) -> DydxWsResult<()> {
        let sub = super::messages::DydxSubscription {
            op: super::enums::DydxWsOperation::Unsubscribe,
            channel: super::enums::DydxWsChannel::BlockHeight,
            id: None,
        };
        let payload = serde_json::to_string(&sub)?;
        self.send_text_inner(&payload).await
    }
}

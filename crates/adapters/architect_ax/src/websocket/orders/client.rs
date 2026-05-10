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

//! Orders WebSocket client for Ax.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    AtomicMap,
    consts::NAUTILUS_USER_AGENT,
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, PingHandler, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use ustr::Ustr;

use super::handler::{AxOrdersWsFeedHandler, HandlerCommand, WsOrderInfo};
use crate::{
    common::{
        consts::AX_NAUTILUS_TAG,
        enums::{AxOrderRequestType, AxOrderSide, AxOrderType, AxTimeInForce},
        parse::{client_order_id_to_cid, quantity_to_contracts},
    },
    websocket::messages::{AxOrdersWsMessage, AxWsPlaceOrder, OrderMetadata},
};

/// Result type for Ax orders WebSocket operations.
pub type AxOrdersWsResult<T> = Result<T, AxOrdersWsClientError>;

/// Shared caches for order state tracking between the client and consumers.
#[derive(Debug, Clone)]
pub struct OrdersCaches {
    /// Maps client order IDs to order metadata.
    pub orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
    /// Maps venue order IDs to client order IDs.
    pub venue_to_client_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
    /// Maps AX cid values to client order IDs.
    pub cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
}

impl Default for OrdersCaches {
    fn default() -> Self {
        Self {
            orders_metadata: Arc::new(DashMap::new()),
            venue_to_client_id: Arc::new(DashMap::new()),
            cid_to_client_order_id: Arc::new(DashMap::new()),
        }
    }
}

/// Error type for the Ax orders WebSocket client.
#[derive(Debug, Clone)]
pub enum AxOrdersWsClientError {
    /// Transport/connection error.
    Transport(String),
    /// Channel send error.
    ChannelError(String),
    /// Authentication error.
    AuthenticationError(String),
    /// Client-side validation error.
    ClientError(String),
}

impl core::fmt::Display for AxOrdersWsClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "Transport error: {msg}"),
            Self::ChannelError(msg) => write!(f, "Channel error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
        }
    }
}

impl std::error::Error for AxOrdersWsClientError {}

impl From<&'static str> for AxOrdersWsClientError {
    fn from(msg: &'static str) -> Self {
        Self::ClientError(msg.to_string())
    }
}

/// Orders WebSocket client for Ax.
///
/// Provides authenticated order management including placing, canceling,
/// and monitoring order status via WebSocket.
pub struct AxOrdersWebSocketClient {
    clock: &'static AtomicTime,
    url: String,
    heartbeat: Option<u64>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<AxOrdersWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    auth_tracker: AuthTracker,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    caches: OrdersCaches,
    request_id_counter: Arc<AtomicI64>,
    account_id: AccountId,
    trader_id: TraderId,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
}

impl Debug for AxOrdersWebSocketClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct(stringify!(AxOrdersWebSocketClient))
            .field("url", &self.url)
            .field("heartbeat", &self.heartbeat)
            .field("account_id", &self.account_id)
            .finish()
    }
}

impl Clone for AxOrdersWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            clock: self.clock,
            url: self.url.clone(),
            heartbeat: self.heartbeat,
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None, // Each clone gets its own receiver
            signal: Arc::clone(&self.signal),
            task_handle: None,
            auth_tracker: self.auth_tracker.clone(),
            instruments_cache: Arc::clone(&self.instruments_cache),
            caches: self.caches.clone(),
            request_id_counter: Arc::clone(&self.request_id_counter),
            account_id: self.account_id,
            trader_id: self.trader_id,
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl AxOrdersWebSocketClient {
    /// Creates a new Ax orders WebSocket client.
    #[must_use]
    pub fn new(
        url: String,
        account_id: AccountId,
        trader_id: TraderId,
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            clock: get_atomic_clock_realtime(),
            url,
            heartbeat: Some(heartbeat),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            auth_tracker: AuthTracker::default(),
            instruments_cache: Arc::new(AtomicMap::new()),
            caches: OrdersCaches::default(),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            account_id,
            trader_id,
            transport_backend,
            proxy_url,
        }
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns whether the client is currently connected and active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Acquire)
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Acquire)
    }

    /// Generates a unique request ID.
    fn next_request_id(&self) -> i64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Caches an instrument for use during message parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.symbol().inner();
        self.instruments_cache.insert(symbol, instrument);
    }

    /// Caches multiple instruments for use during message parsing.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments_cache.rcu(|m| {
            for inst in instruments {
                m.insert(inst.symbol().inner(), inst.clone());
            }
        });
    }

    /// Returns a cached instrument by symbol.
    #[must_use]
    pub fn get_cached_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(symbol)
    }

    /// Returns the shared order caches.
    #[must_use]
    pub fn caches(&self) -> &OrdersCaches {
        &self.caches
    }

    /// Returns the instruments cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<AtomicMap<Ustr, InstrumentAny>> {
        Arc::clone(&self.instruments_cache)
    }

    /// Returns the orders metadata cache.
    #[must_use]
    pub fn orders_metadata(&self) -> &Arc<DashMap<ClientOrderId, OrderMetadata>> {
        &self.caches.orders_metadata
    }

    /// Returns the cid to client order ID mapping for order correlation.
    #[must_use]
    pub fn cid_to_client_order_id(&self) -> &Arc<DashMap<u64, ClientOrderId>> {
        &self.caches.cid_to_client_order_id
    }

    /// Resolves a cid to a ClientOrderId if the mapping exists.
    #[must_use]
    pub fn resolve_cid(&self, cid: u64) -> Option<ClientOrderId> {
        self.caches.cid_to_client_order_id.get(&cid).map(|v| *v)
    }

    /// Registers an external order with the WebSocket handler for event tracking.
    ///
    /// This allows the handler to create proper events (e.g., OrderCanceled, OrderFilled)
    /// for orders that were reconciled externally and not submitted through this client.
    ///
    /// Returns `false` if the instrument is not cached (registration skipped).
    pub fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> bool {
        if self.caches.orders_metadata.contains_key(&client_order_id) {
            return true;
        }

        // Required for correct precision on fills
        let symbol = instrument_id.symbol.inner();
        let Some(instrument) = self.get_cached_instrument(&symbol) else {
            log::warn!(
                "Cannot register external order {client_order_id}: \
                 instrument {instrument_id} not in cache"
            );
            return false;
        };

        let metadata = OrderMetadata {
            trader_id: self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id: Some(venue_order_id),
            ts_init: self.generate_ts_init(),
            size_precision: instrument.size_precision(),
            price_precision: instrument.price_precision(),
            quote_currency: instrument.quote_currency(),
            pending_trigger_price: None,
        };

        self.caches
            .orders_metadata
            .insert(client_order_id, metadata);
        self.caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        log::debug!(
            "Registered external order {client_order_id} ({venue_order_id}) for {instrument_id} [{strategy_id}]"
        );

        true
    }

    /// Establishes the WebSocket connection with authentication.
    ///
    /// # Arguments
    ///
    /// * `bearer_token` - The bearer token for authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self, bearer_token: &str) -> AxOrdersWsResult<()> {
        const MAX_RETRIES: u32 = 5;
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

        self.signal.store(false, Ordering::Release);

        let (raw_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![
                ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
                (
                    "Authorization".to_string(),
                    format!("Bearer {bearer_token}"),
                ),
            ],
            heartbeat: self.heartbeat,
            heartbeat_msg: None, // Ax server sends heartbeats
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        // Retry initial connection with exponential backoff
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_millis(5000),
            2.0,
            250,
            false,
        )
        .map_err(|e| AxOrdersWsClientError::Transport(e.to_string()))?;

        let mut last_error: String;
        let mut attempt = 0;

        let client = loop {
            attempt += 1;

            match tokio::time::timeout(
                Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                WebSocketClient::connect(
                    config.clone(),
                    Some(raw_handler.clone()),
                    Some(ping_handler.clone()),
                    None,
                    vec![],
                    None,
                ),
            )
            .await
            {
                Ok(Ok(client)) => {
                    if attempt > 1 {
                        log::info!("WebSocket connection established after {attempt} attempts");
                    }
                    break client;
                }
                Ok(Err(e)) => {
                    last_error = e.to_string();
                    log::warn!(
                        "WebSocket connection attempt failed: attempt={attempt}, max_retries={MAX_RETRIES}, url={}, error={last_error}",
                        self.url
                    );
                }
                Err(_) => {
                    last_error = format!("Connection timeout after {CONNECTION_TIMEOUT_SECS}s");
                    log::warn!(
                        "WebSocket connection attempt timed out: attempt={attempt}, max_retries={MAX_RETRIES}, url={}",
                        self.url
                    );
                }
            }

            if attempt >= MAX_RETRIES {
                return Err(AxOrdersWsClientError::Transport(format!(
                    "Failed to connect to {} after {MAX_RETRIES} attempts: {}",
                    self.url,
                    if last_error.is_empty() {
                        "unknown error"
                    } else {
                        &last_error
                    }
                )));
            }

            let delay = backoff.next_duration();
            log::debug!(
                "Retrying in {delay:?} (attempt {}/{MAX_RETRIES})",
                attempt + 1
            );
            tokio::time::sleep(delay).await;
        };

        self.connection_mode.store(client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<AxOrdersWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        self.send_cmd(HandlerCommand::SetClient(client)).await?;

        // Bearer token is passed in connection headers
        self.send_cmd(HandlerCommand::Authenticate {
            token: bearer_token.to_string(),
        })
        .await?;

        let signal = Arc::clone(&self.signal);
        let auth_tracker = self.auth_tracker.clone();
        let orders_metadata = Arc::clone(&self.caches.orders_metadata);
        let cid_to_client_order_id = Arc::clone(&self.caches.cid_to_client_order_id);

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = AxOrdersWsFeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                auth_tracker.clone(),
                orders_metadata,
                cid_to_client_order_id,
            );

            while let Some(msg) = handler.next().await {
                if matches!(msg, AxOrdersWsMessage::Reconnected) {
                    log::info!("WebSocket reconnected, authentication will be restored");
                }

                if out_tx.send(msg).is_err() {
                    log::debug!("Output channel closed");
                    break;
                }
            }

            log::debug!("Handler loop exited");
        });

        self.task_handle = Some(stream_handle);

        Ok(())
    }

    /// Submits an order using Nautilus domain types.
    ///
    /// This method handles conversion from Nautilus domain types to AX-specific
    /// types and stores order metadata for event correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The order type is not supported (only MARKET (simulated), LIMIT and STOP_LIMIT).
    /// - The time-in-force is not supported.
    /// - The instrument is not found in the cache.
    /// - A limit order is missing a price.
    /// - A stop-loss order is missing a trigger price.
    /// - The order command cannot be sent.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
    ) -> AxOrdersWsResult<i64> {
        if !matches!(
            order_type,
            OrderType::Market | OrderType::Limit | OrderType::StopLimit
        ) {
            return Err(AxOrdersWsClientError::ClientError(format!(
                "Unsupported order type: {order_type:?}. AX supports MARKET, LIMIT and STOP_LIMIT."
            )));
        }

        // Get instrument from cache for precision
        let symbol = instrument_id.symbol.inner();
        let instrument = self.get_cached_instrument(&symbol).ok_or_else(|| {
            AxOrdersWsClientError::ClientError(format!(
                "Instrument {instrument_id} not found in cache"
            ))
        })?;

        let ax_side = AxOrderSide::try_from(order_side)?;

        let qty_contracts = quantity_to_contracts(quantity)
            .map_err(|e| AxOrdersWsClientError::ClientError(e.to_string()))?;

        // Market orders are simulated as IOC limit orders with aggressive pricing
        // because Architect does not support native market orders
        let request_id = self.next_request_id();

        let (ax_price, ax_tif, ax_post_only, ax_order_type, ax_trigger_price) = match order_type {
            OrderType::Market => {
                let market_price = price.ok_or_else(|| {
                    AxOrdersWsClientError::ClientError(
                        "Market order requires price (calculated from quote)".to_string(),
                    )
                })?;
                (
                    market_price.as_decimal(),
                    AxTimeInForce::Ioc,
                    false,
                    None,
                    None,
                )
            }
            OrderType::Limit => {
                let ax_tif = AxTimeInForce::try_from(time_in_force)?;
                let limit_price = price.ok_or_else(|| {
                    AxOrdersWsClientError::ClientError("Limit order requires price".to_string())
                })?;
                (limit_price.as_decimal(), ax_tif, post_only, None, None)
            }
            OrderType::StopLimit => {
                let ax_tif = AxTimeInForce::try_from(time_in_force)?;
                let limit_price = price.ok_or_else(|| {
                    AxOrdersWsClientError::ClientError(
                        "Stop-limit order requires price".to_string(),
                    )
                })?;
                let stop_price = trigger_price.ok_or_else(|| {
                    AxOrdersWsClientError::ClientError(
                        "Stop-limit order requires trigger price".to_string(),
                    )
                })?;
                (
                    limit_price.as_decimal(),
                    ax_tif,
                    false,
                    Some(AxOrderType::StopLossLimit),
                    Some(stop_price.as_decimal()),
                )
            }
            _ => {
                return Err(AxOrdersWsClientError::ClientError(format!(
                    "Unsupported order type: {order_type:?}"
                )));
            }
        };

        // Store order metadata for event correlation (after validation to avoid stale entries)
        let metadata = OrderMetadata {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id: None,
            ts_init: self.generate_ts_init(),
            size_precision: instrument.size_precision(),
            price_precision: instrument.price_precision(),
            quote_currency: instrument.quote_currency(),
            pending_trigger_price: None,
        };
        self.caches
            .orders_metadata
            .insert(client_order_id, metadata);

        // Store cid -> client_order_id mapping for correlation
        let cid = client_order_id_to_cid(&client_order_id);
        self.caches
            .cid_to_client_order_id
            .insert(cid, client_order_id);

        let order = AxWsPlaceOrder {
            rid: request_id,
            t: AxOrderRequestType::PlaceOrder,
            s: symbol,
            d: ax_side,
            q: qty_contracts,
            p: ax_price,
            tif: ax_tif,
            po: ax_post_only,
            tag: Some(AX_NAUTILUS_TAG.to_string()),
            cid: Some(cid),
            order_type: ax_order_type,
            trigger_price: ax_trigger_price,
        };

        let order_info = WsOrderInfo {
            client_order_id,
            symbol,
        };

        let result = self
            .send_cmd(HandlerCommand::PlaceOrder {
                request_id,
                order,
                order_info,
            })
            .await;

        if result.is_err() {
            self.caches.orders_metadata.remove(&client_order_id);
            self.caches.cid_to_client_order_id.remove(&cid);
        }

        result?;
        Ok(request_id)
    }

    /// Cancels an order via WebSocket.
    ///
    /// Requires a known `venue_order_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel command cannot be sent.
    pub async fn cancel_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> AxOrdersWsResult<i64> {
        let order_id = venue_order_id.map(|v| v.to_string()).ok_or_else(|| {
            AxOrdersWsClientError::ClientError(format!(
                "Cannot cancel order {client_order_id}: missing venue_order_id"
            ))
        })?;

        let request_id = self.next_request_id();

        self.send_cmd(HandlerCommand::CancelOrder {
            request_id,
            order_id,
        })
        .await?;

        Ok(request_id)
    }

    /// Requests open orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the request command cannot be sent.
    pub async fn get_open_orders(&self) -> AxOrdersWsResult<i64> {
        let request_id = self.next_request_id();

        self.send_cmd(HandlerCommand::GetOpenOrders { request_id })
            .await?;

        Ok(request_id)
    }

    /// Returns a stream of WebSocket messages.
    ///
    /// # Panics
    ///
    /// Panics if called before `connect()` or if the stream has already been taken.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = AxOrdersWsMessage> + 'static {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or client not connected - stream() can only be called once");
        let mut rx = Arc::try_unwrap(rx).expect(
            "Cannot take ownership of stream - client was cloned and other references exist",
        );
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Disconnects the WebSocket connection gracefully.
    pub async fn disconnect(&self) {
        log::debug!("Disconnecting WebSocket");
        let _ = self.send_cmd(HandlerCommand::Disconnect).await;
    }

    /// Closes the WebSocket connection and cleans up resources.
    pub async fn close(&mut self) {
        log::debug!("Closing WebSocket client");

        // Send disconnect first to allow graceful cleanup before signal
        let _ = self.send_cmd(HandlerCommand::Disconnect).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.signal.store(true, Ordering::Release);

        if let Some(handle) = self.task_handle.take() {
            const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
            let abort_handle = handle.abort_handle();

            match tokio::time::timeout(CLOSE_TIMEOUT, handle).await {
                Ok(Ok(())) => log::debug!("Handler task completed gracefully"),
                Ok(Err(e)) => log::warn!("Handler task panicked: {e}"),
                Err(_) => {
                    log::warn!("Handler task did not complete within timeout, aborting");
                    abort_handle.abort();
                }
            }
        }
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> AxOrdersWsResult<()> {
        let guard = self.cmd_tx.read().await;
        guard
            .send(cmd)
            .map_err(|e| AxOrdersWsClientError::ChannelError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn test_cancel_order_rejects_without_venue_order_id() {
        let client = AxOrdersWebSocketClient::new(
            "wss://example.com/orders/ws".to_string(),
            AccountId::from("AX-001"),
            TraderId::from("TRADER-001"),
            30,
            TransportBackend::default(),
            None,
        );
        let client_order_id = ClientOrderId::from("CID-123");

        let result = client.cancel_order(client_order_id, None).await;

        assert!(matches!(
            result,
            Err(AxOrdersWsClientError::ClientError(msg))
            if msg.contains("missing venue_order_id")
        ));
    }

    #[tokio::test]
    async fn test_cancel_order_sends_known_venue_order_id() {
        let mut client = AxOrdersWebSocketClient::new(
            "wss://example.com/orders/ws".to_string(),
            AccountId::from("AX-001"),
            TraderId::from("TRADER-001"),
            30,
            TransportBackend::default(),
            None,
        );

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        client.cmd_tx = Arc::new(tokio::sync::RwLock::new(cmd_tx));

        let client_order_id = ClientOrderId::from("CID-456");
        let venue_order_id = VenueOrderId::from("V-ORDER-789");

        let request_id = client
            .cancel_order(client_order_id, Some(venue_order_id))
            .await
            .unwrap();

        assert_eq!(request_id, 1);
        let cmd = cmd_rx.recv().await.unwrap();
        match cmd {
            HandlerCommand::CancelOrder {
                request_id,
                order_id,
            } => {
                assert_eq!(request_id, 1);
                assert_eq!(order_id, "V-ORDER-789");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}

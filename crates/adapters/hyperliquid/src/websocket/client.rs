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

use std::{
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::{cache::fifo::FifoCacheMap, live::get_runtime};
use nautilus_core::{AtomicMap, MUTEX_POISONED};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{HTTP_TIMEOUT, ws_url},
        enums::{HyperliquidBarInterval, HyperliquidEnvironment},
        parse::{
            bar_type_to_interval, clamp_price_to_precision, derive_limit_from_trigger,
            determine_order_list_grouping, extract_error_message, extract_inner_error,
            extract_inner_errors, normalize_price, order_to_hyperliquid_request_with_asset,
            round_to_sig_figs, time_in_force_to_hyperliquid_tif,
        },
    },
    http::{
        client::HyperliquidHttpClient,
        error::{Error as HyperliquidError, Result as HyperliquidResult},
        models::{
            Cloid, HyperliquidExchangeResponse, HyperliquidExecAction,
            HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelOrderRequest,
            HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest,
            HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
            HyperliquidExecTpSl, HyperliquidExecTriggerParams, RESPONSE_STATUS_OK,
        },
        rate_limits::{WeightedLimiter, exec_action_weight},
    },
    websocket::{
        enums::HyperliquidWsChannel,
        handler::{FeedHandler, HandlerCommand},
        messages::{
            NautilusWsMessage, PostRequest, PostResponse, PostResponsePayload, SubscriptionRequest,
        },
        post::{PostIds, PostRouter},
    },
};

const HYPERLIQUID_HEARTBEAT_MSG: &str = r#"{"method":"ping"}"#;

/// FIFO bound on the cloid -> `ClientOrderId` resolution cache so missed
/// evictions self-recover (see GH-3972 cancel-replace drain path).
pub(super) const CLOID_CACHE_CAPACITY: usize = 10_000;

/// Shared cloid -> `ClientOrderId` cache used by the WS handler.
pub(super) type CloidCache = Arc<Mutex<FifoCacheMap<Ustr, ClientOrderId, CLOID_CACHE_CAPACITY>>>;

/// Represents the different data types available from asset context subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AssetContextDataType {
    MarkPrice,
    IndexPrice,
    FundingRate,
}

/// Hyperliquid WebSocket client following the BitMEX pattern.
///
/// Orchestrates WebSocket connection and subscriptions using a command-based architecture,
/// where the inner FeedHandler owns the WebSocketClient and handles all I/O.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.hyperliquid")
)]
pub struct HyperliquidWebSocketClient {
    url: String,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    bar_types: Arc<AtomicMap<String, BarType>>,
    asset_context_subs: Arc<DashMap<Ustr, AHashSet<AssetContextDataType>>>,
    cloid_cache: CloidCache,
    post_router: Arc<PostRouter>,
    post_ids: Arc<PostIds>,
    post_limiter: Arc<WeightedLimiter>,
    post_timeout: Duration,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    account_id: Option<AccountId>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
}

impl Clone for HyperliquidWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            connection_mode: Arc::clone(&self.connection_mode),
            signal: Arc::clone(&self.signal),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            auth_tracker: self.auth_tracker.clone(),
            subscriptions: self.subscriptions.clone(),
            instruments: Arc::clone(&self.instruments),
            bar_types: Arc::clone(&self.bar_types),
            asset_context_subs: Arc::clone(&self.asset_context_subs),
            cloid_cache: Arc::clone(&self.cloid_cache),
            post_router: Arc::clone(&self.post_router),
            post_ids: Arc::clone(&self.post_ids),
            post_limiter: Arc::clone(&self.post_limiter),
            post_timeout: self.post_timeout,
            task_handle: None,
            account_id: self.account_id,
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client without connecting.
    ///
    /// If `url` is `None`, the appropriate URL will be determined from the `environment`:
    /// - `Mainnet`: `wss://api.hyperliquid.xyz/ws`
    /// - `Testnet`: `wss://api.hyperliquid-testnet.xyz/ws`
    ///
    /// The connection will be established when `connect()` is called.
    pub fn new(
        url: Option<String>,
        environment: HyperliquidEnvironment,
        account_id: Option<AccountId>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let url = url.unwrap_or_else(|| ws_url(environment).to_string());
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));
        Self {
            url,
            connection_mode,
            signal: Arc::new(AtomicBool::new(false)),
            auth_tracker: AuthTracker::new(),
            subscriptions: SubscriptionState::new(':'),
            instruments: Arc::new(AtomicMap::new()),
            bar_types: Arc::new(AtomicMap::new()),
            asset_context_subs: Arc::new(DashMap::new()),
            cloid_cache: Arc::new(Mutex::new(FifoCacheMap::new())),
            post_router: PostRouter::new(),
            post_ids: Arc::new(PostIds::new(1)),
            post_limiter: Arc::new(WeightedLimiter::per_minute(1200)),
            post_timeout: HTTP_TIMEOUT,
            cmd_tx: {
                // Placeholder channel until connect() creates the real handler and replays queued instruments
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                Arc::new(tokio::sync::RwLock::new(tx))
            },
            out_rx: None,
            task_handle: None,
            account_id,
            transport_backend,
            proxy_url,
        }
    }

    /// Establishes WebSocket connection and spawns the message handler.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_active() {
            log::warn!("WebSocket already connected");
            return Ok(());
        }
        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(30),
            heartbeat_msg: Some(HYPERLIQUID_HEARTBEAT_MSG.to_string()),
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };
        let client =
            WebSocketClient::connect(cfg, Some(message_handler), None, None, vec![], None).await?;

        // Create channels for handler communication
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        // Update cmd_tx before connection_mode to avoid race where is_active() returns
        // true but subscriptions still go to the old placeholder channel
        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);

        self.connection_mode.store(client.connection_mode_atomic());
        log::info!("Hyperliquid WebSocket connected: {}", self.url);

        // Send SetClient command immediately
        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            anyhow::bail!("Failed to send SetClient command: {e}");
        }

        // Initialize handler with existing instruments
        let instruments_vec: Vec<InstrumentAny> =
            self.instruments.load().values().cloned().collect();

        if !instruments_vec.is_empty()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments_vec))
        {
            log::error!("Failed to send InitializeInstruments: {e}");
        }

        // Spawn handler task
        let signal = Arc::clone(&self.signal);
        let account_id = self.account_id;
        let subscriptions = self.subscriptions.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let cloid_cache = Arc::clone(&self.cloid_cache);
        let post_router = Arc::clone(&self.post_router);

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal,
                cmd_rx,
                raw_rx,
                out_tx,
                account_id,
                subscriptions.clone(),
                cloid_cache,
                post_router,
            );

            let resubscribe_all = || {
                let topics = subscriptions.all_topics();
                if topics.is_empty() {
                    log::debug!("No active subscriptions to restore after reconnection");
                    return;
                }

                log::info!(
                    "Resubscribing to {} active subscriptions after reconnection",
                    topics.len()
                );

                for topic in topics {
                    match subscription_from_topic(&topic) {
                        Ok(subscription) => {
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe {
                                subscriptions: vec![subscription],
                            }) {
                                log::error!("Failed to send resubscribe command: {e}");
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to reconstruct subscription from topic: topic={topic}, {e}"
                            );
                        }
                    }
                }
            };

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        log::info!("WebSocket reconnected");
                        resubscribe_all();
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            log::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        log::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }
            log::debug!("Handler task completed");
        });
        self.task_handle = Some(stream_handle);
        Ok(())
    }

    /// Takes the handler task handle from this client so that another
    /// instance (e.g., the non-clone original) can await it on disconnect.
    pub fn take_task_handle(&mut self) -> Option<tokio::task::JoinHandle<()>> {
        self.task_handle.take()
    }

    pub fn set_task_handle(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.task_handle = Some(handle);
    }

    pub fn set_post_timeout(&mut self, timeout: Duration) {
        self.post_timeout = timeout;
    }

    /// Force-close fallback for the sync `stop()` path.
    /// Prefer `disconnect()` for graceful shutdown.
    pub(crate) fn abort(&mut self) {
        self.signal.store(true, Ordering::Relaxed);
        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }

    /// Disconnects the WebSocket connection.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting Hyperliquid WebSocket");
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(handle) = self.task_handle.take() {
            log::debug!("Waiting for task handle to complete");
            let abort_handle = handle.abort_handle();
            tokio::select! {
                result = handle => {
                    match result {
                        Ok(()) => log::debug!("Task handle completed successfully"),
                        Err(e) if e.is_cancelled() => {
                            log::debug!("Task was cancelled");
                        }
                        Err(e) => log::error!("Task handle encountered an error: {e:?}"),
                    }
                }
                () = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    log::warn!("Timeout waiting for task handle, aborting task");
                    abort_handle.abort();
                }
            }
        } else {
            log::debug!("No task handle to await");
        }
        log::debug!("Disconnected");
        Ok(())
    }

    /// Send a typed exchange action through the Hyperliquid WebSocket post API.
    ///
    /// The supplied HTTP client is used only as the canonical signer for the
    /// action envelope. The signed payload is sent over the active WebSocket
    /// connection and the response is correlated by post id.
    pub async fn post_action_exec(
        &self,
        signer: &HyperliquidHttpClient,
        action: &HyperliquidExecAction,
    ) -> HyperliquidResult<HyperliquidExchangeResponse> {
        self.post_action_exec_with_timeout(signer, action, self.post_timeout, None)
            .await
    }

    /// Send a typed exchange action with a caller-specified timeout and optional expiry.
    pub async fn post_action_exec_with_timeout(
        &self,
        signer: &HyperliquidHttpClient,
        action: &HyperliquidExecAction,
        timeout: Duration,
        expires_after: Option<u64>,
    ) -> HyperliquidResult<HyperliquidExchangeResponse> {
        let weight = exec_action_weight(action);
        self.post_limiter.acquire(weight).await;

        let payload = signer.sign_action_exec_request(action, expires_after)?;
        let response = self
            .send_post_request(PostRequest::Action { payload }, timeout)
            .await?;

        match response.response {
            PostResponsePayload::Action { payload } => {
                let parsed: HyperliquidExchangeResponse =
                    serde_json::from_value(payload).map_err(HyperliquidError::Serde)?;

                match &parsed {
                    HyperliquidExchangeResponse::Status {
                        status,
                        response: response_data,
                    } if status != RESPONSE_STATUS_OK => {
                        let error_msg = response_data
                            .as_str()
                            .map_or_else(|| response_data.to_string(), |s| s.to_string());
                        Err(HyperliquidError::bad_request(format!(
                            "API error: {error_msg}"
                        )))
                    }
                    HyperliquidExchangeResponse::Error { error } => {
                        Err(HyperliquidError::bad_request(format!("API error: {error}")))
                    }
                    _ => Ok(parsed),
                }
            }
            PostResponsePayload::Error { payload } => Err(map_post_payload_error(payload, weight)),
            PostResponsePayload::Info { payload } => Err(HyperliquidError::decode(format!(
                "expected action post response, received info payload: {payload}"
            ))),
        }
    }

    /// Submit an order through the Hyperliquid WebSocket post API.
    ///
    /// The HTTP client supplies signing credentials, builder attribution, and
    /// cached instrument metadata. The action itself is sent over WebSocket.
    #[allow(
        clippy::too_many_arguments,
        reason = "matches the Python and HTTP order submit surface"
    )]
    pub async fn submit_order(
        &self,
        signer: &HyperliquidHttpClient,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
        reduce_only: bool,
    ) -> HyperliquidResult<()> {
        let symbol = instrument_id.symbol.as_str();
        let asset = signer.get_asset_index(symbol).ok_or_else(|| {
            HyperliquidError::bad_request(format!(
                "Asset index not found for symbol: {symbol}. Ensure instruments are loaded."
            ))
        })?;
        let is_buy = matches!(order_side, OrderSide::Buy);
        let price_precision = signer.get_price_precision(symbol).unwrap_or(2);

        let price_decimal = match price {
            Some(px) if signer.normalize_prices() => {
                normalize_price(px.as_decimal(), price_precision).normalize()
            }
            Some(px) => px.as_decimal().normalize(),
            None if matches!(order_type, OrderType::Market) => Decimal::ZERO,
            None if matches!(
                order_type,
                OrderType::StopMarket | OrderType::MarketIfTouched
            ) =>
            {
                match trigger_price {
                    Some(tp) => {
                        let derived = derive_limit_from_trigger(
                            tp.as_decimal().normalize(),
                            is_buy,
                            signer.market_order_slippage_bps(),
                        );
                        let sig_rounded = round_to_sig_figs(derived, 5);
                        clamp_price_to_precision(sig_rounded, price_precision, is_buy).normalize()
                    }
                    None => Decimal::ZERO,
                }
            }
            None => {
                return Err(HyperliquidError::bad_request(
                    "Limit orders require a price",
                ));
            }
        };

        let size_decimal = quantity.as_decimal().normalize();
        let kind = hyperliquid_order_kind(
            order_type,
            time_in_force,
            post_only,
            trigger_price,
            signer.normalize_prices(),
            price_precision,
        )?;

        let order = HyperliquidExecPlaceOrderRequest {
            asset,
            is_buy,
            price: price_decimal,
            size: size_decimal,
            reduce_only,
            kind,
            cloid: Some(Cloid::from_client_order_id(client_order_id)),
        };
        let action = HyperliquidExecAction::Order {
            orders: vec![order],
            grouping: HyperliquidExecGrouping::Na,
            builder: signer.builder_attribution(),
        };
        let response = self.post_action_exec(signer, &action).await?;

        ensure_ws_action_accepted(&response, "Order submission")
    }

    /// Submit multiple orders through the Hyperliquid WebSocket post API.
    pub async fn submit_orders(
        &self,
        signer: &HyperliquidHttpClient,
        orders: &[&OrderAny],
    ) -> HyperliquidResult<()> {
        let mut hyperliquid_orders = Vec::with_capacity(orders.len());

        for order in orders {
            let instrument_id = order.instrument_id();
            let symbol = instrument_id.symbol.as_str();
            let asset = signer.get_asset_index(symbol).ok_or_else(|| {
                HyperliquidError::bad_request(format!(
                    "Asset index not found for symbol: {symbol}. Ensure instruments are loaded."
                ))
            })?;
            let price_decimals = signer.get_price_precision(symbol).unwrap_or(2);
            let request = order_to_hyperliquid_request_with_asset(
                order,
                asset,
                price_decimals,
                signer.normalize_prices(),
                signer.market_order_slippage_bps(),
            )
            .map_err(|e| HyperliquidError::bad_request(format!("Failed to convert order: {e}")))?;
            hyperliquid_orders.push(request);
        }

        let grouping =
            determine_order_list_grouping(&orders.iter().copied().cloned().collect::<Vec<_>>());
        let action = HyperliquidExecAction::Order {
            orders: hyperliquid_orders,
            grouping,
            builder: signer.builder_attribution(),
        };
        let response = self.post_action_exec(signer, &action).await?;

        ensure_ws_action_accepted(&response, "Order list submission")
    }

    /// Cancel an order through the Hyperliquid WebSocket post API.
    pub async fn cancel_order(
        &self,
        signer: &HyperliquidHttpClient,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> HyperliquidResult<()> {
        let symbol = instrument_id.symbol.as_str();
        let asset = signer.get_asset_index(symbol).ok_or_else(|| {
            HyperliquidError::bad_request(format!(
                "Asset index not found for symbol: {symbol}. Ensure instruments are loaded."
            ))
        })?;
        let action = if let Some(cloid) = client_order_id {
            let cancel_req = HyperliquidExecCancelByCloidRequest {
                asset,
                cloid: Cloid::from_client_order_id(cloid),
            };
            HyperliquidExecAction::CancelByCloid {
                cancels: vec![cancel_req],
            }
        } else if let Some(oid) = venue_order_id {
            let oid = oid
                .as_str()
                .parse::<u64>()
                .map_err(|_| HyperliquidError::bad_request("Invalid venue order ID format"))?;
            let cancel_req = HyperliquidExecCancelOrderRequest { asset, oid };
            HyperliquidExecAction::Cancel {
                cancels: vec![cancel_req],
            }
        } else {
            return Err(HyperliquidError::bad_request(
                "Either client_order_id or venue_order_id must be provided",
            ));
        };
        let response = self.post_action_exec(signer, &action).await?;

        ensure_ws_action_accepted(&response, "Cancel order")
    }

    /// Cancel multiple orders through one Hyperliquid WebSocket post action.
    pub async fn cancel_orders(
        &self,
        signer: &HyperliquidHttpClient,
        cancels: &[(InstrumentId, ClientOrderId, Option<VenueOrderId>)],
    ) -> HyperliquidResult<Vec<Option<String>>> {
        let mut cancel_requests = Vec::with_capacity(cancels.len());

        for (instrument_id, client_order_id, _) in cancels {
            let symbol = instrument_id.symbol.as_str();
            let asset = signer.get_asset_index(symbol).ok_or_else(|| {
                HyperliquidError::bad_request(format!(
                    "Asset index not found for symbol: {symbol}. Ensure instruments are loaded."
                ))
            })?;
            cancel_requests.push(HyperliquidExecCancelByCloidRequest {
                asset,
                cloid: Cloid::from_client_order_id(*client_order_id),
            });
        }

        if cancel_requests.is_empty() {
            return Ok(Vec::new());
        }

        let action = HyperliquidExecAction::CancelByCloid {
            cancels: cancel_requests,
        };
        let response = self.post_action_exec(signer, &action).await?;

        if response.is_ok() {
            let errors = extract_inner_errors(&response);
            return cancel_errors_for_requests(errors, cancels.len());
        }

        Err(HyperliquidError::bad_request(format!(
            "Cancel orders failed: {}",
            extract_error_message(&response)
        )))
    }

    /// Modify an order through the Hyperliquid WebSocket post API.
    #[allow(
        clippy::too_many_arguments,
        reason = "matches the Python and HTTP order modify surface"
    )]
    pub async fn modify_order(
        &self,
        signer: &HyperliquidHttpClient,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        price: Price,
        quantity: Quantity,
        trigger_price: Option<Price>,
        reduce_only: bool,
        post_only: bool,
        time_in_force: TimeInForce,
        client_order_id: Option<ClientOrderId>,
    ) -> HyperliquidResult<()> {
        let symbol = instrument_id.symbol.as_str();
        let asset = signer.get_asset_index(symbol).ok_or_else(|| {
            HyperliquidError::bad_request(format!(
                "Asset index not found for symbol: {symbol}. Ensure instruments are loaded."
            ))
        })?;
        let oid = venue_order_id
            .as_str()
            .parse::<u64>()
            .map_err(|_| HyperliquidError::bad_request("Invalid venue order ID format"))?;
        let is_buy = matches!(order_side, OrderSide::Buy);
        let price_decimals = signer.get_price_precision(symbol).unwrap_or(2);
        let price = if signer.normalize_prices() {
            normalize_price(price.as_decimal(), price_decimals).normalize()
        } else {
            price.as_decimal().normalize()
        };
        let kind = hyperliquid_order_kind(
            order_type,
            time_in_force,
            post_only,
            trigger_price,
            signer.normalize_prices(),
            price_decimals,
        )?;
        let order = HyperliquidExecPlaceOrderRequest {
            asset,
            is_buy,
            price,
            size: quantity.as_decimal().normalize(),
            reduce_only,
            kind,
            cloid: client_order_id.map(Cloid::from_client_order_id),
        };
        let action = HyperliquidExecAction::Modify {
            modify: HyperliquidExecModifyOrderRequest { oid, order },
        };
        let response = self.post_action_exec(signer, &action).await?;

        ensure_ws_action_accepted(&response, "Modify order")
    }

    async fn send_post_request(
        &self,
        request: PostRequest,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponse> {
        let id = self.post_ids.next();

        match tokio::time::timeout(timeout, async {
            let rx = self.post_router.register(id).await?;

            let send_result = self
                .cmd_tx
                .read()
                .await
                .send(HandlerCommand::Post { id, request });

            if let Err(e) = send_result {
                self.post_router.cancel(id).await;
                return Err(HyperliquidError::transport(format!(
                    "post command channel closed: {e}"
                )));
            }

            self.post_router.await_with_timeout(id, rx, timeout).await
        })
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => {
                self.post_router.cancel(id).await;
                Err(HyperliquidError::Timeout)
            }
        }
    }

    /// Returns true if the WebSocket is actively connected.
    pub fn is_active(&self) -> bool {
        let mode = self.connection_mode.load();
        mode.load(Ordering::Relaxed) == ConnectionMode::Active as u8
    }

    /// Returns the URL of this WebSocket client.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Caches multiple instruments.
    ///
    /// Clears the existing cache first, then adds all provided instruments.
    /// Instruments are keyed by their raw_symbol which is unique per instrument:
    /// - Perps use base currency (e.g., "BTC")
    /// - Spot uses @{pair_index} format (e.g., "@107") or slash format for PURR
    pub fn cache_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        let mut map = AHashMap::new();

        for inst in instruments {
            let coin = inst.raw_symbol().inner();
            map.insert(coin, inst);
        }
        let count = map.len();
        self.instruments.store(map);
        log::info!("Hyperliquid instrument cache initialized with {count} instruments");
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same raw_symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let coin = instrument.raw_symbol().inner();
        self.instruments.insert(coin, instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::UpdateInstrument(instrument));
        }
    }

    /// Returns a shared reference to the instrument cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<AtomicMap<Ustr, InstrumentAny>> {
        self.instruments.clone()
    }

    /// Caches spot fill coin mappings for instrument lookup.
    ///
    /// Hyperliquid WebSocket fills for spot use `@{pair_index}` format (e.g., `@107`),
    /// while instruments are identified by full symbols (e.g., `HYPE-USDC-SPOT`).
    /// This mapping allows the handler to look up instruments from spot fills.
    pub fn cache_spot_fill_coins(&self, mapping: AHashMap<Ustr, Ustr>) {
        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::CacheSpotFillCoins(mapping));
        }
    }

    /// Caches a cloid (hex hash) to client_order_id mapping for order/fill resolution.
    ///
    /// The cloid is a keccak256 hash of the client_order_id that Hyperliquid uses internally.
    /// This mapping allows WebSocket order status and fill reports to be resolved back to
    /// the original client_order_id.
    ///
    /// This writes directly to a shared cache that the handler reads from, avoiding any
    /// race conditions between caching and WebSocket message processing.
    #[allow(
        clippy::missing_panics_doc,
        reason = "cloid cache mutex poisoning is not expected"
    )]
    pub fn cache_cloid_mapping(&self, cloid: Ustr, client_order_id: ClientOrderId) {
        log::debug!("Caching cloid mapping: {cloid} -> {client_order_id}");
        self.cloid_cache
            .lock()
            .expect(MUTEX_POISONED)
            .insert(cloid, client_order_id);
    }

    /// Removes a cloid mapping from the cache.
    ///
    /// Called on terminal order state. The cache is FIFO-bounded so missed
    /// removals self-evict (see GH-3972 cancel-replace drain).
    #[allow(
        clippy::missing_panics_doc,
        reason = "cloid cache mutex poisoning is not expected"
    )]
    pub fn remove_cloid_mapping(&self, cloid: &Ustr) {
        if self
            .cloid_cache
            .lock()
            .expect(MUTEX_POISONED)
            .remove(cloid)
            .is_some()
        {
            log::debug!("Removed cloid mapping: {cloid}");
        }
    }

    /// Clears all cloid mappings from the cache.
    ///
    /// Useful for cleanup during reconnection or shutdown.
    #[allow(
        clippy::missing_panics_doc,
        reason = "cloid cache mutex poisoning is not expected"
    )]
    pub fn clear_cloid_cache(&self) {
        let mut cache = self.cloid_cache.lock().expect(MUTEX_POISONED);
        let count = cache.len();
        cache.clear();

        if count > 0 {
            log::debug!("Cleared {count} cloid mappings from cache");
        }
    }

    /// Returns the number of cloid mappings in the cache.
    #[must_use]
    #[allow(
        clippy::missing_panics_doc,
        reason = "cloid cache mutex poisoning is not expected"
    )]
    pub fn cloid_cache_len(&self) -> usize {
        self.cloid_cache.lock().expect(MUTEX_POISONED).len()
    }

    /// Looks up a client_order_id by its cloid hash.
    ///
    /// Returns `Some(ClientOrderId)` if the mapping exists, `None` otherwise.
    #[must_use]
    #[allow(
        clippy::missing_panics_doc,
        reason = "cloid cache mutex poisoning is not expected"
    )]
    pub fn get_cloid_mapping(&self, cloid: &Ustr) -> Option<ClientOrderId> {
        self.cloid_cache
            .lock()
            .expect(MUTEX_POISONED)
            .get(cloid)
            .copied()
    }

    /// Gets an instrument from the cache by ID.
    ///
    /// Searches the cache for a matching instrument ID.
    pub fn get_instrument(&self, id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments
            .load()
            .values()
            .find(|inst| inst.id() == *id)
            .cloned()
    }

    /// Gets an instrument from the cache by raw_symbol (coin).
    pub fn get_instrument_by_symbol(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments.get_cloned(symbol)
    }

    /// Returns the count of confirmed subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Gets a bar type from the cache by coin and interval.
    ///
    /// This looks up the subscription key created when subscribing to bars.
    pub fn get_bar_type(&self, coin: &str, interval: &str) -> Option<BarType> {
        // Use canonical key format matching subscribe_bars
        let key = format!("candle:{coin}:{interval}");
        self.bar_types.load().get(&key).copied()
    }

    /// Subscribe to L2 order book for an instrument.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_book_with_options(instrument_id, None, None)
            .await
    }

    /// Subscribe to L2 order book with optional `nSigFigs` / `mantissa`
    /// precision controls passed through to the venue's `l2Book` stream.
    pub async fn subscribe_book_with_options(
        &self,
        instrument_id: InstrumentId,
        n_sig_figs: Option<u32>,
        mantissa: Option<u32>,
    ) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let cmd_tx = self.cmd_tx.read().await;

        // Update the handler's coin→instrument mapping for this subscription
        cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

        let subscription = SubscriptionRequest::L2Book {
            coin,
            mantissa,
            n_sig_figs,
        };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to order book depth-10 snapshots.
    ///
    /// Reuses the same `l2Book` WebSocket subscription as
    /// [`Self::subscribe_book`] and flags the handler to additionally emit
    /// `NautilusWsMessage::Depth10` for this coin.
    pub async fn subscribe_book_depth10(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_book_depth10_with_options(instrument_id, None, None)
            .await
    }

    /// Subscribe to depth-10 snapshots with optional `nSigFigs` /
    /// `mantissa` precision controls.
    pub async fn subscribe_book_depth10_with_options(
        &self,
        instrument_id: InstrumentId,
        n_sig_figs: Option<u32>,
        mantissa: Option<u32>,
    ) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let cmd_tx = self.cmd_tx.read().await;

        cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

        cmd_tx
            .send(HandlerCommand::SetDepth10Sub {
                coin,
                subscribed: true,
            })
            .map_err(|e| anyhow::anyhow!("Failed to send SetDepth10Sub command: {e}"))?;

        let subscription = SubscriptionRequest::L2Book {
            coin,
            mantissa,
            n_sig_figs,
        };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from order book depth-10 snapshots.
    ///
    /// Clears the depth10 emission flag only; the underlying `l2Book`
    /// stream stays open so active deltas subscribers keep receiving
    /// updates. Call [`Self::unsubscribe_book`] separately to tear down
    /// the stream entirely.
    pub async fn unsubscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SetDepth10Sub {
                coin,
                subscribed: false,
            })
            .map_err(|e| anyhow::anyhow!("Failed to send SetDepth10Sub command: {e}"))?;
        Ok(())
    }

    /// Subscribe to best bid/offer (BBO) quotes for an instrument.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let cmd_tx = self.cmd_tx.read().await;

        // Update the handler's coin→instrument mapping for this subscription
        cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

        let subscription = SubscriptionRequest::Bbo { coin };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to all mid prices across markets.
    pub async fn subscribe_all_mids(&self) -> anyhow::Result<()> {
        self.subscribe_all_mids_with_dex(None).await
    }

    /// Subscribe to all mid prices across markets, optionally scoped to a specific dex.
    pub async fn subscribe_all_mids_with_dex(&self, dex: Option<&str>) -> anyhow::Result<()> {
        let cmd_tx = self.cmd_tx.read().await;

        let subscription = SubscriptionRequest::AllMids {
            dex: dex.map(ToString::to_string),
        };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from all mid prices across markets.
    pub async fn unsubscribe_all_mids(&self) -> anyhow::Result<()> {
        self.unsubscribe_all_mids_with_dex(None).await
    }

    /// Unsubscribe from all mid prices across markets, optionally scoped to a specific dex.
    pub async fn unsubscribe_all_mids_with_dex(&self, dex: Option<&str>) -> anyhow::Result<()> {
        let cmd_tx = self.cmd_tx.read().await;

        let subscription = SubscriptionRequest::AllMids {
            dex: dex.map(ToString::to_string),
        };

        cmd_tx
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to trades for an instrument.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let cmd_tx = self.cmd_tx.read().await;

        // Update the handler's coin→instrument mapping for this subscription
        cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

        let subscription = SubscriptionRequest::Trades { coin };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to mark price updates for an instrument.
    pub async fn subscribe_mark_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::MarkPrice)
            .await
    }

    /// Subscribe to index/oracle price updates for an instrument.
    pub async fn subscribe_index_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::IndexPrice)
            .await
    }

    /// Subscribe to candle/bar data for a specific coin and interval.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> anyhow::Result<()> {
        // Get the instrument to extract the raw_symbol (Hyperliquid ticker)
        let instrument = self
            .get_instrument(&bar_type.instrument_id())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {}", bar_type.instrument_id()))?;
        let coin = instrument.raw_symbol().inner();
        let interval = bar_type_to_interval(&bar_type)?;
        let subscription = SubscriptionRequest::Candle { coin, interval };

        // Cache the bar type for parsing using canonical key
        let key = format!("candle:{coin}:{interval}");
        self.bar_types.insert(key.clone(), bar_type);

        let cmd_tx = self.cmd_tx.read().await;

        cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

        cmd_tx
            .send(HandlerCommand::AddBarType { key, bar_type })
            .map_err(|e| anyhow::anyhow!("Failed to send AddBarType command: {e}"))?;

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to funding rate updates for an instrument.
    pub async fn subscribe_funding_rates(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::FundingRate)
            .await
    }

    /// Subscribe to order updates for a specific user address.
    pub async fn subscribe_order_updates(&self, user: &str) -> anyhow::Result<()> {
        let subscription = SubscriptionRequest::OrderUpdates {
            user: user.to_string(),
        };
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to user events (fills, funding, liquidations) for a specific user address.
    pub async fn subscribe_user_events(&self, user: &str) -> anyhow::Result<()> {
        let subscription = SubscriptionRequest::UserEvents {
            user: user.to_string(),
        };
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to user fills for a specific user address.
    ///
    /// Note: This channel is redundant with `userEvents` which already includes fills.
    /// Prefer using `subscribe_user_events` or `subscribe_all_user_channels` instead.
    pub async fn subscribe_user_fills(&self, user: &str) -> anyhow::Result<()> {
        let subscription = SubscriptionRequest::UserFills {
            user: user.to_string(),
            aggregate_by_time: None,
        };
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        Ok(())
    }

    /// Subscribe to all user channels (order updates + user events) for convenience.
    ///
    /// Note: `userEvents` already includes fills, so we don't subscribe to `userFills`
    /// separately to avoid duplicate fill messages.
    pub async fn subscribe_all_user_channels(&self, user: &str) -> anyhow::Result<()> {
        self.subscribe_order_updates(user).await?;
        self.subscribe_user_events(user).await?;
        Ok(())
    }

    /// Unsubscribe from L2 order book for an instrument.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let subscription = SubscriptionRequest::L2Book {
            coin,
            mantissa: None,
            n_sig_figs: None,
        };

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from quote ticks for an instrument.
    pub async fn unsubscribe_quotes(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let subscription = SubscriptionRequest::Bbo { coin };

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from trades for an instrument.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let subscription = SubscriptionRequest::Trades { coin };

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from mark price updates for an instrument.
    pub async fn unsubscribe_mark_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.unsubscribe_asset_context_data(instrument_id, AssetContextDataType::MarkPrice)
            .await
    }

    /// Unsubscribe from index/oracle price updates for an instrument.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        self.unsubscribe_asset_context_data(instrument_id, AssetContextDataType::IndexPrice)
            .await
    }

    /// Unsubscribe from candle/bar data.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> anyhow::Result<()> {
        // Get the instrument to extract the raw_symbol (Hyperliquid ticker)
        let instrument = self
            .get_instrument(&bar_type.instrument_id())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {}", bar_type.instrument_id()))?;
        let coin = instrument.raw_symbol().inner();
        let interval = bar_type_to_interval(&bar_type)?;
        let subscription = SubscriptionRequest::Candle { coin, interval };

        let key = format!("candle:{coin}:{interval}");
        self.bar_types.remove(&key);

        let cmd_tx = self.cmd_tx.read().await;

        cmd_tx
            .send(HandlerCommand::RemoveBarType { key })
            .map_err(|e| anyhow::anyhow!("Failed to send RemoveBarType command: {e}"))?;

        cmd_tx
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
        Ok(())
    }

    /// Unsubscribe from funding rate updates for an instrument.
    pub async fn unsubscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        self.unsubscribe_asset_context_data(instrument_id, AssetContextDataType::FundingRate)
            .await
    }

    async fn subscribe_asset_context_data(
        &self,
        instrument_id: InstrumentId,
        data_type: AssetContextDataType,
    ) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        let mut entry = self.asset_context_subs.entry(coin).or_default();
        let is_first_subscription = entry.is_empty();
        entry.insert(data_type);
        let data_types = entry.clone();
        drop(entry);

        let cmd_tx = self.cmd_tx.read().await;

        cmd_tx
            .send(HandlerCommand::UpdateAssetContextSubs { coin, data_types })
            .map_err(|e| anyhow::anyhow!("Failed to send UpdateAssetContextSubs command: {e}"))?;

        if is_first_subscription {
            log::debug!(
                "First asset context subscription for coin '{coin}', subscribing to ActiveAssetCtx"
            );
            let subscription = SubscriptionRequest::ActiveAssetCtx { coin };

            cmd_tx
                .send(HandlerCommand::UpdateInstrument(instrument.clone()))
                .map_err(|e| anyhow::anyhow!("Failed to send UpdateInstrument command: {e}"))?;

            cmd_tx
                .send(HandlerCommand::Subscribe {
                    subscriptions: vec![subscription],
                })
                .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
        } else {
            log::debug!(
                "Already subscribed to ActiveAssetCtx for coin '{coin}', adding {data_type:?} to tracked types"
            );
        }

        Ok(())
    }

    async fn unsubscribe_asset_context_data(
        &self,
        instrument_id: InstrumentId,
        data_type: AssetContextDataType,
    ) -> anyhow::Result<()> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;
        let coin = instrument.raw_symbol().inner();

        if let Some(mut entry) = self.asset_context_subs.get_mut(&coin) {
            entry.remove(&data_type);
            let should_unsubscribe = entry.is_empty();
            let data_types = entry.clone();
            drop(entry);

            let cmd_tx = self.cmd_tx.read().await;

            if should_unsubscribe {
                self.asset_context_subs.remove(&coin);

                log::debug!(
                    "Last asset context subscription removed for coin '{coin}', unsubscribing from ActiveAssetCtx"
                );
                let subscription = SubscriptionRequest::ActiveAssetCtx { coin };

                cmd_tx
                    .send(HandlerCommand::UpdateAssetContextSubs {
                        coin,
                        data_types: AHashSet::new(),
                    })
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to send UpdateAssetContextSubs command: {e}")
                    })?;

                cmd_tx
                    .send(HandlerCommand::Unsubscribe {
                        subscriptions: vec![subscription],
                    })
                    .map_err(|e| anyhow::anyhow!("Failed to send unsubscribe command: {e}"))?;
            } else {
                log::debug!(
                    "Removed {data_type:?} from tracked types for coin '{coin}', but keeping ActiveAssetCtx subscription"
                );

                cmd_tx
                    .send(HandlerCommand::UpdateAssetContextSubs { coin, data_types })
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to send UpdateAssetContextSubs command: {e}")
                    })?;
            }
        }

        Ok(())
    }

    /// Receives the next message from the WebSocket handler.
    ///
    /// Returns `None` if the handler has disconnected or the receiver was already taken.
    pub async fn next_event(&mut self) -> Option<NautilusWsMessage> {
        if let Some(ref mut rx) = self.out_rx {
            rx.recv().await
        } else {
            None
        }
    }
}

fn cancel_errors_for_requests(
    errors: Vec<Option<String>>,
    request_count: usize,
) -> HyperliquidResult<Vec<Option<String>>> {
    if errors.is_empty() {
        return Ok(vec![None; request_count]);
    }

    if errors.len() != request_count {
        return Err(HyperliquidError::exchange(format!(
            "Cancel orders returned {} statuses for {request_count} cancels",
            errors.len()
        )));
    }

    Ok(errors)
}

fn map_post_payload_error(payload: String, weight: u32) -> HyperliquidError {
    let lower = payload.to_ascii_lowercase();
    let message = format!("WebSocket post error: {payload}");

    if starts_with_status(&lower, &["429"])
        || lower.contains("too many requests")
        || lower.contains("rate limit")
    {
        HyperliquidError::rate_limit("exchange", weight, None)
    } else if starts_with_status(&lower, &["401", "403"])
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("authentication")
        || lower.contains("authorization")
        || lower.contains("invalid signature")
        || contains_word(&lower, "auth")
    {
        HyperliquidError::auth(message)
    } else if starts_with_status(&lower, &["400"]) || lower.contains("bad request") {
        HyperliquidError::bad_request(message)
    } else if starts_with_status(&lower, &["500", "502", "503", "504"]) {
        HyperliquidError::exchange(message)
    } else {
        HyperliquidError::exchange(payload)
    }
}

fn hyperliquid_order_kind(
    order_type: OrderType,
    time_in_force: TimeInForce,
    post_only: bool,
    trigger_price: Option<Price>,
    normalize_prices_enabled: bool,
    price_precision: u8,
) -> HyperliquidResult<HyperliquidExecOrderKind> {
    match order_type {
        OrderType::Market => Ok(HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Ioc,
            },
        }),
        OrderType::Limit => {
            let tif = time_in_force_to_hyperliquid_tif(time_in_force, post_only)
                .map_err(|e| HyperliquidError::bad_request(format!("{e}")))?;
            Ok(HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams { tif },
            })
        }
        OrderType::StopMarket
        | OrderType::StopLimit
        | OrderType::MarketIfTouched
        | OrderType::LimitIfTouched => {
            let trigger_price = trigger_price.ok_or_else(|| {
                HyperliquidError::bad_request("Trigger orders require a trigger price")
            })?;
            let trigger_px = if normalize_prices_enabled {
                normalize_price(trigger_price.as_decimal(), price_precision).normalize()
            } else {
                trigger_price.as_decimal().normalize()
            };
            let tpsl = match order_type {
                OrderType::StopMarket | OrderType::StopLimit => HyperliquidExecTpSl::Sl,
                OrderType::MarketIfTouched | OrderType::LimitIfTouched => HyperliquidExecTpSl::Tp,
                _ => unreachable!(),
            };
            let is_market = matches!(
                order_type,
                OrderType::StopMarket | OrderType::MarketIfTouched
            );

            Ok(HyperliquidExecOrderKind::Trigger {
                trigger: HyperliquidExecTriggerParams {
                    is_market,
                    trigger_px,
                    tpsl,
                },
            })
        }
        _ => Err(HyperliquidError::bad_request(format!(
            "Order type {order_type:?} not supported"
        ))),
    }
}

fn ensure_ws_action_accepted(
    response: &HyperliquidExchangeResponse,
    action_name: &str,
) -> HyperliquidResult<()> {
    if response.is_ok() {
        if let Some(error_msg) = extract_inner_errors(response).into_iter().flatten().next() {
            return Err(HyperliquidError::bad_request(format!(
                "{action_name} rejected: {error_msg}"
            )));
        }

        if let Some(error_msg) = extract_inner_error(response) {
            return Err(HyperliquidError::bad_request(format!(
                "{action_name} rejected: {error_msg}"
            )));
        }

        return Ok(());
    }

    Err(HyperliquidError::bad_request(format!(
        "{action_name} failed: {}",
        extract_error_message(response)
    )))
}

fn starts_with_status(payload: &str, statuses: &[&str]) -> bool {
    let trimmed = payload.trim_start();
    statuses
        .iter()
        .any(|status| starts_with_status_token(trimmed, status))
        || trimmed.strip_prefix("http").is_some_and(|rest| {
            let rest = rest
                .trim_start_matches(|c: char| c.is_ascii_whitespace() || matches!(c, ':' | '/'));
            statuses
                .iter()
                .any(|status| starts_with_status_token(rest, status))
        })
}

fn starts_with_status_token(payload: &str, status: &str) -> bool {
    payload.strip_prefix(status).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    })
}

fn contains_word(payload: &str, word: &str) -> bool {
    payload
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == word)
}

// Uses split_once/rsplit_once because coin names can contain colons
// (e.g., vault tokens `vntls:vCURSOR`)
fn subscription_from_topic(topic: &str) -> anyhow::Result<SubscriptionRequest> {
    let (kind, rest) = topic
        .split_once(':')
        .map_or((topic, None), |(k, r)| (k, Some(r)));

    let channel = HyperliquidWsChannel::from_wire_str(kind)
        .ok_or_else(|| anyhow::anyhow!("Unknown subscription channel: {kind}"))?;

    match channel {
        HyperliquidWsChannel::AllMids => Ok(SubscriptionRequest::AllMids {
            dex: rest.map(|s| s.to_string()),
        }),
        HyperliquidWsChannel::Notification => Ok(SubscriptionRequest::Notification {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::WebData2 => Ok(SubscriptionRequest::WebData2 {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::Candle => {
            // Format: candle:{coin}:{interval} - interval is last segment
            let rest = rest.context("Missing candle params")?;
            let (coin, interval_str) = rest.rsplit_once(':').context("Missing interval")?;
            let interval = HyperliquidBarInterval::from_str(interval_str)?;
            Ok(SubscriptionRequest::Candle {
                coin: Ustr::from(coin),
                interval,
            })
        }
        HyperliquidWsChannel::L2Book => Ok(SubscriptionRequest::L2Book {
            coin: Ustr::from(rest.context("Missing coin")?),
            mantissa: None,
            n_sig_figs: None,
        }),
        HyperliquidWsChannel::Trades => Ok(SubscriptionRequest::Trades {
            coin: Ustr::from(rest.context("Missing coin")?),
        }),
        HyperliquidWsChannel::OrderUpdates => Ok(SubscriptionRequest::OrderUpdates {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::UserEvents => Ok(SubscriptionRequest::UserEvents {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::UserFills => Ok(SubscriptionRequest::UserFills {
            user: rest.context("Missing user")?.to_string(),
            aggregate_by_time: None,
        }),
        HyperliquidWsChannel::UserFundings => Ok(SubscriptionRequest::UserFundings {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::UserNonFundingLedgerUpdates => {
            Ok(SubscriptionRequest::UserNonFundingLedgerUpdates {
                user: rest.context("Missing user")?.to_string(),
            })
        }
        HyperliquidWsChannel::ActiveAssetCtx => Ok(SubscriptionRequest::ActiveAssetCtx {
            coin: Ustr::from(rest.context("Missing coin")?),
        }),
        HyperliquidWsChannel::ActiveSpotAssetCtx => Ok(SubscriptionRequest::ActiveSpotAssetCtx {
            coin: Ustr::from(rest.context("Missing coin")?),
        }),
        HyperliquidWsChannel::ActiveAssetData => {
            // Format: activeAssetData:{user}:{coin} - user is eth addr (no colons)
            let rest = rest.context("Missing params")?;
            let (user, coin) = rest.split_once(':').context("Missing coin")?;
            Ok(SubscriptionRequest::ActiveAssetData {
                user: user.to_string(),
                coin: coin.to_string(),
            })
        }
        HyperliquidWsChannel::UserTwapSliceFills => Ok(SubscriptionRequest::UserTwapSliceFills {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::UserTwapHistory => Ok(SubscriptionRequest::UserTwapHistory {
            user: rest.context("Missing user")?.to_string(),
        }),
        HyperliquidWsChannel::Bbo => Ok(SubscriptionRequest::Bbo {
            coin: Ustr::from(rest.context("Missing coin")?),
        }),

        // Response-only channels are not valid subscription topics
        HyperliquidWsChannel::SubscriptionResponse
        | HyperliquidWsChannel::User
        | HyperliquidWsChannel::Post
        | HyperliquidWsChannel::Pong
        | HyperliquidWsChannel::Error => {
            anyhow::bail!("Not a subscription channel: {kind}")
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{consts::INFLIGHT_MAX, enums::HyperliquidBarInterval},
        websocket::handler::subscription_to_key,
    };

    /// Generates a unique topic key for a subscription request.
    fn subscription_topic(sub: &SubscriptionRequest) -> String {
        subscription_to_key(sub)
    }

    #[rstest]
    #[case(SubscriptionRequest::Trades { coin: "BTC".into() }, "trades:BTC")]
    #[case(SubscriptionRequest::Bbo { coin: "BTC".into() }, "bbo:BTC")]
    #[case(SubscriptionRequest::OrderUpdates { user: "0x123".to_string() }, "orderUpdates:0x123")]
    #[case(SubscriptionRequest::UserEvents { user: "0xabc".to_string() }, "userEvents:0xabc")]
    fn test_subscription_topic_generation(
        #[case] subscription: SubscriptionRequest,
        #[case] expected_topic: &str,
    ) {
        assert_eq!(subscription_topic(&subscription), expected_topic);
    }

    #[rstest]
    fn test_subscription_topics_unique() {
        let sub1 = SubscriptionRequest::Trades { coin: "BTC".into() };
        let sub2 = SubscriptionRequest::Bbo { coin: "BTC".into() };

        let topic1 = subscription_topic(&sub1);
        let topic2 = subscription_topic(&sub2);

        assert_ne!(topic1, topic2);
    }

    #[rstest]
    #[case(SubscriptionRequest::Trades { coin: "BTC".into() })]
    #[case(SubscriptionRequest::Bbo { coin: "ETH".into() })]
    #[case(SubscriptionRequest::Candle { coin: "SOL".into(), interval: HyperliquidBarInterval::OneHour })]
    #[case(SubscriptionRequest::OrderUpdates { user: "0x123".to_string() })]
    #[case(SubscriptionRequest::Trades { coin: "vntls:vCURSOR".into() })]
    #[case(SubscriptionRequest::L2Book { coin: "vntls:vCURSOR".into(), mantissa: None, n_sig_figs: None })]
    #[case(SubscriptionRequest::Candle { coin: "vntls:vCURSOR".into(), interval: HyperliquidBarInterval::OneHour })]
    fn test_subscription_reconstruction(#[case] subscription: SubscriptionRequest) {
        let topic = subscription_topic(&subscription);
        let reconstructed = subscription_from_topic(&topic).expect("Failed to reconstruct");
        assert_eq!(subscription_topic(&reconstructed), topic);
    }

    #[rstest]
    fn test_subscription_topic_candle() {
        let sub = SubscriptionRequest::Candle {
            coin: "BTC".into(),
            interval: HyperliquidBarInterval::OneHour,
        };

        let topic = subscription_topic(&sub);
        assert_eq!(topic, "candle:BTC:1h");
    }

    #[rstest]
    fn set_post_timeout_updates_client_and_clone() {
        let mut client = HyperliquidWebSocketClient::new(
            None,
            HyperliquidEnvironment::Testnet,
            None,
            TransportBackend::default(),
            None,
        );
        let timeout = std::time::Duration::from_secs(7);

        client.set_post_timeout(timeout);

        assert_eq!(client.post_timeout, timeout);
        assert_eq!(client.clone().post_timeout, timeout);
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn send_post_request_times_out_while_waiting_for_inflight_slot() {
        let client = HyperliquidWebSocketClient::new(
            None,
            HyperliquidEnvironment::Testnet,
            None,
            TransportBackend::default(),
            None,
        );
        let mut receivers = Vec::with_capacity(INFLIGHT_MAX);
        for offset in 0..INFLIGHT_MAX {
            receivers.push(
                client
                    .post_router
                    .register(10_000 + offset as u64)
                    .await
                    .unwrap(),
            );
        }

        let err = client
            .send_post_request(
                PostRequest::Info {
                    payload: serde_json::json!({"type": "clearinghouseState", "user": "0x0"}),
                },
                std::time::Duration::from_millis(25),
            )
            .await
            .expect_err("request should timeout before acquiring an inflight slot");

        assert!(matches!(err, HyperliquidError::Timeout));
        assert_eq!(receivers.len(), INFLIGHT_MAX);
    }

    #[rstest]
    fn cancel_errors_for_requests_accepts_empty_as_success() {
        let errors = cancel_errors_for_requests(Vec::new(), 2).unwrap();

        assert_eq!(errors, vec![None, None]);
    }

    #[rstest]
    fn cancel_errors_for_requests_rejects_status_count_mismatch() {
        let err = cancel_errors_for_requests(vec![None], 2).expect_err("mismatch should fail");

        assert!(
            err.to_string()
                .contains("returned 1 statuses for 2 cancels")
        );
    }

    #[rstest]
    fn test_post_payload_error_maps_rate_limit() {
        let err = map_post_payload_error("429 Too Many Requests".to_string(), 3);

        assert!(matches!(
            err,
            HyperliquidError::RateLimit {
                scope: "exchange",
                weight: 3,
                retry_after_ms: None,
            }
        ));
    }

    #[rstest]
    #[case("401 Unauthorized")]
    #[case("HTTP 403: forbidden")]
    #[case("invalid signature")]
    #[case("authentication failed")]
    fn test_post_payload_error_maps_auth(#[case] payload: &str) {
        let err = map_post_payload_error(payload.to_string(), 1);

        assert!(matches!(err, HyperliquidError::Auth(_)));
    }

    #[rstest]
    #[case("400 Bad Request")]
    #[case("HTTP 400: malformed payload")]
    #[case("bad request: missing action")]
    fn test_post_payload_error_maps_bad_request(#[case] payload: &str) {
        let err = map_post_payload_error(payload.to_string(), 1);

        assert!(matches!(err, HyperliquidError::BadRequest(_)));
    }

    #[rstest]
    #[case("500 Internal Server Error")]
    #[case("HTTP 503: service unavailable")]
    fn test_post_payload_error_maps_exchange_status(#[case] payload: &str) {
        let err = map_post_payload_error(payload.to_string(), 1);

        assert!(matches!(err, HyperliquidError::Exchange(_)));
    }

    #[rstest]
    #[case("order 429001 rejected")]
    #[case("asset 5001 is not tradable")]
    #[case("authoritative nonce window exceeded")]
    fn test_post_payload_error_does_not_match_embedded_codes_or_words(#[case] payload: &str) {
        let err = map_post_payload_error(payload.to_string(), 1);

        assert!(matches!(err, HyperliquidError::Exchange(_)));
    }
}

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
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use ustr::Ustr;

use crate::{
    common::{
        consts::ws_url,
        enums::{HyperliquidBarInterval, HyperliquidEnvironment},
        parse::bar_type_to_interval,
    },
    websocket::{
        enums::HyperliquidWsChannel,
        handler::{FeedHandler, HandlerCommand},
        messages::{NautilusWsMessage, SubscriptionRequest},
    },
};

const HYPERLIQUID_HEARTBEAT_MSG: &str = r#"{"method":"ping"}"#;

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
    cloid_cache: Arc<DashMap<Ustr, ClientOrderId>>,
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
            cloid_cache: Arc::new(DashMap::new()),
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

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal,
                cmd_rx,
                raw_rx,
                out_tx,
                account_id,
                subscriptions.clone(),
                cloid_cache,
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
    pub fn cache_cloid_mapping(&self, cloid: Ustr, client_order_id: ClientOrderId) {
        log::debug!("Caching cloid mapping: {cloid} -> {client_order_id}");
        self.cloid_cache.insert(cloid, client_order_id);
    }

    /// Removes a cloid mapping from the cache.
    ///
    /// Should be called when an order reaches a terminal state (filled, canceled, expired)
    /// to prevent unbounded memory growth in long-running sessions.
    pub fn remove_cloid_mapping(&self, cloid: &Ustr) {
        if self.cloid_cache.remove(cloid).is_some() {
            log::debug!("Removed cloid mapping: {cloid}");
        }
    }

    /// Clears all cloid mappings from the cache.
    ///
    /// Useful for cleanup during reconnection or shutdown.
    pub fn clear_cloid_cache(&self) {
        let count = self.cloid_cache.len();
        self.cloid_cache.clear();

        if count > 0 {
            log::debug!("Cleared {count} cloid mappings from cache");
        }
    }

    /// Returns the number of cloid mappings in the cache.
    #[must_use]
    pub fn cloid_cache_len(&self) -> usize {
        self.cloid_cache.len()
    }

    /// Looks up a client_order_id by its cloid hash.
    ///
    /// Returns `Some(ClientOrderId)` if the mapping exists, `None` otherwise.
    #[must_use]
    pub fn get_cloid_mapping(&self, cloid: &Ustr) -> Option<ClientOrderId> {
        self.cloid_cache.get(cloid).map(|entry| *entry.value())
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
    use crate::{common::enums::HyperliquidBarInterval, websocket::handler::subscription_to_key};

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
}

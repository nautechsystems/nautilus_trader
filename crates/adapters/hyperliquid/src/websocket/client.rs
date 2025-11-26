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

use std::{
    collections::HashSet,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use anyhow::Context;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use ustr::Ustr;

use crate::{
    common::{HyperliquidProductType, enums::HyperliquidBarInterval, parse::bar_type_to_interval},
    websocket::{
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidWebSocketClient {
    url: String,
    product_type: HyperliquidProductType,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    instruments: Arc<DashMap<Ustr, InstrumentAny>>,
    bar_types: Arc<DashMap<String, BarType>>,
    asset_context_subs: Arc<DashMap<Ustr, HashSet<AssetContextDataType>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    account_id: Option<AccountId>,
}

impl Clone for HyperliquidWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            product_type: self.product_type,
            connection_mode: Arc::clone(&self.connection_mode),
            signal: Arc::clone(&self.signal),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            auth_tracker: self.auth_tracker.clone(),
            subscriptions: self.subscriptions.clone(),
            instruments: Arc::clone(&self.instruments),
            bar_types: Arc::clone(&self.bar_types),
            asset_context_subs: Arc::clone(&self.asset_context_subs),
            task_handle: None,
            account_id: self.account_id,
        }
    }
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client without connecting.
    ///
    /// If `url` is `None`, the appropriate URL will be determined based on the `testnet` flag:
    /// - `testnet=false`: `wss://api.hyperliquid.xyz/ws`
    /// - `testnet=true`: `wss://api.hyperliquid-testnet.xyz/ws`
    ///
    /// The connection will be established when `connect()` is called.
    pub fn new(
        url: Option<String>,
        testnet: bool,
        product_type: HyperliquidProductType,
        account_id: Option<AccountId>,
    ) -> Self {
        let url = url.unwrap_or_else(|| {
            if testnet {
                "wss://api.hyperliquid-testnet.xyz/ws".to_string()
            } else {
                "wss://api.hyperliquid.xyz/ws".to_string()
            }
        });
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));
        Self {
            url,
            product_type,
            connection_mode,
            signal: Arc::new(AtomicBool::new(false)),
            auth_tracker: AuthTracker::new(),
            subscriptions: SubscriptionState::new(':'),
            instruments: Arc::new(DashMap::new()),
            bar_types: Arc::new(DashMap::new()),
            asset_context_subs: Arc::new(DashMap::new()),
            cmd_tx: {
                // Placeholder channel until connect() creates the real handler and replays queued instruments
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                Arc::new(tokio::sync::RwLock::new(tx))
            },
            out_rx: None,
            task_handle: None,
            account_id,
        }
    }

    /// Establishes WebSocket connection and spawns the message handler.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_active() {
            tracing::warn!("WebSocket already connected");
            return Ok(());
        }
        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            message_handler: Some(message_handler),
            heartbeat: Some(30),
            heartbeat_msg: Some(HYPERLIQUID_HEARTBEAT_MSG.to_string()),
            ping_handler: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
        };
        let client = WebSocketClient::connect(cfg, None, vec![], None).await?;

        // Atomically swap connection state to the client's atomic
        self.connection_mode.store(client.connection_mode_atomic());
        tracing::info!("Hyperliquid WebSocket connected: {}", self.url);

        // Create channels for handler communication
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        // Send SetClient command immediately
        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            anyhow::bail!("Failed to send SetClient command: {e}");
        }

        // Initialize handler with existing instruments
        let instruments_vec: Vec<InstrumentAny> = self
            .instruments
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        if !instruments_vec.is_empty()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments_vec))
        {
            tracing::error!("Failed to send InitializeInstruments: {e}");
        }

        // Spawn handler task
        let signal = Arc::clone(&self.signal);
        let account_id = self.account_id;
        let subscriptions = self.subscriptions.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let stream_handle = tokio::spawn(async move {
            let mut handler = FeedHandler::new(
                signal,
                cmd_rx,
                raw_rx,
                out_tx,
                account_id,
                subscriptions.clone(),
            );

            let resubscribe_all = || {
                let topics = subscriptions.all_topics();
                if topics.is_empty() {
                    tracing::debug!("No active subscriptions to restore after reconnection");
                    return;
                }

                tracing::info!(
                    "Resubscribing to {} active subscriptions after reconnection",
                    topics.len()
                );
                for topic in topics {
                    match subscription_from_topic(&topic) {
                        Ok(subscription) => {
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe {
                                subscriptions: vec![subscription],
                            }) {
                                tracing::error!(error = %e, "Failed to send resubscribe command");
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                topic = %topic,
                                "Failed to reconstruct subscription from topic"
                            );
                        }
                    }
                }
            };
            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        tracing::info!("WebSocket reconnected");
                        resubscribe_all();
                        continue;
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
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
            tracing::debug!("Handler task completed");
        });
        self.task_handle = Some(Arc::new(stream_handle));
        *self.cmd_tx.write().await = cmd_tx;
        self.out_rx = Some(out_rx);
        Ok(())
    }

    /// Disconnects the WebSocket connection.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        tracing::info!("Disconnecting Hyperliquid WebSocket");
        self.signal.store(true, Ordering::Relaxed);
        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
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
        tracing::debug!("Disconnected");
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

    /// Subscribe to all user channels (order updates + user events) for convenience.
    pub async fn subscribe_all_user_channels(&self, user: &str) -> anyhow::Result<()> {
        self.subscribe_order_updates(user).await?;
        self.subscribe_user_events(user).await?;
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

    /// Subscribe to L2 order book for an instrument.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
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
            mantissa: None,
            n_sig_figs: None,
        };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription],
            })
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe command: {e}"))?;
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

    /// Subscribe to mark price updates for an instrument.
    pub async fn subscribe_mark_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::MarkPrice)
            .await
    }

    /// Unsubscribe from mark price updates for an instrument.
    pub async fn unsubscribe_mark_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.unsubscribe_asset_context_data(instrument_id, AssetContextDataType::MarkPrice)
            .await
    }

    /// Subscribe to index/oracle price updates for an instrument.
    pub async fn subscribe_index_prices(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::IndexPrice)
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

    /// Subscribe to funding rate updates for an instrument.
    pub async fn subscribe_funding_rates(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_asset_context_data(instrument_id, AssetContextDataType::FundingRate)
            .await
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
            tracing::debug!(
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
            tracing::debug!(
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

                tracing::debug!(
                    "Last asset context subscription removed for coin '{coin}', unsubscribing from ActiveAssetCtx"
                );
                let subscription = SubscriptionRequest::ActiveAssetCtx { coin };

                cmd_tx
                    .send(HandlerCommand::UpdateAssetContextSubs {
                        coin,
                        data_types: HashSet::new(),
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
                tracing::debug!(
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

    /// Caches multiple instruments.
    ///
    /// Clears the existing cache first, then adds all provided instruments.
    /// Instruments are keyed by their full Nautilus symbol (e.g., "BTC-USD-PERP").
    pub fn cache_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        self.instruments.clear();
        for inst in instruments {
            let symbol = inst.symbol().inner();
            self.instruments.insert(symbol, inst.clone());
        }
        tracing::info!(
            "Hyperliquid instrument cache initialized with {} instruments",
            self.instruments.len()
        );
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.symbol().inner();
        self.instruments.insert(symbol, instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::UpdateInstrument(instrument));
        }
    }

    /// Gets an instrument from the cache by ID.
    ///
    /// Looks up the instrument by its full Nautilus symbol (e.g., "BTC-USD-PERP").
    pub fn get_instrument(&self, id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments
            .get(&id.symbol.inner())
            .map(|e| e.value().clone())
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_instrument_by_symbol(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments.get(symbol).map(|e| e.value().clone())
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
        self.bar_types.get(&key).map(|entry| *entry.value())
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

/// Reconstructs a subscription request from a topic string.
fn subscription_from_topic(topic: &str) -> anyhow::Result<SubscriptionRequest> {
    let parts: Vec<&str> = topic.split(':').collect();

    match parts.first() {
        Some(&"allMids") => {
            let dex = parts.get(1).map(|s| (*s).to_string());
            Ok(SubscriptionRequest::AllMids { dex })
        }
        Some(&"notification") => Ok(SubscriptionRequest::Notification {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"webData2") => Ok(SubscriptionRequest::WebData2 {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"candle") => {
            let coin = Ustr::from(parts.get(1).context("Missing coin")?);
            let interval_str = parts.get(2).context("Missing interval")?;
            let interval = HyperliquidBarInterval::from_str(interval_str)?;
            Ok(SubscriptionRequest::Candle { coin, interval })
        }
        Some(&"l2Book") => Ok(SubscriptionRequest::L2Book {
            coin: Ustr::from(parts.get(1).context("Missing coin")?),
            mantissa: None,
            n_sig_figs: None,
        }),
        Some(&"trades") => Ok(SubscriptionRequest::Trades {
            coin: Ustr::from(parts.get(1).context("Missing coin")?),
        }),
        Some(&"orderUpdates") => Ok(SubscriptionRequest::OrderUpdates {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"userEvents") => Ok(SubscriptionRequest::UserEvents {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"userFills") => Ok(SubscriptionRequest::UserFills {
            user: (*parts.get(1).context("Missing user")?).to_string(),
            aggregate_by_time: None,
        }),
        Some(&"userFundings") => Ok(SubscriptionRequest::UserFundings {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"userNonFundingLedgerUpdates") => {
            Ok(SubscriptionRequest::UserNonFundingLedgerUpdates {
                user: (*parts.get(1).context("Missing user")?).to_string(),
            })
        }
        Some(&"activeAssetCtx") => Ok(SubscriptionRequest::ActiveAssetCtx {
            coin: Ustr::from(parts.get(1).context("Missing coin")?),
        }),
        Some(&"activeSpotAssetCtx") => Ok(SubscriptionRequest::ActiveSpotAssetCtx {
            coin: Ustr::from(parts.get(1).context("Missing coin")?),
        }),
        Some(&"activeAssetData") => Ok(SubscriptionRequest::ActiveAssetData {
            user: (*parts.get(1).context("Missing user")?).to_string(),
            coin: (*parts.get(2).context("Missing coin")?).to_string(),
        }),
        Some(&"userTwapSliceFills") => Ok(SubscriptionRequest::UserTwapSliceFills {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"userTwapHistory") => Ok(SubscriptionRequest::UserTwapHistory {
            user: (*parts.get(1).context("Missing user")?).to_string(),
        }),
        Some(&"bbo") => Ok(SubscriptionRequest::Bbo {
            coin: Ustr::from(parts.get(1).context("Missing coin")?),
        }),
        Some(channel) => anyhow::bail!("Unknown subscription channel: {channel}"),
        None => anyhow::bail!("Empty topic string"),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::HyperliquidBarInterval;

    /// Generates a unique topic key for a subscription request.
    fn subscription_topic(sub: &SubscriptionRequest) -> String {
        match sub {
            SubscriptionRequest::AllMids { dex } => {
                if let Some(dex) = dex {
                    format!("allMids:{dex}")
                } else {
                    "allMids".to_string()
                }
            }
            SubscriptionRequest::Notification { user } => format!("notification:{user}"),
            SubscriptionRequest::WebData2 { user } => format!("webData2:{user}"),
            SubscriptionRequest::Candle { coin, interval } => {
                format!("candle:{coin}:{}", interval.as_str())
            }
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
            SubscriptionRequest::ActiveSpotAssetCtx { coin } => {
                format!("activeSpotAssetCtx:{coin}")
            }
            SubscriptionRequest::ActiveAssetData { user, coin } => {
                format!("activeAssetData:{user}:{coin}")
            }
            SubscriptionRequest::UserTwapSliceFills { user } => {
                format!("userTwapSliceFills:{user}")
            }
            SubscriptionRequest::UserTwapHistory { user } => format!("userTwapHistory:{user}"),
            SubscriptionRequest::Bbo { coin } => format!("bbo:{coin}"),
        }
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

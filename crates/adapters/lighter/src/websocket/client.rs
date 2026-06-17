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

//! Outer WebSocket client orchestrating connection lifecycle and subscriptions.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};

use crate::{
    common::{
        consts::{HEARTBEAT_INTERVAL, RECONNECT_BASE_BACKOFF, RECONNECT_MAX_BACKOFF},
        enums::{LighterCandleResolution, LighterEnvironment},
        rate_limit::ws_message_rate_limiter,
        symbol::MarketRegistry,
        urls::lighter_ws_url,
    },
    websocket::{
        error::LighterWsError,
        handler::{FeedHandler, HandlerCommand},
        messages::{LighterMarketSelection, LighterWsChannel, NautilusWsMessage},
    },
};

const RECONNECT_TIMEOUT_MS: u64 = 15_000;
const RECONNECT_JITTER_MS: u64 = 200;
const RECONNECT_BACKOFF_FACTOR: f64 = 2.0;
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Outer Lighter WebSocket client.
///
/// Orchestrates the connection lifecycle and subscription bookkeeping for the
/// Lighter streaming API. The inner feed handler runs on a dedicated tokio
/// task and exclusively owns the underlying [`WebSocketClient`]; this outer
/// type communicates with it through a command channel and consumes events
/// over an unbounded mpsc.
pub struct LighterWebSocketClient {
    url: String,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    subscriptions: SubscriptionState,
    subscription_args: Arc<DashMap<String, (LighterWsChannel, Option<String>)>>,
    instruments: Arc<DashMap<i16, InstrumentAny>>,
    registry: Arc<MarketRegistry>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
}

impl Debug for LighterWebSocketClient {
    /// Custom `Debug` that redacts the auth token in `subscription_args`.
    ///
    /// Authenticated channel subscriptions store the venue bearer token
    /// alongside the channel for reconnect replay; deriving `Debug` would
    /// otherwise print the token verbatim.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let subscription_topics: Vec<String> = self
            .subscription_args
            .iter()
            .map(|entry| {
                let (channel, auth) = entry.value();
                format!(
                    "topic={} channel={:?} authed={}",
                    entry.key(),
                    channel,
                    auth.is_some(),
                )
            })
            .collect();

        f.debug_struct(stringify!(LighterWebSocketClient))
            .field("url", &self.url)
            .field("is_active", &self.is_active())
            .field("subscription_count", &self.subscriptions.len())
            .field("subscription_args", &subscription_topics)
            .field("instruments_len", &self.instruments.len())
            .field("transport_backend", &self.transport_backend)
            .field("proxy_url", &self.proxy_url)
            .finish_non_exhaustive()
    }
}

impl Clone for LighterWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            connection_mode: Arc::clone(&self.connection_mode),
            signal: Arc::clone(&self.signal),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            subscriptions: self.subscriptions.clone(),
            subscription_args: Arc::clone(&self.subscription_args),
            instruments: Arc::clone(&self.instruments),
            registry: Arc::clone(&self.registry),
            task_handle: None,
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl LighterWebSocketClient {
    /// Creates a new client without connecting.
    ///
    /// `url` overrides the resolved environment URL when supplied.
    #[must_use]
    pub fn new(
        url: Option<String>,
        environment: LighterEnvironment,
        registry: Arc<MarketRegistry>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let url = url.unwrap_or_else(|| lighter_ws_url(environment).to_string());
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));

        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();

        Self {
            url,
            connection_mode,
            signal: Arc::new(AtomicBool::new(false)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            subscriptions: SubscriptionState::new(':'),
            subscription_args: Arc::new(DashMap::new()),
            instruments: Arc::new(DashMap::new()),
            registry,
            task_handle: None,
            transport_backend,
            proxy_url,
        }
    }

    /// Returns the resolved WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns `true` when the underlying connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode.load().load(Ordering::Relaxed) == ConnectionMode::Active as u8
    }

    /// Waits until the underlying connection reports active, or returns an
    /// error after `timeout_secs`.
    ///
    /// Polls [`Self::is_active`] every 10ms. Mirrors the documented
    /// `wait_until_active` contract for adapter WebSocket clients in
    /// `docs/developer_guide/adapters.md`.
    ///
    /// # Errors
    ///
    /// Returns [`LighterWsError::Client`] if the connection does not reach
    /// the active state within `timeout_secs`.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), LighterWsError> {
        let timeout = Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            LighterWsError::Client(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })
    }

    /// Returns the count of confirmed subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns a clone of the shared instrument cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<DashMap<i16, InstrumentAny>> {
        Arc::clone(&self.instruments)
    }

    /// Caches a batch of instruments along with their venue `market_index`,
    /// replaying them to the handler if a connection is already established.
    pub fn cache_instruments(&self, instruments: Vec<(i16, InstrumentAny)>) {
        self.instruments.clear();
        for (market_index, instrument) in &instruments {
            self.instruments.insert(*market_index, instrument.clone());
        }
        log::debug!(
            "Lighter instrument cache initialized with {} instruments",
            instruments.len()
        );

        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments));
        }
    }

    /// Caches a single instrument and pushes it to the handler if connected.
    pub fn cache_instrument(&self, market_index: i16, instrument: InstrumentAny) {
        self.instruments.insert(market_index, instrument.clone());

        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::UpdateInstrument {
                market_index,
                instrument,
            });
        }
    }

    /// Establishes the WebSocket connection and spawns the feed-handler task.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`WebSocketClient::connect`] fails
    /// or the handler cannot be initialized.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_active() {
            log::warn!("Lighter WebSocket already connected");
            return Ok(());
        }

        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(HEARTBEAT_INTERVAL.as_secs()),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(RECONNECT_TIMEOUT_MS),
            reconnect_delay_initial_ms: Some(RECONNECT_BASE_BACKOFF.as_millis() as u64),
            reconnect_delay_max_ms: Some(RECONNECT_MAX_BACKOFF.as_millis() as u64),
            reconnect_backoff_factor: Some(RECONNECT_BACKOFF_FACTOR),
            reconnect_jitter_ms: Some(RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };
        let client = WebSocketClient::connect_with_rate_limiter(
            cfg,
            Some(message_handler),
            None,
            None,
            ws_message_rate_limiter(&self.url),
        )
        .await?;

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        // Capture the connection-mode atomic before moving `client` into the
        // SetClient command below.
        let connection_mode_atomic = client.connection_mode_atomic();

        // Queue SetClient (and the instrument cache replay) onto the new
        // command channel BEFORE publishing it to clones or marking the
        // connection active. Otherwise a clone observing `is_active()` could
        // race in and send a Subscribe before SetClient lands, and the
        // handler would drop the subscription because `inner == None`.
        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            anyhow::bail!("Failed to send SetClient command: {e}");
        }

        let initial_instruments: Vec<(i16, InstrumentAny)> = self
            .instruments
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        if !initial_instruments.is_empty()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(initial_instruments))
        {
            log::error!("Failed to send InitializeInstruments: {e}");
        }

        // Publish the new command channel and connection-mode atomic last.
        // Any clone-driven subscribe call queued from this point lands
        // behind SetClient and InitializeInstruments in cmd_rx.
        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);
        self.connection_mode.store(connection_mode_atomic);

        log::debug!("Lighter WebSocket connected: {}", self.url);

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let subscription_args = Arc::clone(&self.subscription_args);
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let task = get_runtime().spawn(async move {
            let mut handler =
                FeedHandler::new(Arc::clone(&signal), cmd_rx, raw_rx, out_tx, subscriptions);
            handler.set_command_sender(cmd_tx_for_reconnect.clone());

            let restore_subscriptions = || {
                if subscription_args.is_empty() {
                    log::debug!("No active Lighter subscriptions to restore after reconnect");
                    return;
                }
                log::debug!(
                    "Restoring {} Lighter subscriptions after reconnect",
                    subscription_args.len(),
                );

                for entry in subscription_args.iter() {
                    let (channel, auth) = entry.value().clone();
                    if let Err(e) =
                        cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { channel, auth })
                    {
                        log::error!("Failed to resend Lighter subscribe command: {e}");
                    }
                }
            };

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        log::debug!("Lighter WebSocket reconnected");
                        restore_subscriptions();

                        if handler.send(NautilusWsMessage::Reconnected).is_err() {
                            if handler.is_stopped() {
                                log::debug!("Failed to forward Reconnected (receiver dropped)");
                            } else {
                                log::error!("Failed to forward Reconnected (receiver dropped)");
                            }
                            break;
                        }
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            if handler.is_stopped() {
                                log::debug!("Failed to send Lighter message (receiver dropped)");
                            } else {
                                log::error!("Failed to send Lighter message (receiver dropped)");
                            }
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            log::debug!("Lighter handler stop signal observed, exiting loop");
                            break;
                        }
                        log::warn!("Lighter WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }
            log::debug!("Lighter handler task completed");
        });
        self.task_handle = Some(task);
        Ok(())
    }

    /// Disconnects gracefully: signals shutdown, drains the handler, then
    /// awaits the task handle with a timeout.
    ///
    /// # Errors
    ///
    /// This function currently completes best-effort shutdown and returns `Ok(())`.
    pub async fn disconnect(&mut self) -> Result<(), LighterWsError> {
        log::debug!("Disconnecting Lighter WebSocket");

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send Lighter disconnect command: {e}");
        }
        self.signal.store(true, Ordering::Release);

        if let Some(handle) = self.task_handle.take() {
            let abort_handle = handle.abort_handle();
            tokio::select! {
                result = handle => match result {
                    Ok(()) => log::debug!("Lighter handler task completed"),
                    Err(e) if e.is_cancelled() => log::debug!("Lighter handler task cancelled"),
                    Err(e) => log::error!("Lighter handler task error: {e:?}"),
                },
                () = tokio::time::sleep(DISCONNECT_TIMEOUT) => {
                    log::warn!("Timeout waiting for Lighter handler task, aborting");
                    abort_handle.abort();
                }
            }
        }

        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));
        Ok(())
    }

    /// Receives the next message from the handler, or `None` if the receiver
    /// has been taken or the handler has shut down.
    pub async fn next_event(&mut self) -> Option<NautilusWsMessage> {
        if let Some(rx) = self.out_rx.as_mut() {
            rx.recv().await
        } else {
            None
        }
    }

    /// Takes the feed-handler task handle, leaving `None` behind.
    ///
    /// Used by callers that connect on a cloned client and want to await the
    /// inner handler task on a different instance during disconnect.
    #[must_use]
    pub fn take_task_handle(&mut self) -> Option<tokio::task::JoinHandle<()>> {
        self.task_handle.take()
    }

    /// Installs a feed-handler task handle previously obtained from
    /// [`Self::take_task_handle`].
    pub fn set_task_handle(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.task_handle = Some(handle);
    }

    /// Subscribe to L2 order-book updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_cmd(HandlerCommand::SetBookDeltasSub {
            market_index,
            subscribed: true,
        })
        .await?;

        if let Err(e) = self.subscribe_order_book_stream(market_index).await {
            let _ = self
                .send_cmd(HandlerCommand::SetBookDeltasSub {
                    market_index,
                    subscribed: false,
                })
                .await;
            return Err(e);
        }

        Ok(())
    }

    /// Unsubscribe from L2 order-book updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn unsubscribe_book(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_cmd(HandlerCommand::SetBookDeltasSub {
            market_index,
            subscribed: false,
        })
        .await?;
        self.unsubscribe_order_book_stream(market_index).await
    }

    /// Subscribe to depth-10 snapshots derived from the same `order_book`
    /// stream as [`Self::subscribe_book`].
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn subscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_cmd(HandlerCommand::SetDepth10Sub {
            market_index,
            subscribed: true,
        })
        .await?;

        if let Err(e) = self.subscribe_order_book_stream(market_index).await {
            let _ = self
                .send_cmd(HandlerCommand::SetDepth10Sub {
                    market_index,
                    subscribed: false,
                })
                .await;
            return Err(e);
        }

        Ok(())
    }

    /// Unsubscribe from depth-10 snapshots.
    ///
    /// Clears the depth-10 emission flag without tearing down the underlying
    /// `order_book` stream so any active deltas subscriber keeps receiving
    /// updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn unsubscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_cmd(HandlerCommand::SetDepth10Sub {
            market_index,
            subscribed: false,
        })
        .await?;
        self.unsubscribe_order_book_stream(market_index).await
    }

    /// Subscribe to ticker (best bid/offer) updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn subscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_subscribe(LighterWsChannel::Ticker(market_index), None)
            .await
    }

    /// Unsubscribe from ticker updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_unsubscribe(LighterWsChannel::Ticker(market_index))
            .await
    }

    /// Subscribe to trade updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_subscribe(LighterWsChannel::Trade(market_index), None)
            .await
    }

    /// Unsubscribe from trade updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_unsubscribe(LighterWsChannel::Trade(market_index))
            .await
    }

    /// Subscribe to the `candle/{market_id}/{resolution}` stream for an
    /// instrument and resolution.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered, the resolution
    /// is not offered on the WebSocket stream, or the command cannot be
    /// queued.
    pub async fn subscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: LighterCandleResolution,
    ) -> Result<(), LighterWsError> {
        if !resolution.is_ws_streamable() {
            return Err(LighterWsError::Client(format!(
                "resolution {resolution:?} is not offered on the Lighter candle WebSocket stream",
            )));
        }
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_subscribe(
            LighterWsChannel::Candle {
                market_index,
                resolution,
            },
            None,
        )
        .await
    }

    /// Unsubscribe from a candle stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not registered or the command
    /// cannot be queued.
    pub async fn unsubscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: LighterCandleResolution,
    ) -> Result<(), LighterWsError> {
        let market_index = self.market_index_for(&instrument_id)?;
        self.send_unsubscribe(LighterWsChannel::Candle {
            market_index,
            resolution,
        })
        .await
    }

    /// Subscribe to a market-stats stream covering all markets or a single
    /// market index.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn subscribe_market_stats(
        &self,
        selection: LighterMarketSelection,
    ) -> Result<(), LighterWsError> {
        self.send_subscribe(LighterWsChannel::MarketStats(selection), None)
            .await
    }

    /// Unsubscribe from a market-stats stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn unsubscribe_market_stats(
        &self,
        selection: LighterMarketSelection,
    ) -> Result<(), LighterWsError> {
        self.send_unsubscribe(LighterWsChannel::MarketStats(selection))
            .await
    }

    /// Subscribe to a spot market-stats stream covering all spot markets or a
    /// single spot market index.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn subscribe_spot_market_stats(
        &self,
        selection: LighterMarketSelection,
    ) -> Result<(), LighterWsError> {
        self.send_subscribe(LighterWsChannel::SpotMarketStats(selection), None)
            .await
    }

    /// Unsubscribe from a spot market-stats stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn unsubscribe_spot_market_stats(
        &self,
        selection: LighterMarketSelection,
    ) -> Result<(), LighterWsError> {
        self.send_unsubscribe(LighterWsChannel::SpotMarketStats(selection))
            .await
    }

    /// Subscribe to the chain-height stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn subscribe_height(&self) -> Result<(), LighterWsError> {
        self.send_subscribe(LighterWsChannel::Height, None).await
    }

    /// Unsubscribe from the chain-height stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn unsubscribe_height(&self) -> Result<(), LighterWsError> {
        self.send_unsubscribe(LighterWsChannel::Height).await
    }

    /// Provides the execution context the feed handler stamps onto reports
    /// parsed from `account_*` frames.
    ///
    /// Without this context account frames fall back to
    /// [`NautilusWsMessage::Raw`]; once it is set the handler emits typed
    /// [`crate::websocket::messages::ExecutionReport`] and
    /// [`crate::websocket::messages::NautilusWsMessage::AccountState`]
    /// messages stamped with `account_id`. The `account_index` is used by
    /// the fill parser to determine which side of each account-trade frame
    /// the configured account took.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn set_execution_context(
        &self,
        account_id: AccountId,
        account_index: i64,
    ) -> Result<(), LighterWsError> {
        self.send_cmd(HandlerCommand::SetExecutionContext {
            account_id,
            account_index,
        })
        .await
    }

    /// Subscribe to a private account channel using a venue auth token.
    ///
    /// The auth token must be a valid Lighter L2 auth signature; see the
    /// `signing` module for token construction.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn subscribe_account(
        &self,
        channel: LighterWsChannel,
        auth_token: String,
    ) -> Result<(), LighterWsError> {
        self.send_subscribe(channel, Some(auth_token)).await
    }

    /// Unsubscribe from a private account channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn unsubscribe_account(
        &self,
        channel: LighterWsChannel,
    ) -> Result<(), LighterWsError> {
        self.send_unsubscribe(channel).await
    }

    /// Dispatch a signed L2 transaction over the WebSocket.
    ///
    /// `tx_type` is the venue's [`crate::common::enums::LighterTxType`]
    /// discriminant; `tx_info` is the JSON body produced by the matching
    /// [`crate::signing::tx::TxInfoJson`] renderer. The venue confirms
    /// acceptance via the `account_*` streams.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be queued.
    pub async fn send_tx(
        &self,
        tx_type: u8,
        tx_info: Box<serde_json::value::RawValue>,
    ) -> Result<(), LighterWsError> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.send_cmd(HandlerCommand::SendTx {
            tx_type,
            tx_info,
            response_tx,
        })
        .await?;

        response_rx
            .await
            .map_err(|e| LighterWsError::Client(format!("handler dropped sendTx result: {e}")))?
    }

    async fn send_subscribe(
        &self,
        channel: LighterWsChannel,
        auth: Option<String>,
    ) -> Result<(), LighterWsError> {
        let topic = channel.topic_key();
        let previous = self
            .subscription_args
            .insert(topic.clone(), (channel.clone(), auth.clone()));
        if let Err(e) = self
            .send_cmd(HandlerCommand::Subscribe { channel, auth })
            .await
        {
            if let Some(previous) = previous {
                self.subscription_args.insert(topic, previous);
            } else {
                self.subscription_args.remove(&topic);
            }
            return Err(e);
        }

        Ok(())
    }

    async fn send_unsubscribe(&self, channel: LighterWsChannel) -> Result<(), LighterWsError> {
        let topic = channel.topic_key();
        self.send_cmd(HandlerCommand::Unsubscribe { channel })
            .await?;
        self.subscription_args.remove(&topic);
        Ok(())
    }

    async fn subscribe_order_book_stream(&self, market_index: i16) -> Result<(), LighterWsError> {
        let channel = LighterWsChannel::OrderBook(market_index);
        let topic = channel.topic_key();

        if !self.subscriptions.add_reference(topic.as_str()) {
            return Ok(());
        }

        if let Err(e) = self.send_subscribe(channel, None).await {
            self.subscriptions.remove_reference(topic.as_str());
            return Err(e);
        }

        Ok(())
    }

    async fn unsubscribe_order_book_stream(&self, market_index: i16) -> Result<(), LighterWsError> {
        let channel = LighterWsChannel::OrderBook(market_index);
        let topic = channel.topic_key();

        if !self.subscriptions.remove_reference(topic.as_str()) {
            return Ok(());
        }

        if let Err(e) = self.send_unsubscribe(channel).await {
            self.subscriptions.add_reference(topic.as_str());
            return Err(e);
        }

        Ok(())
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> Result<(), LighterWsError> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| LighterWsError::Client(format!("handler unavailable: {e}")))
    }

    fn market_index_for(&self, instrument_id: &InstrumentId) -> Result<i16, LighterWsError> {
        self.registry.market_index(instrument_id).ok_or_else(|| {
            LighterWsError::Client(format!(
                "no Lighter market_index registered for instrument: {instrument_id}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::Symbol,
        instruments::CryptoPerpetual,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::{consts::LIGHTER_VENUE, enums::LighterProductType};

    fn registry_with(
        market_index: i16,
        symbol: &str,
        product: LighterProductType,
    ) -> Arc<MarketRegistry> {
        let registry = Arc::new(MarketRegistry::new());
        registry.insert(market_index, symbol, product);
        registry
    }

    #[rstest]
    fn market_index_for_returns_registered_index() {
        let registry = registry_with(7, "ETH", LighterProductType::Perp);
        let client = LighterWebSocketClient::new(
            Some("wss://example/test".to_string()),
            LighterEnvironment::Testnet,
            Arc::clone(&registry),
            TransportBackend::default(),
            None,
        );
        let id = registry.instrument_id(7).expect("registered");
        assert_eq!(client.market_index_for(&id).unwrap(), 7);
    }

    #[rstest]
    fn market_index_for_unregistered_returns_error() {
        let registry = Arc::new(MarketRegistry::new());
        let client = LighterWebSocketClient::new(
            Some("wss://example/test".to_string()),
            LighterEnvironment::Testnet,
            registry,
            TransportBackend::default(),
            None,
        );
        let id = InstrumentId::new(Symbol::from_str_unchecked("UNKNOWN-PERP"), *LIGHTER_VENUE);
        assert!(client.market_index_for(&id).is_err());
    }

    #[rstest]
    fn cache_instrument_populates_lookup() {
        let registry = registry_with(0, "ETH", LighterProductType::Perp);
        let client = LighterWebSocketClient::new(
            Some("wss://example/test".to_string()),
            LighterEnvironment::Testnet,
            Arc::clone(&registry),
            TransportBackend::default(),
            None,
        );
        let id = registry.instrument_id(0).expect("registered");
        let instrument = stub_instrument(id);
        client.cache_instrument(0, instrument);
        assert!(client.instruments_cache().contains_key(&0));
    }

    fn stub_instrument(id: InstrumentId) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            id,
            id.symbol,
            Currency::from("ETH"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }
}

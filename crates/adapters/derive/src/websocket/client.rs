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

//! `tokio-tungstenite`-backed WebSocket client for the Derive JSON-RPC stream.
//!
//! [`DeriveWebSocketClient`] orchestrates the connection lifecycle and exposes
//! a typed surface for `public/login` + the initial `ticker` channel. The
//! actual I/O runs in `super::handler::FeedHandler`; the client communicates
//! with it through an unbounded command channel and consumes
//! [`DeriveWsMessage`] events.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use alloy::signers::local::PrivateKeySigner;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use serde::de::DeserializeOwned;

use super::{
    error::{DeriveWsError, Result},
    handler::{
        DeriveWsMessage, FeedHandler, HandlerCommand, orderbook_subscribe_params,
        ticker_subscribe_params, trades_subscribe_params,
    },
    messages::{
        DeriveWsChannel, WsLoginParams, WsLoginResult, WsRequestParams, WsSubscribeParams,
        WsSubscribeResult, WsUnsubscribeParams, WsUnsubscribeResult, methods, orderbook_channel,
        ticker_channel, trades_channel,
    },
};
use crate::{
    common::{
        consts::{
            RECONNECT_BACKOFF_FACTOR, RECONNECT_BASE_BACKOFF, RECONNECT_JITTER_MS,
            RECONNECT_MAX_BACKOFF, RECONNECT_TIMEOUT, WS_HEARTBEAT_SECS,
        },
        enums::DeriveEnvironment,
        urls,
    },
    signing::auth::build_ws_login,
};

/// Credentials for `public/login`. The session-key signer never escapes the
/// client; only the wallet address is exposed via [`Debug`].
#[derive(Clone)]
pub struct DeriveWsCredentials {
    /// Derive Chain smart-contract wallet address (`0x`-prefixed, 42 chars).
    pub wallet_address: String,
    /// secp256k1 session-key signer.
    pub signer: PrivateKeySigner,
}

impl DeriveWsCredentials {
    /// Constructs credentials by parsing `session_key_hex` into a signer.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::Transport`] when the session-key hex cannot be parsed.
    pub fn new(wallet_address: impl Into<String>, session_key_hex: &str) -> Result<Self> {
        let signer: PrivateKeySigner = session_key_hex
            .parse()
            .map_err(|e| DeriveWsError::transport(format!("invalid session key: {e}")))?;
        Ok(Self {
            wallet_address: wallet_address.into(),
            signer,
        })
    }
}

impl Debug for DeriveWsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeriveWsCredentials))
            .field("wallet_address", &self.wallet_address)
            .field("signer", &"***redacted***")
            .finish()
    }
}

/// WebSocket client for the Derive JSON-RPC stream.
///
/// Construct with [`Self::new`] (public-only) or [`Self::with_credentials`]
/// when private channels and signed actions are needed. Call [`Self::connect`]
/// before any subscribe call; [`Self::disconnect`] tears the connection down.
#[derive(Debug)]
pub struct DeriveWebSocketClient {
    url: String,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    auth_tracker: AuthTracker,
    credentials: Option<DeriveWsCredentials>,
    next_id: Arc<AtomicU64>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>>,
    subscriptions: Arc<DashMap<String, ()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Cloneable command handle for Derive public market data subscriptions.
#[derive(Debug, Clone)]
pub struct DeriveWebSocketSubscriptionHandle {
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    subscriptions: Arc<DashMap<String, ()>>,
}

impl DeriveWebSocketClient {
    /// Builds a public-only client. URL falls back to the environment default
    /// when `url` is `None`.
    #[must_use]
    pub fn new(
        url: Option<String>,
        environment: DeriveEnvironment,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let url = url.unwrap_or_else(|| urls::ws_url(environment).to_string());
        Self::build(url, transport_backend, proxy_url, None)
    }

    /// Builds a client that will issue `public/login` on connect and replay
    /// it after each reconnect.
    #[must_use]
    pub fn with_credentials(
        url: Option<String>,
        environment: DeriveEnvironment,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: DeriveWsCredentials,
    ) -> Self {
        let url = url.unwrap_or_else(|| urls::ws_url(environment).to_string());
        Self::build(url, transport_backend, proxy_url, Some(credentials))
    }

    fn build(
        url: String,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: Option<DeriveWsCredentials>,
    ) -> Self {
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));
        // Placeholder channel; replaced by connect() before commands are issued.
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            url,
            transport_backend,
            proxy_url,
            connection_mode,
            signal: Arc::new(AtomicBool::new(false)),
            auth_tracker: AuthTracker::new(),
            credentials,
            next_id: Arc::new(AtomicU64::new(1)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            subscriptions: Arc::new(DashMap::new()),
            task_handle: None,
        }
    }

    /// Returns the configured WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns `true` when credentials are configured and the venue has
    /// confirmed the latest `public/login`. Cleared on reconnect.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_tracker.is_authenticated()
    }

    /// Returns `true` while the underlying transport is in the active state.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode.load().load(Ordering::Relaxed) == ConnectionMode::Active as u8
    }

    /// Establishes the WebSocket connection and spawns the I/O handler task.
    ///
    /// When credentials are configured, issues `public/login` and awaits the
    /// venue's acknowledgement before returning.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::Transport`] for handshake failures and
    /// propagates [`DeriveWsError::Auth`] / [`DeriveWsError::JsonRpc`] when
    /// the login flow fails.
    pub async fn connect(&mut self) -> Result<()> {
        // Fast path requires authenticated session when creds are configured;
        // otherwise fall through and rebuild so `Ok` always implies authenticated.
        let auth_ok = self.credentials.is_none() || self.is_authenticated();
        if self.is_active() && auth_ok && self.task_handle.is_some() {
            log::warn!("Derive WebSocket already connected");
            return Ok(());
        }

        // Tear down stale state so we don't orphan the old handler task on rebuild.
        if self.task_handle.is_some() {
            log::debug!("Tearing down stale Derive WebSocket state before connect");
            self.teardown().await;
        }

        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(WS_HEARTBEAT_SECS),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(RECONNECT_TIMEOUT.as_millis() as u64),
            reconnect_delay_initial_ms: Some(RECONNECT_BASE_BACKOFF.as_millis() as u64),
            reconnect_delay_max_ms: Some(RECONNECT_MAX_BACKOFF.as_millis() as u64),
            reconnect_backoff_factor: Some(RECONNECT_BACKOFF_FACTOR),
            reconnect_jitter_ms: Some(RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };
        let client = WebSocketClient::connect(cfg, Some(message_handler), None, None, vec![], None)
            .await
            .map_err(|e| DeriveWsError::transport(e.to_string()))?;

        // Register the tracker so the network controller clears
        // `is_authenticated()` on dead-socket detection, not just on the
        // later RECONNECTED sentinel.
        client.set_auth_tracker(self.auth_tracker.clone(), false);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<DeriveWsMessage>();

        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);

        self.connection_mode.store(client.connection_mode_atomic());
        log::info!("Derive WebSocket connected: {}", self.url);

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            return Err(DeriveWsError::transport(format!(
                "failed to send SetClient command: {e}",
            )));
        }

        let signal = Arc::clone(&self.signal);
        let auth_tracker = self.auth_tracker.clone();
        let next_id = Arc::clone(&self.next_id);
        let credentials = self.credentials.clone();
        let subscriptions = Arc::clone(&self.subscriptions);
        let cmd_tx_for_loop = cmd_tx.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                FeedHandler::new(signal, cmd_rx, raw_rx, next_id, auth_tracker.clone());

            loop {
                match handler.next().await {
                    Some(DeriveWsMessage::Reconnected) => {
                        log::info!("Derive WebSocket re-establishing session after reconnect");
                        if out_tx.send(DeriveWsMessage::Reconnected).is_err() {
                            log::debug!("Derive outer receiver dropped, exiting stream loop");
                            break;
                        }
                        // Spawn so the loop keeps draining messages while
                        // re-login + resubscribe are in flight.
                        let cmd_tx_async = cmd_tx_for_loop.clone();
                        let auth_tracker_async = auth_tracker.clone();
                        let creds_async = credentials.clone();
                        let subs_async = Arc::clone(&subscriptions);

                        get_runtime().spawn(async move {
                            if let Some(creds) = creds_async
                                && let Err(e) =
                                    login_via_handler(&cmd_tx_async, &auth_tracker_async, &creds)
                                        .await
                            {
                                log::error!("Derive WebSocket re-login failed: {e}");
                            }
                            // Snapshot channels before awaiting: a DashMap
                            // shard guard held across `.await` can deadlock
                            // on a single-worker runtime.
                            let channels: Vec<String> =
                                subs_async.iter().map(|e| e.key().clone()).collect();
                            for channel in channels {
                                if let Err(e) =
                                    subscribe_via_handler(&cmd_tx_async, vec![channel.clone()])
                                        .await
                                {
                                    log::error!(
                                        "Derive WebSocket resubscribe failed for {channel}: {e}",
                                    );
                                }
                            }
                        });
                    }
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            log::debug!("Derive outer receiver dropped, exiting stream loop");
                            break;
                        }
                    }
                    None => {
                        log::debug!("Derive handler task ended");
                        break;
                    }
                }
            }
        });
        self.task_handle = Some(stream_handle);

        if let Some(creds) = self.credentials.clone()
            && let Err(e) = login_via_handler(&cmd_tx, &self.auth_tracker, &creds).await
        {
            // Without teardown, a retry connect() would short-circuit on
            // is_active() and return Ok without a valid session.
            log::warn!("Derive WebSocket login failed; tearing down transport: {e}");
            self.teardown().await;
            return Err(e);
        }

        Ok(())
    }

    /// Signals the handler to disconnect, aborts the spawn task, and resets
    /// the client's transport-related state. Shared by [`Self::disconnect`]
    /// and the login-failure branch of [`Self::connect`].
    async fn teardown(&mut self) {
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!(
                "Failed to enqueue Disconnect command (handler may already be shut down): {e}",
            );
        }

        if let Some(handle) = self.task_handle.take() {
            let abort_handle = handle.abort_handle();
            tokio::select! {
                result = handle => match result {
                    Ok(()) => log::debug!("Derive WebSocket task completed"),
                    Err(e) if e.is_cancelled() => log::debug!("Derive WebSocket task cancelled"),
                    Err(e) => log::error!("Derive WebSocket task error: {e:?}"),
                },
                () = tokio::time::sleep(Duration::from_secs(2)) => {
                    log::warn!("Timeout waiting for Derive WebSocket task, aborting");
                    abort_handle.abort();
                }
            }
        }

        // Subscriptions are also dropped: the venue session ended with the
        // transport, so a fresh connect() must re-issue them.
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();
        *self.cmd_tx.write().await = placeholder_tx;
        self.out_rx = None;
        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));
        self.auth_tracker.invalidate();
        self.subscriptions.clear();
        self.signal.store(false, Ordering::Relaxed);
    }

    /// Disconnects the WebSocket connection and awaits the handler task.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::Transport`] when the disconnect command
    /// cannot be enqueued; the handler still tears down on signal.
    pub async fn disconnect(&mut self) -> Result<()> {
        log::info!("Disconnecting Derive WebSocket");
        self.teardown().await;
        Ok(())
    }

    /// Subscribes to `ticker_slim.{instrument_name}.{interval}`. `interval` is the
    /// millisecond cadence string the venue exposes (e.g. `"100"`, `"1000"`).
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_ticker(&self, instrument_name: &str, interval: &str) -> Result<()> {
        self.subscription_handle()
            .subscribe_ticker(instrument_name, interval)
            .await
    }

    /// Unsubscribes from `ticker_slim.{instrument_name}.{interval}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_ticker(&self, instrument_name: &str, interval: &str) -> Result<()> {
        self.subscription_handle()
            .unsubscribe_ticker(instrument_name, interval)
            .await
    }

    /// Subscribes to `orderbook.{instrument_name}.{group}.{depth}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_orderbook(
        &self,
        instrument_name: &str,
        group: &str,
        depth: &str,
    ) -> Result<()> {
        self.subscription_handle()
            .subscribe_orderbook(instrument_name, group, depth)
            .await
    }

    /// Unsubscribes from `orderbook.{instrument_name}.{group}.{depth}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_orderbook(
        &self,
        instrument_name: &str,
        group: &str,
        depth: &str,
    ) -> Result<()> {
        self.subscription_handle()
            .unsubscribe_orderbook(instrument_name, group, depth)
            .await
    }

    /// Subscribes to `trades.{instrument_type}.{currency}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_trades(&self, instrument_type: &str, currency: &str) -> Result<()> {
        self.subscription_handle()
            .subscribe_trades(instrument_type, currency)
            .await
    }

    /// Unsubscribes from `trades.{instrument_type}.{currency}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_trades(&self, instrument_type: &str, currency: &str) -> Result<()> {
        self.subscription_handle()
            .unsubscribe_trades(instrument_type, currency)
            .await
    }

    /// Subscribes to a list of channel topics in a single `subscribe` frame.
    ///
    /// Used by the execution client to bulk-subscribe to the private
    /// `{subaccount_id}.orders`, `{subaccount_id}.trades`, and
    /// `{subaccount_id}.balances` channels after login.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_channels<C>(&self, channels: Vec<C>) -> Result<()>
    where
        C: Into<DeriveWsChannel>,
    {
        self.subscription_handle()
            .subscribe_channels(channels)
            .await
    }

    /// Unsubscribes from a list of channel topics in a single
    /// `unsubscribe` frame.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_channels<C>(&self, channels: Vec<C>) -> Result<()>
    where
        C: Into<DeriveWsChannel>,
    {
        self.subscription_handle()
            .unsubscribe_channels(channels)
            .await
    }

    /// Returns the next event emitted by the handler.
    pub async fn next_event(&mut self) -> Option<DeriveWsMessage> {
        if let Some(rx) = self.out_rx.as_mut() {
            rx.recv().await
        } else {
            None
        }
    }

    /// Returns the count of channels the client currently has confirmed
    /// subscriptions for.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns a cloneable handle for issuing subscription commands.
    #[must_use]
    pub fn subscription_handle(&self) -> DeriveWebSocketSubscriptionHandle {
        DeriveWebSocketSubscriptionHandle {
            cmd_tx: Arc::clone(&self.cmd_tx),
            subscriptions: Arc::clone(&self.subscriptions),
        }
    }

    /// Takes the event receiver from the client.
    ///
    /// This lets the live data client own the receive loop while subscription
    /// commands continue through [`Self::subscription_handle`].
    pub fn take_event_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>> {
        self.out_rx.take()
    }
}

impl DeriveWebSocketSubscriptionHandle {
    /// Subscribes to `ticker_slim.{instrument_name}.{interval}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_ticker(&self, instrument_name: &str, interval: &str) -> Result<()> {
        let channel = ticker_channel(instrument_name, interval);
        let params = ticker_subscribe_params(instrument_name, interval);
        self.send_subscribe(channel, &params).await
    }

    /// Unsubscribes from `ticker_slim.{instrument_name}.{interval}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_ticker(&self, instrument_name: &str, interval: &str) -> Result<()> {
        let channel = ticker_channel(instrument_name, interval);
        self.send_unsubscribe(channel).await
    }

    /// Subscribes to `orderbook.{instrument_name}.{group}.{depth}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_orderbook(
        &self,
        instrument_name: &str,
        group: &str,
        depth: &str,
    ) -> Result<()> {
        let channel = orderbook_channel(instrument_name, group, depth);
        let params = orderbook_subscribe_params(instrument_name, group, depth);
        self.send_subscribe(channel, &params).await
    }

    /// Unsubscribes from `orderbook.{instrument_name}.{group}.{depth}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_orderbook(
        &self,
        instrument_name: &str,
        group: &str,
        depth: &str,
    ) -> Result<()> {
        let channel = orderbook_channel(instrument_name, group, depth);
        self.send_unsubscribe(channel).await
    }

    /// Subscribes to `trades.{instrument_type}.{currency}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_trades(&self, instrument_type: &str, currency: &str) -> Result<()> {
        let channel = trades_channel(instrument_type, currency);
        let params = trades_subscribe_params(instrument_type, currency);
        self.send_subscribe(channel, &params).await
    }

    /// Unsubscribes from `trades.{instrument_type}.{currency}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_trades(&self, instrument_type: &str, currency: &str) -> Result<()> {
        let channel = trades_channel(instrument_type, currency);
        self.send_unsubscribe(channel).await
    }

    /// Subscribes to multiple channel topics in a single `subscribe` frame.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_channels<C>(&self, channels: Vec<C>) -> Result<()>
    where
        C: Into<DeriveWsChannel>,
    {
        let channels = channels.into_iter().map(Into::into).collect::<Vec<_>>();
        if channels.is_empty() {
            return Ok(());
        }
        let topics = channel_topics(&channels);
        let params = WsSubscribeParams { channels };
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsSubscribeResult =
            send_request(&cmd_tx, methods::PUBLIC_SUBSCRIBE, params.into()).await?;

        for channel in topics {
            self.subscriptions.insert(channel, ());
        }
        Ok(())
    }

    /// Unsubscribes from multiple channel topics in a single
    /// `unsubscribe` frame.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn unsubscribe_channels<C>(&self, channels: Vec<C>) -> Result<()>
    where
        C: Into<DeriveWsChannel>,
    {
        let channels = channels.into_iter().map(Into::into).collect::<Vec<_>>();
        if channels.is_empty() {
            return Ok(());
        }
        let topics = channel_topics(&channels);
        let params = WsUnsubscribeParams { channels };
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsUnsubscribeResult =
            send_request(&cmd_tx, methods::PUBLIC_UNSUBSCRIBE, params.into()).await?;

        for channel in topics {
            self.subscriptions.remove(&channel);
        }
        Ok(())
    }

    async fn send_subscribe(&self, channel: String, params: &WsSubscribeParams) -> Result<()> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsSubscribeResult =
            send_request(&cmd_tx, methods::PUBLIC_SUBSCRIBE, params.clone().into()).await?;
        self.subscriptions.insert(channel, ());
        Ok(())
    }

    async fn send_unsubscribe(&self, channel: String) -> Result<()> {
        let params = WsUnsubscribeParams {
            channels: vec![DeriveWsChannel::from(channel.clone())],
        };
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsUnsubscribeResult =
            send_request(&cmd_tx, methods::PUBLIC_UNSUBSCRIBE, params.into()).await?;
        self.subscriptions.remove(&channel);
        Ok(())
    }
}

async fn send_request<R>(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: WsRequestParams,
) -> Result<R>
where
    R: Default + DeserializeOwned,
{
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(HandlerCommand::Request {
            method,
            params,
            response_tx,
        })
        .map_err(|e| DeriveWsError::transport(format!("failed to enqueue `{method}`: {e}")))?;
    let value = response_rx
        .await
        .map_err(|_| DeriveWsError::RequestCancelled {
            method: method.to_owned(),
        })??;
    let typed = if value.is_null() {
        R::default()
    } else {
        serde_json::from_value(value)?
    };
    Ok(typed)
}

fn channel_topics(channels: &[DeriveWsChannel]) -> Vec<String> {
    channels.iter().map(ToString::to_string).collect()
}

async fn login_via_handler(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    auth_tracker: &AuthTracker,
    creds: &DeriveWsCredentials,
) -> Result<()> {
    let login = build_ws_login(&creds.wallet_address, &creds.signer)?;
    let params = WsLoginParams {
        wallet: login.wallet,
        timestamp: login.timestamp,
        signature: login.signature,
    };
    let _receiver = auth_tracker.begin();

    match send_request::<WsLoginResult>(cmd_tx, methods::PUBLIC_LOGIN, params.into()).await {
        Ok(_) => {
            auth_tracker.succeed();
            log::info!("Derive WebSocket authenticated");
            Ok(())
        }
        Err(e) => {
            auth_tracker.fail(e.to_string());
            Err(e)
        }
    }
}

async fn subscribe_via_handler(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    channels: Vec<String>,
) -> Result<()> {
    let params = WsSubscribeParams {
        channels: channels.into_iter().map(DeriveWsChannel::from).collect(),
    };
    let _: WsSubscribeResult =
        send_request(cmd_tx, methods::PUBLIC_SUBSCRIBE, params.into()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_public_client_defaults_to_environment_url() {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Mainnet,
            TransportBackend::default(),
            None,
        );
        assert!(client.url().starts_with("wss://"));
        assert!(client.url().contains("api.lyra.finance"));
        assert!(!client.is_authenticated());
        assert!(!client.is_active());
        assert_eq!(client.subscription_count(), 0);
    }

    #[rstest]
    fn test_testnet_client_routes_to_demo_url() {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        assert!(client.url().contains("demo"));
    }

    #[rstest]
    fn test_credentials_constructor_parses_session_key() {
        let creds = DeriveWsCredentials::new(
            "0x000000000000000000000000000000000000aaaa",
            "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd",
        )
        .unwrap();
        assert!(creds.wallet_address.starts_with("0x"));
        let client = DeriveWebSocketClient::with_credentials(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
            creds,
        );
        assert!(client.url().contains("demo"));
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_credentials_debug_redacts_signer() {
        let creds = DeriveWsCredentials::new(
            "0xWALLET",
            "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd",
        )
        .unwrap();
        let debug = format!("{creds:?}");
        assert!(debug.contains("redacted"));
        assert!(debug.contains("0xWALLET"));
        assert!(!debug.contains("2ae8be44"));
    }

    #[rstest]
    fn test_credentials_constructor_rejects_invalid_session_key() {
        let err = DeriveWsCredentials::new("0xWALLET", "not-a-hex-key").unwrap_err();
        assert!(err.to_string().contains("invalid session key"));
    }
}

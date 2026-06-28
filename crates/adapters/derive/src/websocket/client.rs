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
use nautilus_core::UUID4;
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
    websocket::{
        AuthTracker, TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use ustr::Ustr;

use super::{
    error::{DeriveWsError, Result},
    handler::{
        DeriveWsMessage, FeedHandler, HandlerCommand, orderbook_subscribe_params,
        ticker_subscribe_params, trades_subscribe_params,
    },
    messages::{
        DeriveWsChannel, WsLoginParams, WsLoginResult, WsSubscribeParams, WsSubscribeResult,
        WsUnsubscribeParams, WsUnsubscribeResult, methods, orderbook_channel, rate_limit_key_for,
        ticker_channel, trades_channel,
    },
};
use crate::{
    common::{
        consts::{
            RECONNECT_BACKOFF_FACTOR, RECONNECT_BASE_BACKOFF, RECONNECT_JITTER_MS,
            RECONNECT_MAX_BACKOFF, RECONNECT_TIMEOUT, WS_HEARTBEAT_SECS, WS_REQUEST_TIMEOUT,
        },
        enums::DeriveEnvironment,
        rate_limit::{self, DERIVE_MATCHING_RATE_KEY},
        urls,
    },
    http::{
        models::{
            DeriveEmptyResult, DeriveOpenOrdersResult, DeriveOrder, DeriveOrderResult,
            DeriveReplaceResult,
        },
        query::{
            DeriveCancelAllParams, DeriveCancelParams, DeriveCancelTriggerOrderParams,
            DeriveGetTriggerOrdersParams, DeriveOrderParams, DeriveReplaceParams,
            DeriveTriggerOrderParams,
        },
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

// Rate limiter keyed by request kind (matching vs non-matching), shared with the
// command handles so each frame is paced in the caller's task before it is
// enqueued for the feed handler.
type WsRateLimiter = RateLimiter<Ustr, MonotonicClock>;

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
    request_timeout: Duration,
    conn_id: Arc<ArcSwap<String>>,
    rate_limiter: Arc<WsRateLimiter>,
}

/// Cloneable command handle for Derive public market data subscriptions.
#[derive(Debug, Clone)]
pub struct DeriveWebSocketSubscriptionHandle {
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    subscriptions: Arc<DashMap<String, ()>>,
    request_timeout: Duration,
    rate_limiter: Arc<WsRateLimiter>,
}

/// Cloneable handle for issuing signed `private/*` trading requests over the
/// WebSocket transport.
///
/// Carries the same `cmd_tx` the owning [`DeriveWebSocketClient`] swaps on
/// connect/reconnect, so a handle obtained at construction stays valid for the
/// client's lifetime. The handle is transport-only: it sends the pre-signed
/// body and surfaces the venue's JSON-RPC outcome. Session authorization is the
/// client's responsibility (via `public/login`).
#[derive(Debug, Clone)]
pub struct DeriveWsExecutionHandle {
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    request_timeout: Duration,
    conn_id: Arc<ArcSwap<String>>,
    rate_limiter: Arc<WsRateLimiter>,
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
        Self::build(url, transport_backend, proxy_url, None, None)
    }

    /// Builds a client that will issue `public/login` on connect and replay
    /// it after each reconnect.
    ///
    /// `max_matching_requests_per_second` sets the matching-engine rate limit
    /// for order writes; `None` applies the Trader-tier default. See
    /// [`crate::common::rate_limit`].
    #[must_use]
    pub fn with_credentials(
        url: Option<String>,
        environment: DeriveEnvironment,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: DeriveWsCredentials,
        max_matching_requests_per_second: Option<u32>,
    ) -> Self {
        let url = url.unwrap_or_else(|| urls::ws_url(environment).to_string());
        let matching_quota = rate_limit::matching_quota(max_matching_requests_per_second);
        Self::build(
            url,
            transport_backend,
            proxy_url,
            Some(credentials),
            Some(matching_quota),
        )
    }

    fn build(
        url: String,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: Option<DeriveWsCredentials>,
        matching_quota: Option<Quota>,
    ) -> Self {
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));
        // Placeholder channel; replaced by connect() before commands are issued.
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();
        // Matching-engine writes draw on the matching quota (authenticated
        // clients only); logins, subscriptions, and reads fall through to the
        // non-matching default. Handles pace each frame against this in the
        // caller's task before enqueueing, so the feed handler never sleeps.
        let mut keyed_quotas: Vec<(Ustr, Quota)> = Vec::new();
        if let Some(quota) = matching_quota {
            keyed_quotas.push((Ustr::from(DERIVE_MATCHING_RATE_KEY), quota));
        }
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(
            Some(rate_limit::non_matching_quota()),
            keyed_quotas,
        ));
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
            request_timeout: WS_REQUEST_TIMEOUT,
            conn_id: Arc::new(ArcSwap::from_pointee(UUID4::new().to_string())),
            rate_limiter,
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
        // Rate limiting runs caller-side via `self.rate_limiter` before frames
        // are enqueued, so the network client's own limiter is left unconfigured
        // and never sleeps inside the single feed-handler task.
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
        self.conn_id.store(Arc::new(UUID4::new().to_string()));

        self.connection_mode.store(client.connection_mode_atomic());
        log::debug!("Derive WebSocket connected: {}", self.url);

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
        let conn_id = Arc::clone(&self.conn_id);
        let cmd_tx_for_loop = cmd_tx.clone();
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let request_timeout = self.request_timeout;

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                FeedHandler::new(signal, cmd_rx, raw_rx, next_id, auth_tracker.clone());

            loop {
                match handler.next().await {
                    Some(DeriveWsMessage::Reconnected) => {
                        log::info!("Derive WebSocket re-establishing session after reconnect");
                        conn_id.store(Arc::new(UUID4::new().to_string()));

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
                        let rate_limiter_async = Arc::clone(&rate_limiter);

                        get_runtime().spawn(async move {
                            if let Some(creds) = creds_async
                                && let Err(e) = login_via_handler(
                                    &rate_limiter_async,
                                    &cmd_tx_async,
                                    &auth_tracker_async,
                                    &creds,
                                    request_timeout,
                                )
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
                                if let Err(e) = subscribe_via_handler(
                                    &rate_limiter_async,
                                    &cmd_tx_async,
                                    vec![channel.clone()],
                                    request_timeout,
                                )
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
            && let Err(e) = login_via_handler(
                &self.rate_limiter,
                &cmd_tx,
                &self.auth_tracker,
                &creds,
                self.request_timeout,
            )
            .await
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
        log::debug!("Disconnecting Derive WebSocket");
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
            request_timeout: self.request_timeout,
            rate_limiter: Arc::clone(&self.rate_limiter),
        }
    }

    /// Returns a cloneable handle for issuing signed `private/*` trading
    /// requests.
    ///
    /// The handle shares the client's command channel, so it stays valid across
    /// reconnects (the channel is swapped behind a shared lock). Obtain it once
    /// and clone it into each order-submission task.
    #[must_use]
    pub fn execution_handle(&self) -> DeriveWsExecutionHandle {
        DeriveWsExecutionHandle {
            cmd_tx: Arc::clone(&self.cmd_tx),
            request_timeout: self.request_timeout,
            conn_id: Arc::clone(&self.conn_id),
            rate_limiter: Arc::clone(&self.rate_limiter),
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
        let _: WsSubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_SUBSCRIBE,
            &params,
            self.request_timeout,
        )
        .await?;

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
        let _: WsUnsubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_UNSUBSCRIBE,
            &params,
            self.request_timeout,
        )
        .await?;

        for channel in topics {
            self.subscriptions.remove(&channel);
        }
        Ok(())
    }

    async fn send_subscribe(&self, channel: String, params: &WsSubscribeParams) -> Result<()> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsSubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_SUBSCRIBE,
            params,
            self.request_timeout,
        )
        .await?;
        self.subscriptions.insert(channel, ());
        Ok(())
    }

    async fn send_unsubscribe(&self, channel: String) -> Result<()> {
        let params = WsUnsubscribeParams {
            channels: vec![DeriveWsChannel::from(channel.clone())],
        };
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: WsUnsubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_UNSUBSCRIBE,
            &params,
            self.request_timeout,
        )
        .await?;
        self.subscriptions.remove(&channel);
        Ok(())
    }
}

impl DeriveWsExecutionHandle {
    /// Returns the current WebSocket connection id used by trigger orders.
    #[must_use]
    pub fn conn_id(&self) -> String {
        self.conn_id.load_full().as_ref().clone()
    }

    /// Submits a signed order via `private/order`.
    ///
    /// `params` must be the fully-built signed body from
    /// [`crate::http::query::order_to_derive_payload`]. Returns the accepted
    /// order echoed by the venue.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn submit_order(&self, params: &DeriveOrderParams) -> Result<DeriveOrder> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let result: DeriveOrderResult = send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_ORDER,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(result.order)
    }

    /// Submits a signed trigger order via `private/trigger_order`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn submit_trigger_order(
        &self,
        params: &DeriveTriggerOrderParams,
    ) -> Result<DeriveOrder> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let result: DeriveOrderResult = send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_TRIGGER_ORDER,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(result.order)
    }

    /// Modifies a working order by atomically cancelling it and submitting a
    /// replacement (the venue's `private/replace`). Returns the new order
    /// echoed by the venue.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn modify_order(&self, params: &DeriveReplaceParams) -> Result<DeriveOrder> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let result: DeriveReplaceResult = send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_REPLACE,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(result.order)
    }

    /// Cancels a single order via `private/cancel`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_order(&self, params: &DeriveCancelParams) -> Result<()> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: DeriveEmptyResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(())
    }

    /// Cancels a single trigger order via `private/cancel_trigger_order`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_trigger_order(
        &self,
        params: &DeriveCancelTriggerOrderParams,
    ) -> Result<DeriveOrder> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL_TRIGGER_ORDER,
            params,
            self.request_timeout,
        )
        .await
    }

    /// Returns currently untriggered trigger orders via
    /// `private/get_trigger_orders`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn get_trigger_orders(
        &self,
        params: &DeriveGetTriggerOrdersParams,
    ) -> Result<DeriveOpenOrdersResult> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_GET_TRIGGER_ORDERS,
            params,
            self.request_timeout,
        )
        .await
    }

    /// Cancels every open order on the subaccount (the venue's
    /// `private/cancel_all`), optionally scoped to an instrument.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_all_orders(&self, params: &DeriveCancelAllParams) -> Result<()> {
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: DeriveEmptyResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL_ALL,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(())
    }
}

// Awaits the venue's raw `result`, bounded by `timeout`. A dropped responder
// (handler torn down on reconnect) surfaces as `RequestCancelled`, a timeout as
// `Timeout`; both leave a state-changing write's outcome ambiguous.
async fn send_raw<P>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
) -> Result<Value>
where
    P: Serialize + ?Sized,
{
    let params = serde_json::to_value(params)?;

    // Pace in the caller's task before enqueueing, so the shared feed handler
    // never sleeps mid-loop. Matching methods (order/cancel/replace) draw on the
    // matching quota; everything else on the non-matching default.
    //
    // Known limitation: matching writes arrive here already signed with a
    // `signature_expiry_sec`, so a long pace eats into that TTL. It only bites
    // under a pathological backlog (wait beyond the venue's ~300s TTL margin,
    // i.e. hundreds of orders queued at the 1/s Trader rate); moderate bursts
    // stay well inside it. Pacing above the signing layer would remove it and
    // is left as a follow-up.
    let rate_keys = [rate_limit_key_for(method)];
    rate_limiter.await_keys_ready(Some(&rate_keys)).await;

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(HandlerCommand::Request {
            method,
            params,
            response_tx,
        })
        .map_err(|e| DeriveWsError::transport(format!("failed to enqueue `{method}`: {e}")))?;

    // On timeout the handler's `pending` entry leaks until the next reconnect's
    // `fail_pending` drains it; the later send to the dropped receiver is a
    // no-op logged at debug.
    match tokio::time::timeout(timeout, response_rx).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => Err(DeriveWsError::RequestCancelled {
            method: method.to_owned(),
        }),
        Err(_) => Err(DeriveWsError::Timeout {
            method: method.to_owned(),
        }),
    }
}

// Decodes the result, treating a null/absent `result` as `R::default()` (for
// login/subscribe/unsubscribe and the cancel family's `DeriveEmptyResult`).
async fn send_request<P, R>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
) -> Result<R>
where
    P: Serialize + ?Sized,
    R: Default + DeserializeOwned,
{
    let value = send_raw(rate_limiter, cmd_tx, method, params, timeout).await?;
    let typed = if value.is_null() {
        R::default()
    } else {
        serde_json::from_value(value)?
    };
    Ok(typed)
}

// Decodes the result with no `Default` fallback, for `private/order` and
// `private/replace` whose success result is always a populated object.
async fn send_request_typed<P, R>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
) -> Result<R>
where
    P: Serialize + ?Sized,
    R: DeserializeOwned,
{
    let value = send_raw(rate_limiter, cmd_tx, method, params, timeout).await?;
    Ok(serde_json::from_value(value)?)
}

fn channel_topics(channels: &[DeriveWsChannel]) -> Vec<String> {
    channels.iter().map(ToString::to_string).collect()
}

async fn login_via_handler(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    auth_tracker: &AuthTracker,
    creds: &DeriveWsCredentials,
    timeout: Duration,
) -> Result<()> {
    let login = build_ws_login(&creds.wallet_address, &creds.signer)?;
    let params = WsLoginParams {
        wallet: login.wallet,
        timestamp: login.timestamp,
        signature: login.signature,
    };
    let _receiver = auth_tracker.begin();

    match send_request::<_, WsLoginResult>(
        rate_limiter,
        cmd_tx,
        methods::PUBLIC_LOGIN,
        &params,
        timeout,
    )
    .await
    {
        Ok(_) => {
            auth_tracker.succeed();
            log::debug!("Derive WebSocket authenticated");
            Ok(())
        }
        Err(e) => {
            auth_tracker.fail(e.to_string());
            Err(e)
        }
    }
}

async fn subscribe_via_handler(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    channels: Vec<String>,
    timeout: Duration,
) -> Result<()> {
    let params = WsSubscribeParams {
        channels: channels.into_iter().map(DeriveWsChannel::from).collect(),
    };
    let _: WsSubscribeResult = send_request(
        rate_limiter,
        cmd_tx,
        methods::PUBLIC_SUBSCRIBE,
        &params,
        timeout,
    )
    .await?;
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
            None,
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

    #[rstest]
    #[tokio::test]
    async fn test_send_raw_times_out_when_no_response_arrives() {
        // Keep the receiver alive so the request enqueues, but never reply: the
        // bounded await must surface a Timeout rather than hang forever.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let rate_limiter: WsRateLimiter = RateLimiter::new_with_quota(None, Vec::new());
        let err = send_raw(
            &rate_limiter,
            &cmd_tx,
            methods::PRIVATE_ORDER,
            &serde_json::json!({}),
            Duration::from_millis(50),
        )
        .await
        .expect_err("must time out");

        match err {
            DeriveWsError::Timeout { method } => assert_eq!(method, methods::PRIVATE_ORDER),
            other => panic!("expected Timeout, was {other:?}"),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_request_typed_rejects_null_result() {
        // `private/order` and `private/replace` always return a populated
        // object on success; a null result is a protocol violation that must
        // surface as a serde error (classified ambiguous by the exec client).
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        tokio::spawn(async move {
            if let Some(HandlerCommand::Request { response_tx, .. }) = cmd_rx.recv().await {
                let _ = response_tx.send(Ok(Value::Null));
            }
        });
        let rate_limiter: WsRateLimiter = RateLimiter::new_with_quota(None, Vec::new());
        let result: Result<DeriveOrderResult> = send_request_typed(
            &rate_limiter,
            &cmd_tx,
            methods::PRIVATE_ORDER,
            &serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(result, Err(DeriveWsError::Serde(_))));
    }
}

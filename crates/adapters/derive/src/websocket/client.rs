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
use nautilus_common::live::dst::time;
#[cfg(test)]
use nautilus_common::live::get_runtime;
use nautilus_core::{
    UUID4,
    string::secret::{REDACTED, SecretString},
};
use nautilus_live::{
    SocketControl,
    task::{SharedTaskSlot, TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_network::{
    http::create_standard_nautilus_headers,
    mode::ConnectionMode,
    ratelimiter::clock::MonotonicClock,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use ustr::Ustr;
use zeroize::Zeroizing;

use super::{
    error::{DeriveWsError, Result},
    handler::{
        DeriveWsMessage, FeedHandler, HandlerCommand, orderbook_subscribe_params,
        ticker_subscribe_params, trades_subscribe_params,
    },
    messages::{
        DeriveWsChannel, WsLoginParams, WsLoginResult, WsSubscribeParams, WsSubscribeResult,
        WsUnsubscribeParams, WsUnsubscribeResult, methods, orderbook_channel, ticker_channel,
        trades_channel,
    },
};
use crate::{
    common::{
        consts::{
            RECONNECT_BACKOFF_FACTOR, RECONNECT_BASE_BACKOFF, RECONNECT_JITTER_MS,
            RECONNECT_MAX_BACKOFF, RECONNECT_TIMEOUT, WS_HEARTBEAT_SECS, WS_HEARTBEAT_TIMEOUT,
            WS_REQUEST_TIMEOUT,
        },
        enums::DeriveEnvironment,
        parse::salvage_rows,
        rate_limit::{DeriveRateLimiter, RateClass, RateReservation, rate_class_for_method},
        retry::should_retry_ws_error,
        urls,
    },
    http::{
        models::{
            DeriveCancelByInstrumentResult, DeriveCancelByLabelResult, DeriveEmptyResult,
            DeriveOpenOrdersResult, DeriveOrder, DeriveOrderResult, DeriveReplaceOutcome,
            DeriveReplaceResult, DeriveTrade,
        },
        query::{
            DeriveCancelAllParams, DeriveCancelByInstrumentParams, DeriveCancelByLabelParams,
            DeriveCancelParams, DeriveCancelTriggerOrderParams, DeriveGetTriggerOrdersParams,
            DeriveOrderParams, DeriveReplaceParams, DeriveTriggerOrderParams,
        },
    },
    signing::auth::build_ws_login,
};

/// Credentials for `public/login`. The session-key signer never escapes the
/// client; only the wallet address is exposed via [`Debug`].
#[derive(Clone)]
pub struct DeriveWsCredentials {
    /// Owner's EOA or multisig address (`0x`-prefixed, 42 chars).
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
            .field("signer", &REDACTED)
            .finish()
    }
}

// Wallet limiter shared with the command handles so each frame is
// paced in the caller's task before it is enqueued for the feed handler.
type WsRateLimiter = DeriveRateLimiter;

const MAX_SESSION_RECOVERY_ATTEMPTS: u32 = 3;
const SUBSCRIPTION_ACCEPTED_STATUSES: &[&str] = &["ok"];
const SUBSCRIPTION_REPLAY_ACCEPTED_STATUSES: &[&str] = &["ok", "already subscribed"];
pub(super) const UNAUTHENTICATED_CONNECTION_EPOCH: u64 = u64::MAX;

/// WebSocket client for the Derive JSON-RPC stream.
///
/// Construct with [`Self::new`] (public-only) or [`Self::with_credentials`]
/// when private channels and signed actions are needed. Call [`Self::connect`]
/// before any subscribe call; [`Self::disconnect`] tears the connection down.
#[derive(Debug)]
pub struct DeriveWebSocketClient {
    url: String,
    transport_backend: TransportBackend,
    proxy_url: Option<SecretString>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    connection_epoch: Arc<ArcSwap<AtomicU64>>,
    signal: Arc<AtomicBool>,
    auth_tracker: AuthTracker,
    authenticated_epoch: Arc<AtomicU64>,
    credentials: Option<DeriveWsCredentials>,
    next_id: Arc<AtomicU64>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>>,
    subscriptions: Arc<SubscriptionState>,
    subscription_lock: Arc<tokio::sync::Mutex<()>>,
    task_handle: TaskSlot<()>,
    send_task: Arc<SharedTaskSlot<()>>,
    recovery_task: Arc<SharedTaskSlot<()>>,
    shutdown_errors: Vec<String>,
    request_timeout: Duration,
    conn_id: Arc<ArcSwap<String>>,
    rate_limiter: Arc<WsRateLimiter>,
    socket_control: Option<SocketControl>,
}

/// Cloneable command handle for Derive public market data subscriptions.
#[derive(Debug, Clone)]
pub struct DeriveWebSocketSubscriptionHandle {
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    subscriptions: Arc<SubscriptionState>,
    subscription_lock: Arc<tokio::sync::Mutex<()>>,
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
    auth_tracker: AuthTracker,
    request_timeout: Duration,
    conn_id: Arc<ArcSwap<String>>,
    rate_limiter: Arc<WsRateLimiter>,
}

#[derive(Debug)]
pub(crate) struct MatchingRateLimitReservation {
    method: &'static str,
    quota: RateReservation<MonotonicClock>,
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
        Self::build(url, transport_backend, proxy_url, None, None, None)
    }

    /// Builds a client that will issue `public/login` on connect and replay
    /// it after each reconnect.
    ///
    /// `max_matching_requests_per_second` sets the shared wallet matching
    /// allowance. `max_per_instrument_matching_requests_per_second` sets the
    /// independent token refill rate for perpetuals, spot, and options.
    /// Unset values preserve existing shared settings or use reference defaults.
    /// Clients for the same wallet on the same API host share the first explicit
    /// settings.
    /// See [`crate::common::rate_limit`].
    #[must_use]
    pub fn with_credentials(
        url: Option<String>,
        environment: DeriveEnvironment,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: DeriveWsCredentials,
        max_matching_requests_per_second: Option<u32>,
        max_per_instrument_matching_requests_per_second: Option<u32>,
    ) -> Self {
        let url = url.unwrap_or_else(|| urls::ws_url(environment).to_string());
        Self::build(
            url,
            transport_backend,
            proxy_url,
            Some(credentials),
            max_matching_requests_per_second,
            max_per_instrument_matching_requests_per_second,
        )
    }

    fn build(
        url: String,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        credentials: Option<DeriveWsCredentials>,
        matching_tps: Option<u32>,
        instrument_tps: Option<u32>,
    ) -> Self {
        let connection_mode = Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
            ConnectionMode::Closed as u8,
        ))));
        let connection_epoch = Arc::new(ArcSwap::new(Arc::new(AtomicU64::new(0))));

        // Placeholder channel; replaced by connect() before commands are issued.
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let rate_limiter = Arc::new(DeriveRateLimiter::shared(
            &url,
            credentials
                .as_ref()
                .map(|credentials| credentials.wallet_address.as_str()),
            matching_tps,
            instrument_tps,
        ));

        Self {
            url,
            transport_backend,
            proxy_url: proxy_url.map(SecretString::from),
            connection_mode,
            connection_epoch,
            signal: Arc::new(AtomicBool::new(false)),
            auth_tracker: AuthTracker::new(),
            authenticated_epoch: Arc::new(AtomicU64::new(UNAUTHENTICATED_CONNECTION_EPOCH)),
            credentials,
            next_id: Arc::new(AtomicU64::new(1)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            subscriptions: Arc::new(SubscriptionState::new('.')),
            subscription_lock: Arc::new(tokio::sync::Mutex::new(())),
            task_handle: TaskSlot::new(),
            send_task: Arc::new(SharedTaskSlot::new()),
            recovery_task: Arc::new(SharedTaskSlot::new()),
            shutdown_errors: Vec::new(),
            request_timeout: WS_REQUEST_TIMEOUT,
            conn_id: Arc::new(ArcSwap::from_pointee(UUID4::new().to_string())),
            rate_limiter,
            socket_control: None,
        }
    }

    /// Configures socket state reporting and reconnect control.
    #[must_use]
    pub fn with_socket_control(mut self, control: SocketControl) -> Self {
        self.socket_control = Some(control);
        self
    }

    /// Returns the configured WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sets the per-operation WebSocket timeout (login, subscribe, reads, writes).
    ///
    /// Must be called before `connect()`. Defaults to `WS_REQUEST_TIMEOUT`.
    pub fn set_request_timeout(&mut self, timeout: Duration) {
        self.request_timeout = timeout;
    }

    /// Returns `true` when credentials are configured and the venue has
    /// confirmed the latest `public/login`. Cleared on reconnect.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_tracker.is_authenticated()
            && self.is_active()
            && self.authenticated_epoch.load(Ordering::Acquire)
                == self.connection_epoch.load().load(Ordering::Acquire)
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
        if self.task_handle.is_some()
            || !self.send_task.is_empty()
            || !self.recovery_task.is_empty()
        {
            log::debug!("Tearing down stale Derive WebSocket state before connect");
            self.teardown().await?;
        }

        self.signal.store(false, Ordering::Release);
        let setup_guard = DeriveWebSocketSetupGuard::new(self.shutdown_handle());

        self.authenticated_epoch
            .store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);

        let (message_handler, raw_rx) = channel_message_handler();
        let headers = create_standard_nautilus_headers();

        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat_interval_secs: Some(WS_HEARTBEAT_SECS),
            heartbeat_payload: None,
            connect_timeout_ms: Some(RECONNECT_TIMEOUT.as_millis() as u64),
            reconnect_delay_initial_ms: Some(RECONNECT_BASE_BACKOFF.as_millis() as u64),
            reconnect_delay_max_ms: Some(RECONNECT_MAX_BACKOFF.as_millis() as u64),
            reconnect_backoff_factor: Some(RECONNECT_BACKOFF_FACTOR),
            reconnect_jitter_ms: Some(RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: Some(WS_HEARTBEAT_TIMEOUT.as_secs()),
            idle_timeout_ms: None,
            writer_capacity: None,
            backend: self.transport_backend,
            proxy_url: self
                .proxy_url
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            max_message_size_bytes: None,
            max_frame_size_bytes: None,
        };

        // Rate limiting runs caller-side via `self.rate_limiter` before frames
        // are enqueued, so the network client's own limiter is left unconfigured
        // and never sleeps inside the single feed-handler task.
        let client = WebSocketClient::builder()
            .config(cfg)
            .message_handler(message_handler)
            .maybe_state_sink(self.socket_control.as_ref().map(SocketControl::sink))
            .connect()
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

        let connection_mode = client.connection_mode_atomic();
        let connection_epoch = client.connection_epoch_atomic();
        let reconnect_handle = client.reconnect_handle();
        self.connection_mode.store(Arc::clone(&connection_mode));
        self.connection_epoch.store(Arc::clone(&connection_epoch));
        log::debug!("Derive WebSocket connected: {}", self.url);

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            return Err(DeriveWsError::transport(format!(
                "failed to send SetClient command: {e}",
            )));
        }

        let recovery = SessionRecovery {
            subscription: self.subscription_handle(),
            cmd_tx: cmd_tx.clone(),
            auth_tracker: self.auth_tracker.clone(),
            authenticated_epoch: Arc::clone(&self.authenticated_epoch),
            connection_mode: Arc::clone(&connection_mode),
            connection_epoch: Arc::clone(&connection_epoch),
            credentials: self.credentials.clone(),
            signal: Arc::clone(&self.signal),
        };

        let handler = FeedHandler::new_with_send_task(
            Arc::clone(&self.signal),
            cmd_rx,
            raw_rx,
            Arc::clone(&self.next_id),
            self.auth_tracker.clone(),
            Arc::clone(&self.authenticated_epoch),
            Arc::clone(&self.send_task),
        );

        if let Err(e) = self.task_handle.spawn(run_session_handler(
            handler,
            recovery,
            out_tx,
            Arc::clone(&self.recovery_task),
            Arc::clone(&self.conn_id),
        )) {
            let shutdown_result = self.teardown().await;
            return Err(DeriveWsError::transport(match shutdown_result {
                Ok(()) => format!("failed to start WebSocket handler task: {e}"),
                Err(shutdown_error) => format!(
                    "failed to start WebSocket handler task: {e}; startup rollback failed: \
                     {shutdown_error}"
                ),
            }));
        }

        if let Some(control) = &self.socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        if let Some(creds) = self.credentials.clone()
            && let Err(e) = login_via_handler(
                &self.rate_limiter,
                &cmd_tx,
                &self.auth_tracker,
                &self.authenticated_epoch,
                &connection_mode,
                &connection_epoch,
                &creds,
                self.request_timeout,
            )
            .await
        {
            // Without teardown, a retry connect() would short-circuit on
            // is_active() and return Ok without a valid session.
            log::warn!("Derive WebSocket login failed; tearing down transport: {e}");
            self.teardown().await?;
            return Err(e);
        }

        setup_guard.disarm();
        Ok(())
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown_handle().begin_shutdown();
    }

    pub(crate) fn shutdown_handle(&self) -> DeriveWebSocketShutdownHandle {
        DeriveWebSocketShutdownHandle {
            signal: Arc::clone(&self.signal),
            recovery_task: Arc::clone(&self.recovery_task),
        }
    }

    /// Signals the handler to disconnect, aborts the spawn task, and resets
    /// the client's transport-related state. Shared by [`Self::disconnect`]
    /// and the login-failure branch of [`Self::connect`].
    async fn teardown(&mut self) -> Result<()> {
        self.begin_shutdown();

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!(
                "Failed to enqueue Disconnect command (handler may already be shut down): {e}",
            );
        }

        match finish_task(
            &mut self.task_handle,
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .await
        {
            None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => {}
            Some(TaskJoinOutcome::Failed(error)) => self
                .shutdown_errors
                .push(format!("WebSocket handler task failed: {error}")),
            Some(TaskJoinOutcome::Incomplete) => self
                .shutdown_errors
                .push("WebSocket handler task did not stop after abort".to_string()),
        }

        if self.task_handle.is_some() {
            if let Some(control) = &self.socket_control {
                control.deregister();
            }

            return self.take_shutdown_result();
        }

        for (name, slot) in [
            ("recovery", Arc::clone(&self.recovery_task)),
            ("send", Arc::clone(&self.send_task)),
        ] {
            match slot
                .finish(Duration::from_secs(1), Duration::from_secs(2))
                .await
            {
                None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => {}
                Some(TaskJoinOutcome::Failed(error)) => self
                    .shutdown_errors
                    .push(format!("WebSocket {name} worker failed: {error}")),
                Some(TaskJoinOutcome::Incomplete) => self
                    .shutdown_errors
                    .push(format!("WebSocket {name} worker did not stop after abort")),
            }

            if !slot.is_empty() {
                if let Some(control) = &self.socket_control {
                    control.deregister();
                }

                return self.take_shutdown_result();
            }
        }

        // Subscriptions are also dropped: the venue session ended with the
        // transport, so a fresh connect() must re-issue them.
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();
        *self.cmd_tx.write().await = placeholder_tx;
        self.out_rx = None;
        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));
        self.connection_epoch.store(Arc::new(AtomicU64::new(0)));
        self.auth_tracker.invalidate();
        self.authenticated_epoch
            .store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);
        self.subscriptions.clear();
        self.signal.store(false, Ordering::Relaxed);

        if let Some(control) = &self.socket_control {
            control.deregister();
        }

        self.take_shutdown_result()
    }

    fn take_shutdown_result(&mut self) -> Result<()> {
        if self.shutdown_errors.is_empty() {
            Ok(())
        } else {
            Err(DeriveWsError::transport(
                std::mem::take(&mut self.shutdown_errors).join("; "),
            ))
        }
    }

    /// Disconnects the WebSocket connection and awaits the handler task.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::Transport`] when the disconnect command
    /// cannot be enqueued; the handler still tears down on signal.
    pub async fn disconnect(&mut self) -> Result<()> {
        log::debug!("Disconnecting Derive WebSocket");
        self.begin_shutdown();
        self.teardown().await
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
            subscription_lock: Arc::clone(&self.subscription_lock),
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
            auth_tracker: self.auth_tracker.clone(),
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

impl Drop for DeriveWebSocketClient {
    fn drop(&mut self) {
        self.begin_shutdown();

        if let Some(handle) = self.task_handle.as_ref() {
            handle.abort();
        }

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
    }
}

struct SessionRecovery {
    subscription: DeriveWebSocketSubscriptionHandle,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    auth_tracker: AuthTracker,
    authenticated_epoch: Arc<AtomicU64>,
    connection_mode: Arc<AtomicU8>,
    connection_epoch: Arc<AtomicU64>,
    credentials: Option<DeriveWsCredentials>,
    signal: Arc<AtomicBool>,
}

async fn run_session_handler(
    mut handler: FeedHandler,
    recovery: SessionRecovery,
    out_tx: tokio::sync::mpsc::UnboundedSender<DeriveWsMessage>,
    recovery_task: Arc<SharedTaskSlot<()>>,
    conn_id: Arc<ArcSwap<String>>,
) {
    let (recovery_tx, recovery_rx) = tokio::sync::mpsc::unbounded_channel();
    let cmd_tx = recovery.cmd_tx.clone();
    let connection_epoch = Arc::clone(&recovery.connection_epoch);
    if let Err(e) = recovery_task.spawn(recovery.run(recovery_rx, out_tx.clone())) {
        let _ = out_tx.send(DeriveWsMessage::SessionRecoveryFailed(format!(
            "failed to start recovery worker: {e}"
        )));
        let _ = cmd_tx.send(HandlerCommand::Disconnect);
        return;
    }

    loop {
        match handler.next().await {
            Some(DeriveWsMessage::Reconnected) => {
                log::info!("Derive WebSocket re-establishing session after reconnect");
                conn_id.store(Arc::new(UUID4::new().to_string()));
                let epoch = connection_epoch.load(Ordering::Acquire);
                if recovery_tx.send(epoch).is_err() {
                    log::error!("Derive WebSocket recovery task stopped unexpectedly");
                    let _ = cmd_tx.send(HandlerCommand::Disconnect);
                }
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

    recovery_task.abort();
}

impl SessionRecovery {
    async fn run(
        self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<u64>,
        out_tx: tokio::sync::mpsc::UnboundedSender<DeriveWsMessage>,
    ) {
        if self.signal.load(Ordering::Acquire) {
            return;
        }

        while let Some(mut requested_epoch) = rx.recv().await {
            take_latest_epoch(&mut rx, &mut requested_epoch);
            match self.recover_until_current(&mut rx, requested_epoch).await {
                Ok(()) => {
                    if out_tx.send(DeriveWsMessage::Reconnected).is_err() {
                        log::debug!("Derive outer receiver dropped during recovery");
                    }
                }
                Err(e) => {
                    log::error!("Derive WebSocket session recovery failed: {e}");
                    let _ = out_tx.send(DeriveWsMessage::SessionRecoveryFailed(e.to_string()));
                    let _ = self.cmd_tx.send(HandlerCommand::Disconnect);
                    return;
                }
            }
        }
    }

    async fn recover_until_current(
        &self,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<u64>,
        mut requested_epoch: u64,
    ) -> Result<()> {
        loop {
            match recover_session(
                &self.subscription.rate_limiter,
                &self.cmd_tx,
                &self.auth_tracker,
                &self.authenticated_epoch,
                &self.connection_mode,
                &self.connection_epoch,
                self.credentials.as_ref(),
                &self.subscription.subscriptions,
                &self.subscription.subscription_lock,
                self.subscription.request_timeout,
            )
            .await
            {
                Ok(recovered_epoch) => {
                    take_latest_epoch(rx, &mut requested_epoch);
                    if requested_epoch > recovered_epoch
                        || !session_connection_is_active(
                            &self.connection_mode,
                            &self.connection_epoch,
                            recovered_epoch,
                        )
                    {
                        continue;
                    }

                    return Ok(());
                }
                Err(e) => {
                    if take_latest_epoch(rx, &mut requested_epoch) {
                        continue;
                    }

                    return Err(e);
                }
            }
        }
    }
}

fn take_latest_epoch(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<u64>,
    requested_epoch: &mut u64,
) -> bool {
    let mut received = false;

    while let Ok(epoch) = rx.try_recv() {
        *requested_epoch = (*requested_epoch).max(epoch);
        received = true;
    }

    received
}

#[derive(Clone, Debug)]
pub(crate) struct DeriveWebSocketShutdownHandle {
    signal: Arc<AtomicBool>,
    recovery_task: Arc<SharedTaskSlot<()>>,
}

impl DeriveWebSocketShutdownHandle {
    pub(crate) fn begin_shutdown(&self) {
        self.signal.store(true, Ordering::Release);
        self.recovery_task.abort();
    }
}

struct DeriveWebSocketSetupGuard {
    shutdown: DeriveWebSocketShutdownHandle,
    armed: bool,
}

impl DeriveWebSocketSetupGuard {
    fn new(shutdown: DeriveWebSocketShutdownHandle) -> Self {
        Self {
            shutdown,
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for DeriveWebSocketSetupGuard {
    fn drop(&mut self) {
        if self.armed {
            self.shutdown.begin_shutdown();
        }
    }
}

impl DeriveWebSocketSubscriptionHandle {
    pub(crate) fn has_subscription(&self, channel: &str) -> bool {
        let (name, symbol) = channel.split_once('.').unwrap_or((channel, ""));
        self.subscriptions
            .is_subscribed(&Ustr::from(name), &Ustr::from(symbol))
    }

    pub(crate) fn forget_subscription(&self, channel: &str) {
        self.subscriptions.mark_unsubscribe(channel);
        self.subscriptions.confirm_unsubscribe(channel);
    }

    pub(crate) fn remember_subscription(&self, channel: &str) {
        self.subscriptions.mark_subscribe(channel);
    }

    /// Subscribes to `ticker_slim.{instrument_name}.{interval}`.
    ///
    /// # Errors
    ///
    /// Propagates JSON-RPC errors raised by the venue and transport-level
    /// failures.
    pub async fn subscribe_ticker(&self, instrument_name: &str, interval: &str) -> Result<()> {
        let params = ticker_subscribe_params(instrument_name, interval);
        self.subscribe_channels(params.channels).await
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
        let params = orderbook_subscribe_params(instrument_name, group, depth);
        self.subscribe_channels(params.channels).await
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
        let params = trades_subscribe_params(instrument_type, currency);
        self.subscribe_channels(params.channels).await
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

        let _guard = self.subscription_lock.lock().await;
        let params = WsSubscribeParams { channels };
        for topic in channel_topics(&params.channels) {
            self.subscriptions.mark_subscribe(&topic);
        }

        let cmd_tx = self.cmd_tx.read().await.clone();
        let result: WsSubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_SUBSCRIBE,
            &params,
            self.request_timeout,
        )
        .await?;

        subscription_outcome(
            &params.channels,
            &result,
            SUBSCRIPTION_ACCEPTED_STATUSES,
            &self.subscriptions,
        )
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

        let _guard = self.subscription_lock.lock().await;

        for topic in channel_topics(&channels) {
            self.subscriptions.mark_unsubscribe(&topic);
        }

        let params = WsUnsubscribeParams { channels };
        let cmd_tx = self.cmd_tx.read().await.clone();
        let result: WsUnsubscribeResult = send_request(
            &self.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_UNSUBSCRIBE,
            &params,
            self.request_timeout,
        )
        .await?;

        unsubscribe_outcome(&params.channels, &result, &self.subscriptions)
    }

    async fn send_unsubscribe(&self, channel: String) -> Result<()> {
        self.unsubscribe_channels(vec![DeriveWsChannel::from(channel)])
            .await
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
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections or local
    /// throttling before dispatch, and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn submit_order(&self, params: &DeriveOrderParams) -> Result<DeriveOrder> {
        let reservation = self
            .reserve_matching_request(methods::PRIVATE_ORDER, &params.instrument_name)
            .await?;
        self.submit_order_after_rate_limit(params, reservation)
            .await
            .map(|result| result.order)
    }

    pub(crate) async fn submit_order_after_rate_limit(
        &self,
        params: &DeriveOrderParams,
        reservation: MatchingRateLimitReservation,
    ) -> Result<DeriveOrderResult> {
        self.ensure_authenticated(methods::PRIVATE_ORDER)?;
        debug_assert_eq!(reservation.method, methods::PRIVATE_ORDER);
        let cmd_tx = self.cmd_tx.read().await.clone();
        self.ensure_authenticated(reservation.method)?;
        self.commit_matching_reservation(reservation)?;
        let result: DeriveOrderResult = send_request_typed_after_rate_limit(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_ORDER,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(result)
    }

    /// Submits a signed trigger order via `private/order`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections or local
    /// throttling before dispatch, and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn submit_trigger_order(
        &self,
        params: &DeriveTriggerOrderParams,
    ) -> Result<DeriveOrder> {
        let reservation = self
            .reserve_matching_request(
                methods::PRIVATE_TRIGGER_ORDER,
                &params.order.instrument_name,
            )
            .await?;
        self.submit_trigger_order_after_rate_limit(params, reservation)
            .await
            .map(|result| result.order)
    }

    pub(crate) async fn submit_trigger_order_after_rate_limit(
        &self,
        params: &DeriveTriggerOrderParams,
        reservation: MatchingRateLimitReservation,
    ) -> Result<DeriveOrderResult> {
        self.submit_order_after_rate_limit(&params.order, reservation)
            .await
    }

    /// Modifies a working order by cancelling it and submitting a replacement
    /// through the venue's `private/replace`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections or local
    /// throttling before dispatch, and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn modify_order(&self, params: &DeriveReplaceParams) -> Result<DeriveReplaceOutcome> {
        let reservation = self
            .reserve_matching_request(methods::PRIVATE_REPLACE, &params.order.instrument_name)
            .await?;
        self.modify_order_after_rate_limit(params, reservation)
            .await
            .and_then(|response| {
                response
                    .result
                    .and_then(|result| {
                        result.into_outcome(
                            &params.order_id_to_cancel,
                            &params.order.label,
                            params.order.envelope.subaccount_id,
                            params.order.instrument_name.as_str(),
                        )
                    })
                    .map_err(|message| {
                        DeriveWsError::Serde(<serde_json::Error as serde::de::Error>::custom(
                            message,
                        ))
                    })
            })
    }

    pub(crate) async fn modify_order_after_rate_limit(
        &self,
        params: &DeriveReplaceParams,
        reservation: MatchingRateLimitReservation,
    ) -> Result<DeriveReplaceResponse> {
        self.ensure_authenticated(methods::PRIVATE_REPLACE)?;
        debug_assert_eq!(reservation.method, methods::PRIVATE_REPLACE);
        let cmd_tx = self.cmd_tx.read().await.clone();
        self.ensure_authenticated(reservation.method)?;
        self.commit_matching_reservation(reservation)?;
        let response = send_raw_after_rate_limit(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_REPLACE,
            params,
            self.request_timeout,
        )
        .await?;
        Ok(DeriveReplaceResponse::from_raw(&response))
    }

    /// Cancels a single order via `private/cancel`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_order(&self, params: &DeriveCancelParams) -> Result<()> {
        self.require_authenticated(methods::PRIVATE_CANCEL).await?;
        let cmd_tx = self.cmd_tx.read().await.clone();
        let _: DeriveEmptyResult = send_request_for_instrument(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL,
            params,
            self.request_timeout,
            params.instrument_name,
        )
        .await?;
        Ok(())
    }

    /// Cancels every open order for one instrument via `private/cancel_by_instrument`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_by_instrument(
        &self,
        params: &DeriveCancelByInstrumentParams,
    ) -> Result<DeriveCancelByInstrumentResult> {
        self.require_authenticated(methods::PRIVATE_CANCEL_BY_INSTRUMENT)
            .await?;
        let cmd_tx = self.cmd_tx.read().await.clone();
        send_request_typed_for_instrument(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL_BY_INSTRUMENT,
            params,
            self.request_timeout,
            params.instrument_name,
        )
        .await
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
        self.require_authenticated(methods::PRIVATE_CANCEL_TRIGGER_ORDER)
            .await?;
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

    pub(crate) async fn cancel_trigger_order_current(
        &self,
        trigger: &DeriveCancelTriggerOrderParams,
        regular: &DeriveCancelParams,
        is_active: impl Fn() -> bool,
    ) -> Result<Option<DeriveOrder>> {
        self.require_authenticated(methods::PRIVATE_CANCEL_TRIGGER_ORDER)
            .await?;
        self.rate_limiter
            .await_class_ready(RateClass::NonMatching, None)
            .await;
        let cmd_tx = self.cmd_tx.read().await.clone();

        if is_active() {
            return self.cancel_order(regular).await.map(|()| None);
        }

        self.ensure_authenticated(methods::PRIVATE_CANCEL_TRIGGER_ORDER)?;
        send_request_typed_after_rate_limit(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL_TRIGGER_ORDER,
            trigger,
            self.request_timeout,
        )
        .await
        .map(Some)
    }

    /// Cancels every open order with the given label via
    /// `private/cancel_by_label`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveWsError::JsonRpc`] for venue rejections and
    /// [`DeriveWsError::Transport`] / [`DeriveWsError::Timeout`] when the
    /// outcome is ambiguous.
    pub async fn cancel_by_label(
        &self,
        params: &DeriveCancelByLabelParams,
    ) -> Result<DeriveCancelByLabelResult> {
        self.require_authenticated(methods::PRIVATE_CANCEL_BY_LABEL)
            .await?;
        let cmd_tx = self.cmd_tx.read().await.clone();
        send_request_typed(
            &self.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL_BY_LABEL,
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
        self.require_authenticated(methods::PRIVATE_GET_TRIGGER_ORDERS)
            .await?;
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
        self.require_authenticated(methods::PRIVATE_CANCEL_ALL)
            .await?;
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

    pub(crate) async fn reserve_matching_request(
        &self,
        operation: &'static str,
        instrument_name: &Ustr,
    ) -> Result<MatchingRateLimitReservation> {
        self.require_authenticated(operation).await?;
        debug_assert_eq!(rate_class_for_method(operation), RateClass::Matching);
        let quota = self
            .rate_limiter
            .reserve_matching_ready(instrument_name)
            .await;
        self.ensure_authenticated(operation)?;
        Ok(MatchingRateLimitReservation {
            method: operation,
            quota,
        })
    }

    fn commit_matching_reservation(&self, reservation: MatchingRateLimitReservation) -> Result<()> {
        reservation
            .quota
            .commit()
            .map_err(|retry_after| DeriveWsError::RateLimited { retry_after })
    }

    fn ensure_authenticated(&self, operation: &'static str) -> Result<()> {
        if self.auth_tracker.is_authenticated() {
            return Ok(());
        }

        Err(DeriveWsError::Authentication {
            operation: operation.to_string(),
            reason: "WebSocket session is not authenticated".to_string(),
        })
    }

    async fn require_authenticated(&self, operation: &'static str) -> Result<()> {
        if self
            .auth_tracker
            .wait_for_authenticated(self.request_timeout)
            .await
        {
            return Ok(());
        }

        Err(DeriveWsError::Authentication {
            operation: operation.to_string(),
            reason: "WebSocket session is not authenticated".to_string(),
        })
    }
}

// Awaits the venue's raw `result`, bounded by `timeout`. A dropped responder
// (handler torn down on reconnect) surfaces as `RequestCancelled`, a timeout as
// `Timeout`; both leave a state-changing write's outcome ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestRateLimit {
    /// Pace the request now, against the class buckets plus the carried
    /// instrument's per-instrument bucket when present.
    Await(Option<Ustr>),
    /// A matching reservation already consumed the cells; do not pace or
    /// consume again.
    Reserved,
}

async fn send_raw<P>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
) -> Result<Box<RawValue>>
where
    P: Serialize + ?Sized,
{
    send_raw_with_rate_limit(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        RequestRateLimit::Await(None),
        None,
    )
    .await
}

// Awaits the venue's raw `result` for a matching write that carries an
// instrument, pacing it against the account-wide and per-instrument buckets.
async fn send_raw_for_instrument<P>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
    instrument_name: Ustr,
) -> Result<Box<RawValue>>
where
    P: Serialize + ?Sized,
{
    send_raw_with_rate_limit(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        RequestRateLimit::Await(Some(instrument_name)),
        None,
    )
    .await
}

async fn send_raw_after_rate_limit<P>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
) -> Result<Box<RawValue>>
where
    P: Serialize + ?Sized,
{
    send_raw_with_rate_limit(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        RequestRateLimit::Reserved,
        None,
    )
    .await
}

async fn send_raw_with_rate_limit<P>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
    rate_limit: RequestRateLimit,
    connection_epoch: Option<u64>,
) -> Result<Box<RawValue>>
where
    P: Serialize + ?Sized,
{
    let params = SecretString::from(serde_json::to_string(params)?);

    if let RequestRateLimit::Await(instrument_name) = rate_limit {
        rate_limiter
            .await_class_ready(rate_class_for_method(method), instrument_name.as_ref())
            .await;
    }

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(HandlerCommand::Request {
            method,
            params,
            connection_epoch,
            response_tx,
        })
        .map_err(|_| DeriveWsError::NotConnected)?;

    // On timeout the handler's `pending` entry leaks until the next reconnect's
    // `fail_pending` drains it; the later send to the dropped receiver is a
    // no-op logged at debug.
    match time::timeout(timeout, response_rx).await {
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
// subscribe/unsubscribe and the cancel family's `DeriveEmptyResult`).
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
    decode_default_result(&value)
}

// Same as `send_request` for a matching write that carries an instrument, so
// the venue's per-instrument allowance is paced alongside the global one.
async fn send_request_for_instrument<P, R>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
    instrument_name: Ustr,
) -> Result<R>
where
    P: Serialize + ?Sized,
    R: Default + DeserializeOwned,
{
    let value = send_raw_for_instrument(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        instrument_name,
    )
    .await?;
    decode_default_result(&value)
}

// Keep strict result decoding while reserving both matching buckets
async fn send_request_typed_for_instrument<P, R>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
    instrument_name: Ustr,
) -> Result<R>
where
    P: Serialize + ?Sized,
    R: DeserializeOwned,
{
    let value = send_raw_for_instrument(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        instrument_name,
    )
    .await?;
    Ok(serde_json::from_str(value.get())?)
}

fn decode_default_result<R>(value: &RawValue) -> Result<R>
where
    R: Default + DeserializeOwned,
{
    if value.get() == "null" {
        Ok(R::default())
    } else {
        Ok(serde_json::from_str(value.get())?)
    }
}

async fn send_request_on_connection<P, R>(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    method: &'static str,
    params: &P,
    timeout: Duration,
    connection_epoch: u64,
) -> Result<R>
where
    P: Serialize + ?Sized,
    R: Default + DeserializeOwned,
{
    let value = send_raw_with_rate_limit(
        rate_limiter,
        cmd_tx,
        method,
        params,
        timeout,
        RequestRateLimit::Await(None),
        Some(connection_epoch),
    )
    .await?;

    decode_default_result(&value)
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
    Ok(serde_json::from_str(value.get())?)
}

async fn send_request_typed_after_rate_limit<P, R>(
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
    let value = send_raw_after_rate_limit(rate_limiter, cmd_tx, method, params, timeout).await?;
    Ok(serde_json::from_str(value.get())?)
}

fn channel_topics(channels: &[DeriveWsChannel]) -> Vec<String> {
    channels.iter().map(ToString::to_string).collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "authentication state is passed explicitly for epoch fencing"
)]
async fn login_via_handler(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    auth_tracker: &AuthTracker,
    authenticated_epoch: &AtomicU64,
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
    creds: &DeriveWsCredentials,
    timeout: Duration,
) -> Result<()> {
    let _receiver = auth_tracker.begin();
    let expected_epoch = connection_epoch.load(Ordering::Acquire);

    match send_login_request(rate_limiter, cmd_tx, creds, timeout, expected_epoch).await {
        Ok(())
            if complete_session_authentication(
                auth_tracker,
                authenticated_epoch,
                connection_mode,
                connection_epoch,
                expected_epoch,
            ) =>
        {
            log::debug!("Derive WebSocket authenticated");

            Ok(())
        }
        Ok(()) => {
            let e = DeriveWsError::transport(
                "connection changed while completing WebSocket authentication",
            );
            authenticated_epoch.store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);
            auth_tracker.fail(e.to_string());
            Err(e)
        }
        Err(e) => {
            authenticated_epoch.store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);
            auth_tracker.fail(e.to_string());
            Err(e)
        }
    }
}

async fn send_login_request(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    creds: &DeriveWsCredentials,
    timeout: Duration,
    connection_epoch: u64,
) -> Result<()> {
    rate_limiter.await_public_ready().await;
    let login = build_ws_login(&creds.wallet_address, &creds.signer)?;

    let params = Zeroizing::new(WsLoginParams {
        wallet: login.wallet,
        timestamp: login.timestamp,
        signature: login.signature,
    });

    let value = send_raw_with_rate_limit(
        rate_limiter,
        cmd_tx,
        methods::PUBLIC_LOGIN,
        &*params,
        timeout,
        RequestRateLimit::Reserved,
        Some(connection_epoch),
    )
    .await?;

    let _: WsLoginResult =
        serde_json::from_str(value.get()).map_err(|e| DeriveWsError::Authentication {
            operation: methods::PUBLIC_LOGIN.to_string(),
            reason: format!("invalid login result: {e}"),
        })?;

    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "recovery state is passed explicitly for epoch fencing"
)]
async fn recover_session(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    auth_tracker: &AuthTracker,
    authenticated_epoch: &AtomicU64,
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
    creds: Option<&DeriveWsCredentials>,
    subscriptions: &SubscriptionState,
    subscription_lock: &tokio::sync::Mutex<()>,
    timeout: Duration,
) -> Result<u64> {
    let _guard = subscription_lock.lock().await;
    let mut channels = subscriptions.reset_after_reconnect();
    channels.sort();
    let _receiver = creds.map(|_| auth_tracker.begin());

    for attempt in 1..=MAX_SESSION_RECOVERY_ATTEMPTS {
        let expected_epoch = wait_for_session_connection(connection_mode, connection_epoch).await?;

        let result = recover_session_attempt(
            rate_limiter,
            cmd_tx,
            connection_mode,
            connection_epoch,
            creds,
            subscriptions,
            channels.clone(),
            timeout,
            expected_epoch,
        )
        .await;

        match result {
            Ok(()) => {
                if creds.is_some()
                    && !complete_session_authentication(
                        auth_tracker,
                        authenticated_epoch,
                        connection_mode,
                        connection_epoch,
                        expected_epoch,
                    )
                {
                    continue;
                }

                if creds.is_some() {
                    log::info!("Derive WebSocket session re-authenticated");
                }

                return Ok(expected_epoch);
            }
            Err(e) if attempt < MAX_SESSION_RECOVERY_ATTEMPTS && should_retry_ws_error(&e) => {
                let multiplier = 1_u32 << (attempt - 1);
                let delay = RECONNECT_BASE_BACKOFF
                    .saturating_mul(multiplier)
                    .min(RECONNECT_MAX_BACKOFF);
                log::warn!(
                    "Derive WebSocket session recovery attempt {attempt}/{MAX_SESSION_RECOVERY_ATTEMPTS} failed: {e}; retrying in {delay:?}",
                );
                time::sleep(delay).await;
            }
            Err(e) => {
                authenticated_epoch.store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);

                if creds.is_some() {
                    auth_tracker.fail(e.to_string());
                }

                return Err(e);
            }
        }
    }

    let e = DeriveWsError::transport("WebSocket session changed while recovery completed");
    authenticated_epoch.store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);

    if creds.is_some() {
        auth_tracker.fail(e.to_string());
    }

    Err(e)
}

#[expect(
    clippy::too_many_arguments,
    reason = "a session attempt retains its fixed connection epoch and native replay context"
)]
async fn recover_session_attempt(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
    creds: Option<&DeriveWsCredentials>,
    subscriptions: &SubscriptionState,
    channels: Vec<String>,
    timeout: Duration,
    expected_epoch: u64,
) -> Result<()> {
    if !session_connection_is_active(connection_mode, connection_epoch, expected_epoch) {
        return Err(DeriveWsError::transport(
            "connection changed before WebSocket session recovery",
        ));
    }

    if let Some(creds) = creds {
        send_login_request(rate_limiter, cmd_tx, creds, timeout, expected_epoch).await?;
    }

    subscribe_via_handler(
        rate_limiter,
        cmd_tx,
        channels,
        timeout,
        expected_epoch,
        subscriptions,
    )
    .await?;

    if !session_connection_is_active(connection_mode, connection_epoch, expected_epoch) {
        return Err(DeriveWsError::transport(
            "connection changed during WebSocket session recovery",
        ));
    }

    Ok(())
}

async fn wait_for_session_connection(
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
) -> Result<u64> {
    loop {
        match ConnectionMode::from_atomic(connection_mode) {
            ConnectionMode::Active => return Ok(connection_epoch.load(Ordering::Acquire)),
            ConnectionMode::Reconnect => time::sleep(RECONNECT_BASE_BACKOFF).await,
            ConnectionMode::Disconnect | ConnectionMode::Closed => {
                return Err(DeriveWsError::transport(
                    "WebSocket closed during session recovery",
                ));
            }
        }
    }
}

fn complete_session_authentication(
    auth_tracker: &AuthTracker,
    authenticated_epoch: &AtomicU64,
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
    expected_epoch: u64,
) -> bool {
    if !session_connection_is_active(connection_mode, connection_epoch, expected_epoch) {
        return false;
    }

    authenticated_epoch.store(expected_epoch, Ordering::Release);
    auth_tracker.succeed();

    if session_connection_is_active(connection_mode, connection_epoch, expected_epoch) {
        true
    } else {
        authenticated_epoch.store(UNAUTHENTICATED_CONNECTION_EPOCH, Ordering::Release);
        auth_tracker.invalidate();
        false
    }
}

fn session_connection_is_active(
    connection_mode: &AtomicU8,
    connection_epoch: &AtomicU64,
    expected_epoch: u64,
) -> bool {
    ConnectionMode::from_atomic(connection_mode).is_active()
        && connection_epoch.load(Ordering::Acquire) == expected_epoch
}

async fn subscribe_via_handler(
    rate_limiter: &WsRateLimiter,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    channels: Vec<String>,
    timeout: Duration,
    connection_epoch: u64,
    subscriptions: &SubscriptionState,
) -> Result<()> {
    if channels.is_empty() {
        return Ok(());
    }

    let params = WsSubscribeParams {
        channels: channels.into_iter().map(DeriveWsChannel::from).collect(),
    };

    let result: WsSubscribeResult = send_request_on_connection(
        rate_limiter,
        cmd_tx,
        methods::PUBLIC_SUBSCRIBE,
        &params,
        timeout,
        connection_epoch,
    )
    .await?;

    subscription_outcome(
        &params.channels,
        &result,
        SUBSCRIPTION_REPLAY_ACCEPTED_STATUSES,
        subscriptions,
    )
}

fn subscription_outcome(
    requested: &[DeriveWsChannel],
    result: &WsSubscribeResult,
    accepted_statuses: &[&str],
    subscriptions: &SubscriptionState,
) -> Result<()> {
    let mut failures = Vec::new();

    for channel in requested {
        let topic = channel.to_string();
        match result.status.get(channel) {
            Some(status)
                if accepted_statuses.contains(&status.as_str())
                    && result.channels.contains(channel) =>
            {
                subscriptions.confirm_subscribe(&topic);
            }
            Some(status) if accepted_statuses.contains(&status.as_str()) => {
                subscriptions.mark_failure(&topic);
                failures.push(format!("{topic}: not subscribed"));
            }
            other => {
                subscriptions.mark_failure(&topic);
                let reason = other.map_or("missing channel status", |status| status.as_str());
                failures.push(format!("{topic}: {reason}"));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(DeriveWsError::Subscription {
            details: failures.join(", "),
        })
    }
}

fn unsubscribe_outcome(
    requested: &[DeriveWsChannel],
    result: &WsUnsubscribeResult,
    subscriptions: &SubscriptionState,
) -> Result<()> {
    let mut failures = Vec::new();

    for channel in requested {
        let topic = channel.to_string();
        match result.status.get(channel) {
            Some(status) if status == "ok" && !result.remaining_subscriptions.contains(channel) => {
                subscriptions.confirm_unsubscribe(&topic);
            }
            Some(status) if status == "ok" => failures.push(format!("{topic}: still subscribed")),
            Some(status) => failures.push(format!("{topic}: {status}")),
            None => failures.push(format!("{topic}: missing channel status")),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(DeriveWsError::Subscription {
            details: failures.join(", "),
        })
    }
}

pub(crate) struct DeriveReplaceResponse {
    pub result: std::result::Result<DeriveReplaceResult, String>,
    pub trades: Vec<DeriveTrade>,
}

impl DeriveReplaceResponse {
    fn from_raw(response: &RawValue) -> Self {
        #[derive(Deserialize)]
        struct Trades {
            #[serde(default)]
            trades: Vec<Box<RawValue>>,
        }
        let trades = serde_json::from_str::<Trades>(response.get())
            .map(|response| salvage_rows(response.trades))
            .unwrap_or_default();
        let result = serde_json::from_str(response.get()).map_err(|e| e.to_string());
        Self { result, trades }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, time::SystemTime};

    use axum::{Json, Router, routing::post};
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::{
        common::{
            consts::REST_URL_TESTNET,
            rate_limit::{DERIVE_RATE_WINDOW_SECS, RateLimits, RequestLimiter},
            retry::{is_fatal_ws_error, is_write_outcome_ambiguous_ws},
        },
        http::{DeriveHttpClient, client::DeriveCredentials},
    };

    #[tokio::test]
    async fn test_expired_matching_reservation_has_local_failure_evidence() {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        let handle = client.execution_handle();
        let quota = handle
            .rate_limiter
            .reserve_matching_ready(&Ustr::from("ETH-PERP"))
            .await;
        time::sleep(Duration::from_secs(DERIVE_RATE_WINDOW_SECS)).await;
        for _ in 0..DERIVE_RATE_WINDOW_SECS {
            handle
                .rate_limiter
                .await_class_ready(RateClass::Matching, None)
                .await;
        }

        let reservation = MatchingRateLimitReservation {
            method: methods::PRIVATE_ORDER,
            quota,
        };

        let error = handle.commit_matching_reservation(reservation).unwrap_err();

        assert!(matches!(error, DeriveWsError::RateLimited { .. }));
        assert!(should_retry_ws_error(&error));
        assert!(!is_fatal_ws_error(&error));
        assert!(!is_write_outcome_ambiguous_ws(&error));
    }

    #[rstest]
    fn test_replace_response_preserves_order_with_undecodable_trade() {
        let order: Value = serde_json::from_str(include_str!(
            "../../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let trade: Value = serde_json::from_str(include_str!(
            "../../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        let response = DeriveReplaceResponse::from_raw(
            &serde_json::value::to_raw_value(&serde_json::json!({
                "order": order,
                "trades": [{"trade_id": "bad-row"}, trade],
            }))
            .unwrap(),
        );
        let outcome = response
            .result
            .unwrap()
            .into_outcome("old-order", "alpha-strategy", 42, "ETH-PERP")
            .unwrap();

        let DeriveReplaceOutcome::Replaced(order) = outcome else {
            panic!("expected replaced outcome");
        };

        let trades = response.trades;
        assert_eq!(order.order_id, "abc-123");
        assert_eq!(order.label, "alpha-strategy");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].trade_id, "trade-xyz");
        assert_eq!(trades[0].order_id, "order-abc");
    }

    const SESSION_KEY_HEX: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";

    #[rstest]
    #[case(Value::Null)]
    #[case(serde_json::json!("bad-nonce"))]
    #[case(serde_json::json!("18446744073709551616"))]
    fn test_replace_response_preserves_financial_rows_with_invalid_order_nonce(
        #[case] nonce: Value,
    ) {
        let mut order: Value = serde_json::from_str(include_str!(
            "../../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let trade: DeriveTrade = serde_json::from_str(include_str!(
            "../../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        order["nonce"] = nonce;
        let response = DeriveReplaceResponse::from_raw(
            &serde_json::value::to_raw_value(
                &serde_json::json!({"order": order, "trades": [trade]}),
            )
            .unwrap(),
        );
        assert!(response.result.is_err());
        assert_eq!(
            serde_json::to_value(response.trades).unwrap(),
            serde_json::to_value(vec![trade]).unwrap()
        );
    }

    #[tokio::test]
    async fn test_http_and_websocket_share_matching_consumption() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/private/cancel",
                    post(|| async { Json(serde_json::json!({"result": true})) }),
                ),
            )
            .await
            .unwrap();
        });

        let wallet = "0x000000000000000000000000000000000000ab01";
        let http = DeriveHttpClient::with_credentials(
            format!("http://{address}"),
            DeriveCredentials::new(wallet, SESSION_KEY_HEX).unwrap(),
            None,
            None,
            None,
        )
        .unwrap();
        let ws = DeriveWebSocketClient::with_credentials(
            Some(format!("ws://{address}/ws")),
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
            DeriveWsCredentials::new(wallet.to_ascii_uppercase(), SESSION_KEY_HEX).unwrap(),
            None,
            None,
        );

        for _ in 0..5 {
            let result: bool = http
                .send_private_once("private/cancel", &serde_json::json!({}))
                .await
                .unwrap();
            assert!(result);
        }

        server.abort();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let params = serde_json::json!({});
        let request = send_raw(
            &ws.rate_limiter,
            &cmd_tx,
            methods::PRIVATE_CANCEL,
            &params,
            Duration::from_secs(1),
        );
        tokio::pin!(request);
        assert!(futures_util::poll!(&mut request).is_pending());
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_login_builds_auth_timestamp_after_public_quota_wait() {
        let rate_limiter = RequestLimiter::new(RateLimits::new(None, None), MonotonicClock);
        for _ in 0..25 {
            rate_limiter.await_public_ready().await;
        }

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let captured = tokio::spawn(async move {
            let Some(HandlerCommand::Request {
                method,
                params,
                response_tx,
                ..
            }) = cmd_rx.recv().await
            else {
                panic!("login request must enqueue");
            };

            assert_eq!(method, methods::PUBLIC_LOGIN);
            let params: Value = serde_json::from_str(params.expose_secret()).unwrap();
            let timestamp: u64 = params["timestamp"].as_str().unwrap().parse().unwrap();
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            response_tx
                .send(Ok(serde_json::value::to_raw_value(&serde_json::json!([
                    77
                ]))
                .unwrap()))
                .unwrap();
            now.saturating_sub(u128::from(timestamp))
        });

        let started = tokio::time::Instant::now();
        send_login_request(
            &rate_limiter,
            &cmd_tx,
            &DeriveWsCredentials::new(
                "0x000000000000000000000000000000000000ab02",
                SESSION_KEY_HEX,
            )
            .unwrap(),
            Duration::from_secs(1),
            0,
        )
        .await
        .unwrap();
        assert!(started.elapsed() >= Duration::from_secs(4));
        assert!(
            captured.await.unwrap() < 1_000,
            "login timestamp must remain fresh after the pacing wait"
        );
    }

    #[tokio::test]
    #[ignore = "requires Derive public testnet network access"]
    async fn test_live_testnet_unsubscribe_acknowledgement() {
        let mut ws = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        ws.connect().await.unwrap();
        let cmd_tx = ws.cmd_tx.read().await.clone();
        let params = serde_json::json!({"channels": ["ticker_slim.ETH-PERP.1000"]});
        let subscribe = send_raw(
            &ws.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_SUBSCRIBE,
            &params,
            Duration::from_secs(15),
        )
        .await;
        let unsubscribe = send_raw(
            &ws.rate_limiter,
            &cmd_tx,
            methods::PUBLIC_UNSUBSCRIBE,
            &params,
            Duration::from_secs(15),
        )
        .await;
        ws.disconnect().await.unwrap();
        let subscribe: Value = serde_json::from_str(subscribe.unwrap().get()).unwrap();
        let unsubscribe: Value = serde_json::from_str(unsubscribe.unwrap().get()).unwrap();
        println!("Public testnet subscribe acknowledgement: {subscribe}");
        println!("Public testnet unsubscribe acknowledgement: {unsubscribe}");
        assert_eq!(subscribe["status"]["ticker_slim.ETH-PERP.1000"], "ok");
        assert_eq!(unsubscribe["status"]["ticker_slim.ETH-PERP.1000"], "ok");
        assert_eq!(
            unsubscribe["remaining_subscriptions"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    #[ignore = "requires Derive testnet credentials and network access"]
    async fn test_live_testnet_request_budget_scopes() {
        let wallet = env::var("DERIVE_TESTNET_WALLET_ADDRESS").expect("testnet wallet configured");
        let key = env::var("DERIVE_TESTNET_SESSION_PRIVATE_KEY").expect("testnet key configured");
        let subaccount: u64 = env::var("DERIVE_TESTNET_SUBACCOUNT_ID")
            .expect("testnet subaccount configured")
            .parse()
            .unwrap();
        let http = DeriveHttpClient::with_credentials(
            REST_URL_TESTNET,
            DeriveCredentials::new(&wallet, &key).unwrap(),
            Some(15),
            None,
            None,
        )
        .unwrap();
        let mut ws = DeriveWebSocketClient::with_credentials(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
            DeriveWsCredentials::new(&wallet, &key).unwrap(),
            None,
            None,
        );
        ws.connect()
            .await
            .expect("testnet WebSocket login succeeds");
        let cmd_tx = ws.cmd_tx.read().await.clone();
        let before: Value = http
            .send_private("public/getRateLimits", &serde_json::json!({}))
            .await
            .expect("authenticated HTTP budget query succeeds");
        let ws_limits = send_raw(
            &ws.rate_limiter,
            &cmd_tx,
            "public/getRateLimits",
            &serde_json::json!({}),
            Duration::from_secs(15),
        )
        .await
        .expect("authenticated WebSocket budget query succeeds");
        println!("Testnet authenticated HTTP budgets: {before}");
        println!(
            "Testnet authenticated WebSocket budgets: {}",
            ws_limits.get()
        );
        let label = format!("nautilus-pacing-{}", UUID4::new());

        for scoped in [true, false] {
            let mut params = serde_json::json!({"subaccount_id": subaccount, "label": label});
            if scoped {
                params["instrument_name"] = serde_json::json!("ETH-PERP");
            }

            let result: Value = http
                .send_private_once("private/cancel_by_label", &params)
                .await
                .expect("testnet cancellation of unique empty label succeeds");
            let budgets: Value = http
                .send_private("public/getRateLimits", &serde_json::json!({}))
                .await
                .unwrap();
            println!("Testnet cancel-by-label scoped={scoped}: {result}; budgets={budgets}");
            let ws_budgets = send_raw(
                &ws.rate_limiter,
                &cmd_tx,
                "public/getRateLimits",
                &serde_json::json!({}),
                Duration::from_secs(15),
            )
            .await
            .unwrap();
            println!(
                "Testnet wallet budgets after HTTP scoped={scoped}: {}",
                ws_budgets.get()
            );
        }

        let started = tokio::time::Instant::now();
        let name = Ustr::from("ETH-PERP");
        let params = serde_json::json!({"subaccount_id": subaccount, "label": label, "instrument_name": name});

        for _ in 0..6 {
            let result = send_raw_for_instrument(
                &ws.rate_limiter,
                &cmd_tx,
                methods::PRIVATE_CANCEL_BY_LABEL,
                &params,
                Duration::from_secs(15),
                name,
            )
            .await
            .expect("paced testnet WebSocket scoped cancellation succeeds");
            let result: Value = serde_json::from_str(result.get()).unwrap();
            assert_eq!(result["cancelled_orders"], 0);
        }

        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(1),
            "six same-instrument cancels must exceed the local five-token burst"
        );
        println!(
            "Testnet six scoped WebSocket cancellations pass with local pacing in {elapsed:?}"
        );
        let after = send_raw(
            &ws.rate_limiter,
            &cmd_tx,
            "public/getRateLimits",
            &serde_json::json!({}),
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        println!(
            "Testnet wallet budgets after paced WebSocket requests: {}",
            after.get()
        );
        ws.disconnect().await.unwrap();
        ws.connect()
            .await
            .expect("testnet reconnect authenticates with preserved pacing state");
        ws.disconnect().await.unwrap();
    }

    #[rstest]
    fn test_public_client_defaults_to_environment_url() {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Mainnet,
            TransportBackend::default(),
            None,
        );
        assert_eq!(client.url(), "wss://api.derive.xyz/v3/ws");
        assert!(!client.is_authenticated());
        assert!(!client.is_active());
        assert_eq!(client.subscription_count(), 0);
    }

    #[tokio::test]
    async fn test_execution_auth_barrier_waits_for_authentication() {
        let client = DeriveWebSocketClient::with_credentials(
            None,
            DeriveEnvironment::Mainnet,
            TransportBackend::default(),
            None,
            DeriveWsCredentials::new(
                "0x000000000000000000000000000000000000aaaa",
                "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd",
            )
            .unwrap(),
            None,
            None,
        );
        let execution = client.execution_handle();
        let auth_tracker = execution.auth_tracker.clone();
        let _receiver = auth_tracker.begin();
        let tracker_for_task = auth_tracker.clone();

        get_runtime().spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            tracker_for_task.succeed();
        });

        execution
            .require_authenticated(methods::PRIVATE_ORDER)
            .await
            .expect("barrier should wait for successful authentication");
    }

    #[tokio::test]
    async fn test_execution_auth_barrier_fails_on_terminal_auth_failure() {
        let client = DeriveWebSocketClient::with_credentials(
            None,
            DeriveEnvironment::Mainnet,
            TransportBackend::default(),
            None,
            DeriveWsCredentials::new(
                "0x000000000000000000000000000000000000aaaa",
                "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd",
            )
            .unwrap(),
            None,
            None,
        );
        let execution = client.execution_handle();
        let _receiver = execution.auth_tracker.begin();
        execution.auth_tracker.fail("bad signature");

        let error = execution
            .require_authenticated(methods::PRIVATE_ORDER)
            .await
            .expect_err("terminal auth failure must reject private operations");

        assert!(matches!(error, DeriveWsError::Authentication { .. }));
    }

    #[tokio::test]
    async fn test_session_recovery_waits_for_reconnecting_transport() {
        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Reconnect as u8));
        let connection_epoch = Arc::new(AtomicU64::new(1));
        let mode_for_task = Arc::clone(&connection_mode);
        let epoch_for_task = Arc::clone(&connection_epoch);

        get_runtime().spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            epoch_for_task.store(2, Ordering::Release);
            mode_for_task.store(ConnectionMode::Active as u8, Ordering::Release);
        });

        let epoch = tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_session_connection(&connection_mode, &connection_epoch),
        )
        .await
        .expect("session recovery should resume after reconnect")
        .expect("active replacement connection should be accepted");

        assert_eq!(epoch, 2);
    }

    #[rstest]
    fn test_testnet_client_routes_to_v3_url() {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        assert_eq!(client.url(), "wss://testnet.api.derive.xyz/v3/ws");
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
            None,
        );
        assert_eq!(client.url(), "wss://testnet.api.derive.xyz/v3/ws");
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
    async fn test_send_raw_closed_command_queue_is_definitive() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(cmd_rx);
        let rate_limiter: WsRateLimiter =
            RequestLimiter::new(RateLimits::new(None, None), MonotonicClock);
        let error = send_raw(
            &rate_limiter,
            &cmd_tx,
            methods::PRIVATE_REPLACE,
            &serde_json::json!({}),
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, DeriveWsError::NotConnected));
        assert!(!crate::common::retry::is_write_outcome_ambiguous_ws(&error));
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_raw_times_out_when_no_response_arrives() {
        // Keep the receiver alive so the request enqueues, but never reply: the
        // bounded await must surface a Timeout rather than hang forever.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let rate_limiter: WsRateLimiter =
            RequestLimiter::new(RateLimits::new(None, None), MonotonicClock);
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
                let _ = response_tx.send(Ok(RawValue::NULL.to_owned()));
            }
        });

        let rate_limiter: WsRateLimiter =
            RequestLimiter::new(RateLimits::new(None, None), MonotonicClock);
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

    #[rstest]
    #[case::pending(false)]
    #[case::fired_during_pacing(true)]
    #[tokio::test]
    async fn test_trigger_cancel_selects_endpoint_after_pacing(#[case] fires: bool) {
        let client = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        let mut handle = client.execution_handle();
        handle.auth_tracker.succeed();

        handle.rate_limiter = Arc::new(RequestLimiter::new(
            RateLimits::new(None, None),
            MonotonicClock,
        ));
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        *handle.cmd_tx.write().await = cmd_tx;
        for _ in 0..crate::common::rate_limit::DERIVE_WEBSOCKET_NON_MATCHING_TPS
            * crate::common::rate_limit::DERIVE_RATE_BURST_MULTIPLIER
        {
            handle
                .rate_limiter
                .try_class_ready(RateClass::NonMatching, None)
                .unwrap();
        }

        assert!(
            handle
                .rate_limiter
                .try_class_ready(RateClass::NonMatching, None)
                .is_err()
        );
        let active = AtomicBool::new(false);
        let trigger = DeriveCancelTriggerOrderParams::new(42, "paced-trigger");
        let regular = DeriveCancelParams::new(42, "ETH-PERP", "paced-trigger");
        let cancel = handle
            .cancel_trigger_order_current(&trigger, &regular, || active.load(Ordering::Acquire));
        tokio::pin!(cancel);
        assert!(futures_util::poll!(cancel.as_mut()).is_pending());
        assert!(cmd_rx.try_recv().is_err());
        active.store(fires, Ordering::Release);
        let mut canceled: DeriveOrder = serde_json::from_str(include_str!(
            "../../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        canceled.order_id = "paced-trigger".into();
        canceled.order_status = crate::common::enums::DeriveOrderStatus::Cancelled;
        canceled.filled_amount = rust_decimal::Decimal::ZERO;
        canceled.average_price = rust_decimal::Decimal::ZERO;
        let expected = (!fires).then_some(canceled);

        let response = async {
            let HandlerCommand::Request {
                method,
                params,
                response_tx,
                ..
            } = cmd_rx.recv().await.unwrap()
            else {
                panic!("expected one cancel request")
            };

            let params: Value = serde_json::from_str(params.expose_secret()).unwrap();

            let expected_method = if fires {
                methods::PRIVATE_CANCEL
            } else {
                methods::PRIVATE_CANCEL_TRIGGER_ORDER
            };

            assert_eq!(method, expected_method);
            assert_eq!(params["subaccount_id"], serde_json::json!(42));
            assert_eq!(params["order_id"], serde_json::json!("paced-trigger"));
            assert_eq!(
                params.get("instrument_name"),
                fires
                    .then_some(&regular.instrument_name)
                    .map(|name| serde_json::json!(name))
                    .as_ref()
            );
            response_tx
                .send(Ok(serde_json::value::to_raw_value(&expected).unwrap()))
                .unwrap();
        };

        let (result, ()) = tokio::join!(cancel, response);
        assert_eq!(
            serde_json::to_value(result.unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert!(cmd_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_reserved_send_does_not_wait_for_or_consume_second_quota_cell() {
        let rate_limiter: WsRateLimiter =
            RequestLimiter::new(RateLimits::new(None, None), MonotonicClock);

        for _ in 0..5 {
            rate_limiter
                .try_class_ready(RateClass::Matching, None)
                .expect("reservation consumes the matching window");
        }

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        tokio::spawn(async move {
            if let Some(HandlerCommand::Request { response_tx, .. }) = cmd_rx.recv().await {
                let _ = response_tx.send(Ok(serde_json::value::to_raw_value(
                    &serde_json::json!({"accepted": true}),
                )
                .unwrap()));
            }
        });

        let response = tokio::time::timeout(
            Duration::from_millis(100),
            send_raw_after_rate_limit(
                &rate_limiter,
                &cmd_tx,
                methods::PRIVATE_ORDER,
                &serde_json::json!({}),
                Duration::from_secs(1),
            ),
        )
        .await
        .expect("reserved send must not wait for quota")
        .expect("reserved send succeeds");

        assert_eq!(
            serde_json::from_str::<Value>(response.get()).unwrap(),
            serde_json::json!({"accepted": true}),
        );
        assert!(
            rate_limiter
                .try_class_ready(RateClass::Matching, None)
                .is_err(),
            "reserved send must not consume a second cell",
        );
    }
}

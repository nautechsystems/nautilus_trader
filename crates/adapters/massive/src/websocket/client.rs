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

//! WebSocket client for the Massive streaming API.
//!
//! Manages connection lifecycle, API-key authentication, subscription state,
//! and dispatches parsed Nautilus messages through the [`FeedHandler`].

use std::{
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_live::SocketControl;
use nautilus_model::data::bar::BarType;
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            RECONNECT_BACKOFF_FACTOR, RECONNECT_BASE_BACKOFF, RECONNECT_JITTER_MS,
            RECONNECT_MAX_BACKOFF, RECONNECT_TIMEOUT, WS_DISCONNECT_TIMEOUT, WS_HEARTBEAT_SECS,
        },
        credential::MassiveCredential,
        enums::MassiveWsChannel,
    },
    websocket::handler::{FeedHandler, HandlerCommand, NautilusWsMessage},
};

/// Massive WebSocket connection rate limit.
pub static MASSIVE_WS_CONNECTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(4).expect("non-zero")).expect("valid constant")
});

/// Massive WebSocket message send rate limit.
pub static MASSIVE_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid constant")
});

/// Rate-limit key for subscribe/unsubscribe operations.
pub const MASSIVE_RATE_LIMIT_KEY_SUBSCRIPTION: &str = "subscription";

/// Pre-interned [`MASSIVE_RATE_LIMIT_KEY_SUBSCRIPTION`] slice.
pub static MASSIVE_WS_SUBSCRIPTION_KEYS: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from(MASSIVE_RATE_LIMIT_KEY_SUBSCRIPTION)]);

/// Maximum topics carried in one subscribe/unsubscribe request, to keep
/// individual frames bounded.
const TOPICS_PER_REQUEST: usize = 500;

/// WebSocket client for Massive market data streams.
///
/// Manages connection lifecycle, subscription state, and authentication
/// (the `auth` action must precede any subscription). Spawns a
/// [`FeedHandler`] task that parses raw messages into Nautilus types.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.massive", from_py_object)
)]
pub struct MassiveWebSocketClient {
    url: String,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    bar_types: ahash::AHashMap<String, BarType>,
    subscriptions: SubscriptionState,
    credential: Option<MassiveCredential>,
    bars_timestamp_on_close: bool,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    socket_control: Option<SocketControl>,
}

impl Clone for MassiveWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            connection_mode: Arc::clone(&self.connection_mode),
            signal: Arc::clone(&self.signal),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            bar_types: self.bar_types.clone(),
            subscriptions: self.subscriptions.clone(),
            credential: self.credential.clone(),
            bars_timestamp_on_close: self.bars_timestamp_on_close,
            task_handle: None,
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
            socket_control: self.socket_control.clone(),
        }
    }
}

impl MassiveWebSocketClient {
    /// Creates a new [`MassiveWebSocketClient`].
    pub fn new(
        url: &str,
        credential: Option<MassiveCredential>,
        bars_timestamp_on_close: bool,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();

        Self {
            url: url.to_string(),
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed.as_u8(),
            ))),
            signal: Arc::new(AtomicBool::new(false)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            bar_types: ahash::AHashMap::new(),
            subscriptions: SubscriptionState::new('.'),
            credential,
            bars_timestamp_on_close,
            task_handle: None,
            transport_backend,
            proxy_url,
            socket_control: None,
        }
    }

    /// Configures socket state reporting and reconnect control.
    #[must_use]
    pub fn with_socket_control(mut self, control: SocketControl) -> Self {
        self.socket_control = Some(control);
        self
    }

    /// Establishes the WebSocket connection and spawns the feed handler.
    ///
    /// Authenticates immediately after the transport is established; Massive
    /// rejects subscriptions on unauthenticated connections.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_active() || self.is_reconnecting() {
            log::warn!("WebSocket already connected or reconnecting");
            return Ok(());
        }

        // Clear stop signal from any previous disconnect
        self.signal.store(false, Ordering::Relaxed);

        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat_interval_secs: Some(WS_HEARTBEAT_SECS),
            heartbeat_payload: None,
            connect_timeout_ms: Some(RECONNECT_TIMEOUT.as_millis() as u64),
            reconnect_delay_initial_ms: Some(RECONNECT_BASE_BACKOFF.as_millis() as u64),
            reconnect_delay_max_ms: Some(RECONNECT_MAX_BACKOFF.as_millis() as u64),
            reconnect_backoff_factor: Some(RECONNECT_BACKOFF_FACTOR),
            reconnect_jitter_ms: Some(RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let keyed_quotas = vec![(
            MASSIVE_RATE_LIMIT_KEY_SUBSCRIPTION.to_string(),
            *MASSIVE_WS_SUBSCRIPTION_QUOTA,
        )];

        let client = WebSocketClient::connect_with_state_sink(
            cfg,
            Some(message_handler),
            None,
            keyed_quotas,
            Some(*MASSIVE_WS_CONNECTION_QUOTA),
            self.socket_control.as_ref().map(SocketControl::sink),
        )
        .await?;

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);
        self.connection_mode.store(client.connection_mode_atomic());
        let reconnect_handle = client.reconnect_handle();
        log::debug!("Massive WebSocket connected: {}", self.url);

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            anyhow::bail!("Failed to send SetClient command: {e}");
        }

        if let Some(control) = &self.socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        // Restore bar type registrations from previous session
        for (key, bar_type) in &self.bar_types {
            if let Err(e) = cmd_tx.send(HandlerCommand::AddBarType {
                key: key.clone(),
                bar_type: *bar_type,
            }) {
                log::error!("Failed to restore bar type {key}: {e}");
            }
        }

        // Authenticate, then replay retained subscriptions
        authenticate_and_resubscribe(&self.subscriptions, &self.credential, &cmd_tx);

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let credential = self.credential.clone();
        let cmd_tx_reconnect = cmd_tx.clone();
        let bars_timestamp_on_close = self.bars_timestamp_on_close;

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(signal, cmd_rx, raw_rx, bars_timestamp_on_close);

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        subscriptions.reset_after_reconnect();
                        authenticate_and_resubscribe(
                            &subscriptions,
                            &credential,
                            &cmd_tx_reconnect,
                        );

                        if let Err(e) = out_tx.send(NautilusWsMessage::Reconnected) {
                            log::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    Some(msg) => {
                        if let Err(e) = out_tx.send(msg) {
                            log::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    None => {
                        log::debug!("Feed handler stopped");
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(stream_handle);
        Ok(())
    }

    /// Subscribes to a channel for the given tickers.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be dispatched to the handler.
    pub async fn subscribe(
        &self,
        channel: MassiveWsChannel,
        tickers: &[Ustr],
    ) -> anyhow::Result<()> {
        let topics: Vec<String> = tickers
            .iter()
            .map(|ticker| format!("{}.{ticker}", channel.as_ref()))
            .collect();

        for topic in &topics {
            self.subscriptions.mark_subscribe(topic);
        }

        let cmd_tx = self.cmd_tx.read().await;
        for chunk in topics.chunks(TOPICS_PER_REQUEST) {
            cmd_tx
                .send(HandlerCommand::Subscribe(chunk.to_vec()))
                .map_err(|e| anyhow::anyhow!("Failed to send Subscribe command: {e}"))?;
        }
        Ok(())
    }

    /// Unsubscribes from a channel for the given tickers.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be dispatched to the handler.
    pub async fn unsubscribe(
        &self,
        channel: MassiveWsChannel,
        tickers: &[Ustr],
    ) -> anyhow::Result<()> {
        let topics: Vec<String> = tickers
            .iter()
            .map(|ticker| format!("{}.{ticker}", channel.as_ref()))
            .collect();

        for topic in &topics {
            self.subscriptions.mark_unsubscribe(topic);
        }

        let cmd_tx = self.cmd_tx.read().await;
        for chunk in topics.chunks(TOPICS_PER_REQUEST) {
            cmd_tx
                .send(HandlerCommand::Unsubscribe(chunk.to_vec()))
                .map_err(|e| anyhow::anyhow!("Failed to send Unsubscribe command: {e}"))?;
        }
        Ok(())
    }

    /// Returns the next parsed message from the feed handler.
    pub async fn next_message(&mut self) -> Option<NautilusWsMessage> {
        self.out_rx.as_mut()?.recv().await
    }

    /// Disconnects the WebSocket and stops the feed handler.
    pub async fn disconnect(&mut self) {
        // Send Disconnect command before setting the signal so the handler
        // processes it and calls notify_closed() on the inner WebSocket client
        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send Disconnect command: {e}");
        }
        drop(cmd_tx);

        // Release pairs with the handler's Acquire load; fallback for when
        // the command channel is full or closed.
        self.signal.store(true, Ordering::Release);

        if let Some(handle) = self.task_handle.take() {
            // Capture an abort handle before awaiting so a stuck task can be
            // forcibly stopped on timeout instead of leaking.
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(WS_DISCONNECT_TIMEOUT, handle).await {
                Ok(_) => log::debug!("Feed handler task completed"),
                Err(_) => {
                    log::warn!("Feed handler task did not complete within timeout, aborting");
                    abort_handle.abort();
                }
            }
        }

        // Wait for the inner WebSocket's connection_mode atomic to reach
        // Closed before returning so a subsequent connect() does not observe
        // a stale Active/Reconnect state.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

        loop {
            let mode_ptr = self.connection_mode.load();

            if ConnectionMode::from_u8(mode_ptr.load(Ordering::Relaxed)).is_closed() {
                break;
            }

            if tokio::time::Instant::now() >= deadline {
                log::warn!("Timed out waiting for WebSocket to reach Closed state");
                break;
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
    }

    /// Returns true if the WebSocket connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let mode_ptr = self.connection_mode.load();
        ConnectionMode::from_u8(mode_ptr.load(Ordering::Relaxed)).is_active()
    }

    /// Returns true if the WebSocket is reconnecting after a transport drop.
    #[must_use]
    pub fn is_reconnecting(&self) -> bool {
        let mode_ptr = self.connection_mode.load();
        ConnectionMode::from_u8(mode_ptr.load(Ordering::Relaxed)).is_reconnect()
    }

    /// Returns the subscription state.
    #[must_use]
    pub fn subscriptions(&self) -> &SubscriptionState {
        &self.subscriptions
    }

    /// Takes the output message receiver, leaving `None` in its place.
    ///
    /// Used by the data client to move the receiver into a background
    /// consumption task.
    pub fn take_out_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>> {
        self.out_rx.take()
    }

    /// Registers a bar type locally without notifying the handler.
    ///
    /// Used by the data client to persist registrations on the original
    /// client before cloning for async command dispatch.
    pub fn register_bar_type(&mut self, key: String, bar_type: BarType) {
        self.bar_types.insert(key, bar_type);
    }

    /// Registers a bar type for aggregate parsing, keyed by wire topic
    /// (e.g. `AM.AAPL`).
    pub async fn add_bar_type(&mut self, key: String, bar_type: BarType) {
        self.bar_types.insert(key.clone(), bar_type);

        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::AddBarType { key, bar_type }) {
            log::debug!("Failed to send AddBarType: {e}");
        }
    }

    /// Removes a bar type registration.
    pub async fn remove_bar_type(&mut self, key: &str) {
        self.bar_types.remove(key);

        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::RemoveBarType {
            key: key.to_string(),
        }) {
            log::debug!("Failed to send RemoveBarType: {e}");
        }
    }
}

// Sends the auth action (when a credential is configured) followed by
// subscribe requests for every retained topic. Massive processes messages in
// order, so subscriptions pipelined behind the auth action succeed once
// authentication completes.
fn authenticate_and_resubscribe(
    subscriptions: &SubscriptionState,
    credential: &Option<MassiveCredential>,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
) {
    match credential {
        Some(credential) => {
            if let Err(e) = cmd_tx.send(HandlerCommand::Authenticate(credential.ws_auth_message()))
            {
                log::error!("Failed to send Authenticate command: {e}");
                return;
            }
        }
        None => log::warn!("No API key configured; Massive will reject subscriptions"),
    }

    let topics = subscriptions.all_topics();

    if topics.is_empty() {
        log::debug!("No active subscriptions to restore");
        return;
    }

    log::info!("Resubscribing to {} topics", topics.len());

    for chunk in topics.chunks(TOPICS_PER_REQUEST) {
        if let Err(e) = cmd_tx.send(HandlerCommand::Subscribe(chunk.to_vec())) {
            log::error!("Failed to resubscribe: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn test_client() -> MassiveWebSocketClient {
        MassiveWebSocketClient::new(
            "wss://test",
            Some(MassiveCredential::new("test-key".to_string())),
            true,
            TransportBackend::default(),
            None,
        )
    }

    #[rstest]
    fn test_authenticate_and_resubscribe_sends_auth_first() {
        let client = test_client();
        client.subscriptions.mark_subscribe("T.AAPL");
        client.subscriptions.mark_subscribe("Q.MSFT");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        authenticate_and_resubscribe(&client.subscriptions, &client.credential, &tx);

        let first = rx.try_recv().unwrap();
        assert!(matches!(first, HandlerCommand::Authenticate(_)));

        let second = rx.try_recv().unwrap();
        let HandlerCommand::Subscribe(topics) = second else {
            panic!("expected Subscribe, was {second:?}");
        };
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&"T.AAPL".to_string()));
        assert!(topics.contains(&"Q.MSFT".to_string()));
    }

    #[rstest]
    fn test_authenticate_without_topics_sends_auth_only() {
        let client = test_client();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        authenticate_and_resubscribe(&client.subscriptions, &client.credential, &tx);

        assert!(matches!(
            rx.try_recv().unwrap(),
            HandlerCommand::Authenticate(_)
        ));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_resubscribe_without_credential_still_replays_topics() {
        let client = MassiveWebSocketClient::new(
            "wss://test",
            None,
            true,
            TransportBackend::default(),
            None,
        );
        client.subscriptions.mark_subscribe("T.AAPL");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        authenticate_and_resubscribe(&client.subscriptions, &client.credential, &tx);

        // No Authenticate command; topics still replayed so the venue's
        // rejection surfaces as a status error rather than silence.
        let first = rx.try_recv().unwrap();
        assert!(matches!(first, HandlerCommand::Subscribe(_)));
    }

    #[rstest]
    fn test_client_starts_closed() {
        let client = test_client();
        assert!(!client.is_active());
        assert!(!client.is_reconnecting());
    }

    #[rstest]
    fn test_ws_quotas() {
        assert_eq!(MASSIVE_WS_CONNECTION_QUOTA.burst_size().get(), 4);
        assert_eq!(MASSIVE_WS_SUBSCRIPTION_QUOTA.burst_size().get(), 10);
    }
}

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

//! Provides the WebSocket client for the Polymarket CLOB API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use nautilus_common::live::get_runtime;
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};

use super::{
    handler::{FeedHandler, HandlerCommand},
    messages::PolymarketWsMessage,
};
use crate::common::{
    credential::Credential,
    urls::{clob_ws_market_url, clob_ws_user_url},
};

const POLYMARKET_HEARTBEAT_SECS: u64 = 30;

/// Polymarket WebSocket channel: market data or authenticated user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsChannel {
    Market,
    User,
}

// Market channel streams continuously; user channel can legitimately be quiet
// when no orders or fills exist, so give it a longer window before treating
// silence as a zombie connection.
fn idle_timeout_ms_for(channel: WsChannel) -> u64 {
    match channel {
        WsChannel::Market => 60_000,
        WsChannel::User => 300_000,
    }
}

/// Lightweight handle for subscribing/unsubscribing to market data.
///
/// `Clone` + `Send` safe for use in spawned async tasks.
#[derive(Clone, Debug)]
pub struct WsSubscriptionHandle {
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
}

impl WsSubscriptionHandle {
    /// Sends a market subscribe command to the handler.
    pub async fn subscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeMarket(asset_ids))
            .map_err(|e| anyhow::anyhow!("Failed to send SubscribeMarket: {e}"))
    }

    /// Sends a market unsubscribe command to the handler.
    pub async fn unsubscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeMarket(asset_ids))
            .map_err(|e| anyhow::anyhow!("Failed to send UnsubscribeMarket: {e}"))
    }

    // Constructs a handle around a raw command sender. Test-only: lets unit
    // tests observe the commands the handle emits without spinning up the real
    // feed handler.
    #[cfg(test)]
    pub(crate) fn from_sender(sender: tokio::sync::mpsc::UnboundedSender<HandlerCommand>) -> Self {
        Self {
            cmd_tx: Arc::new(tokio::sync::RwLock::new(sender)),
        }
    }
}

/// Provides a WebSocket client for the Polymarket CLOB API.
///
/// A single instance targets one channel (market or user). Use
/// [`PolymarketWebSocketClient::new_market`] for public market data and
/// [`PolymarketWebSocketClient::new_user`] for authenticated order/trade streams.
#[derive(Debug)]
pub struct PolymarketWebSocketClient {
    channel: WsChannel,
    url: String,
    connection_mode: Arc<AtomicU8>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>>,
    credential: Option<Credential>,
    subscriptions: SubscriptionState,
    auth_tracker: AuthTracker,
    // Survives disconnect() so that connect() can replay a prior subscribe_user() call.
    // Arc<AtomicBool> allows mutation from &self in subscribe_user().
    user_subscribed: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    subscribe_new_markets: bool,
    transport_backend: TransportBackend,
}

impl PolymarketWebSocketClient {
    /// Creates a new market-channel client (unauthenticated).
    ///
    /// If `base_url` is `None`, the default production URL is used.
    #[must_use]
    pub fn new_market(
        base_url: Option<String>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
    ) -> Self {
        let url = base_url.unwrap_or_else(|| clob_ws_market_url().to_string());
        Self::new_inner(
            WsChannel::Market,
            url,
            None,
            subscribe_new_markets,
            transport_backend,
        )
    }

    /// Creates a new user-channel client (authenticated).
    ///
    /// If `base_url` is `None`, the default production URL is used.
    #[must_use]
    pub fn new_user(
        base_url: Option<String>,
        credential: Credential,
        transport_backend: TransportBackend,
    ) -> Self {
        let url = base_url.unwrap_or_else(|| clob_ws_user_url().to_string());
        Self::new_inner(
            WsChannel::User,
            url,
            Some(credential),
            false,
            transport_backend,
        )
    }

    fn new_inner(
        channel: WsChannel,
        url: String,
        credential: Option<Credential>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
    ) -> Self {
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();
        Self {
            channel,
            url,
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Closed.as_u8())),
            signal: Arc::new(AtomicBool::new(false)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            credential,
            subscriptions: SubscriptionState::new(':'),
            auth_tracker: AuthTracker::new(),
            user_subscribed: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscribe_new_markets,
            transport_backend,
        }
    }

    /// Establishes the WebSocket connection and spawns the message handler.
    ///
    /// # Errors
    ///
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let mode = ConnectionMode::from_atomic(&self.connection_mode);
        if mode.is_active() || mode.is_reconnect() {
            log::warn!("Polymarket WebSocket already connected or reconnecting");
            return Ok(());
        }

        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(POLYMARKET_HEARTBEAT_SECS),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
            idle_timeout_ms: Some(idle_timeout_ms_for(self.channel)),
            backend: self.transport_backend,
            proxy_url: None,
        };

        let client =
            WebSocketClient::connect(cfg, Some(message_handler), None, None, vec![], None).await?;

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<PolymarketWsMessage>();

        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);

        let client_mode = client.connection_mode_atomic();
        self.connection_mode = client_mode;

        log::info!("Polymarket WebSocket connected: {}", self.url);

        cmd_tx
            .send(HandlerCommand::SetClient(client))
            .map_err(|e| anyhow::anyhow!("Failed to send SetClient: {e}"))?;

        // Replay retained state onto the new session. Unlike the RECONNECTED sentinel
        // path, a fresh connect() never fires resubscribe_all() inside the handler, so
        // we must queue the commands here before the handler task is even spawned.
        match self.channel {
            WsChannel::Market => {
                let topics = self.subscriptions.all_topics();
                if !topics.is_empty() {
                    log::info!(
                        "Replaying {} market subscription(s) onto new session",
                        topics.len()
                    );
                    cmd_tx
                        .send(HandlerCommand::SubscribeMarket(topics))
                        .map_err(|e| anyhow::anyhow!("Failed to replay SubscribeMarket: {e}"))?;
                }
            }
            WsChannel::User => {
                if self.user_subscribed.load(Ordering::Relaxed) {
                    log::info!("Replaying user subscribe onto new session");
                    cmd_tx
                        .send(HandlerCommand::SubscribeUser)
                        .map_err(|e| anyhow::anyhow!("Failed to replay SubscribeUser: {e}"))?;
                }
            }
        }

        let signal = Arc::clone(&self.signal);
        let channel = self.channel;
        let credential = self.credential.clone();
        let subscriptions = self.subscriptions.clone();
        let auth_tracker = self.auth_tracker.clone();
        let user_subscribed = self.user_subscribed.load(Ordering::Relaxed);
        let subscribe_new_markets = self.subscribe_new_markets;

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal,
                channel,
                cmd_rx,
                raw_rx,
                out_tx,
                credential,
                subscriptions,
                auth_tracker,
                user_subscribed,
                subscribe_new_markets,
            );

            loop {
                match handler.next().await {
                    Some(PolymarketWsMessage::Reconnected) => {
                        log::info!("Polymarket WebSocket reconnected");
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            log::error!("Output channel closed, stopping handler");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            log::debug!("Stop signal received, ending handler task");
                        } else {
                            log::warn!("Polymarket WebSocket stream ended unexpectedly");
                        }
                        break;
                    }
                }
            }
            log::debug!("Polymarket WebSocket handler task completed");
        });
        self.task_handle = Some(stream_handle);
        Ok(())
    }

    /// Force-close fallback for the sync `stop()` path.
    /// Prefer `disconnect()` for graceful shutdown.
    pub(crate) fn abort(&mut self) {
        self.signal.store(true, Ordering::Relaxed);
        self.connection_mode
            .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.auth_tracker.invalidate();
    }

    /// Disconnects the WebSocket connection.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting Polymarket WebSocket");
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send disconnect (handler may already be shut down): {e}");
        }

        if let Some(handle) = self.task_handle.take() {
            let abort_handle = handle.abort_handle();
            tokio::select! {
                result = handle => {
                    match result {
                        Ok(()) => log::debug!("Handler task completed"),
                        Err(e) if e.is_cancelled() => log::debug!("Handler task was cancelled"),
                        Err(e) => log::error!("Handler task error: {e:?}"),
                    }
                }
                () = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    log::warn!("Timeout waiting for handler task, aborting");
                    abort_handle.abort();
                }
            }
        }
        // Invalidate after the task has stopped so any in-flight auth_tracker.succeed()
        // calls from the handler cannot race with and survive the invalidation.
        self.auth_tracker.invalidate();
        log::debug!("Polymarket WebSocket disconnected");
        Ok(())
    }

    /// Returns `true` if the WebSocket is actively connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        ConnectionMode::from_atomic(&self.connection_mode).is_active()
    }

    /// Returns the URL this client connects to.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the number of active market asset subscriptions (pending + confirmed).
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.all_topics().len()
    }

    /// Returns `true` if the user channel has been authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_tracker.is_authenticated()
    }

    /// Subscribe to market data for the given asset IDs.
    ///
    /// Sends a subscribe message immediately if connected; the IDs are also
    /// retained so they are re-sent automatically on reconnect.
    ///
    /// # Errors
    ///
    /// Returns an error if called on a user-channel client (incompatible channel).
    pub async fn subscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        if self.channel != WsChannel::Market {
            anyhow::bail!(
                "subscribe_market() requires a market-channel client (created with new_market())"
            );
        }
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeMarket(asset_ids))
            .map_err(|e| anyhow::anyhow!("Failed to send SubscribeMarket: {e}"))
    }

    /// Remove asset IDs from the active subscription set.
    ///
    /// The IDs are dropped from the reconnect set so they will not be
    /// re-subscribed after a reconnect. No wire message is sent.
    ///
    /// # Errors
    ///
    /// Returns an error if called on a user-channel client (incompatible channel).
    pub async fn unsubscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        if self.channel != WsChannel::Market {
            anyhow::bail!(
                "unsubscribe_market() requires a market-channel client (created with new_market())"
            );
        }
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeMarket(asset_ids))
            .map_err(|e| anyhow::anyhow!("Failed to send UnsubscribeMarket: {e}"))
    }

    /// Authenticate and subscribe to the user channel.
    ///
    /// # Errors
    ///
    /// Returns an error if called on a market-channel client (no credentials available).
    pub async fn subscribe_user(&self) -> anyhow::Result<()> {
        if self.channel != WsChannel::User {
            anyhow::bail!(
                "subscribe_user() requires a user-channel client (created with new_user())"
            );
        }
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeUser)
            .map_err(|e| anyhow::anyhow!("Failed to send SubscribeUser: {e}"))?;
        // Set only after the command is successfully enqueued so a failed send does not
        // leave user_subscribed=true and cause an unintended replay on the next connect().
        self.user_subscribed.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Returns a cloneable subscription handle for use in spawned tasks.
    #[must_use]
    pub fn clone_subscription_handle(&self) -> WsSubscriptionHandle {
        WsSubscriptionHandle {
            cmd_tx: Arc::clone(&self.cmd_tx),
        }
    }

    /// Takes the message receiver, leaving `None` in its place.
    ///
    /// This is useful when the data client needs to spawn its own handler
    /// task that reads messages independently of the WS client.
    /// Subscription methods (`subscribe_market`, etc.) remain usable on `&self`.
    #[must_use]
    pub fn take_message_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>> {
        self.out_rx.take()
    }

    /// Receives the next message from the WebSocket handler.
    ///
    /// Returns `None` when the handler has disconnected or the receiver
    /// was not yet initialized (call `connect` first).
    pub async fn next_message(&mut self) -> Option<PolymarketWsMessage> {
        if let Some(ref mut rx) = self.out_rx {
            rx.recv().await
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{WsChannel, idle_timeout_ms_for};

    #[rstest]
    #[case::market(WsChannel::Market, 60_000)]
    #[case::user(WsChannel::User, 300_000)]
    fn test_idle_timeout_ms_for_channel(#[case] channel: WsChannel, #[case] expected: u64) {
        assert_eq!(idle_timeout_ms_for(channel), expected);
    }
}

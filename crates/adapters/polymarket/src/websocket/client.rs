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

use nautilus_live::{
    SocketControl,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_network::{
    SocketStateSink,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_epoch_message_handler, proxy::ProxyUrl,
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

// The venue counts only the `PING` text frame, not protocol ping frames, and
// closes with `1008 no ping received` otherwise. Cadence per venue docs:
// https://docs.polymarket.com/api-reference/wss/market
pub(super) const POLYMARKET_HEARTBEAT_SECS: u64 = 10;
pub(super) const POLYMARKET_HEARTBEAT_PAYLOAD: &str = "PING";

// Prediction markets go quiet for long stretches, so liveness is the venue
// still sending frames, not data arriving. A data-silence timer cannot serve:
// `PONG` is a text frame and refreshes it. Tear down after three cycles.
const POLYMARKET_HEARTBEAT_TIMEOUT_SECS: u64 = POLYMARKET_HEARTBEAT_SECS * 3;

/// Polymarket WebSocket channel: market data or authenticated user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsChannel {
    Market,
    User,
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
    discovery_subscribed: Arc<AtomicBool>,
    auth_tracker: AuthTracker,
    // Survives disconnect() so that connect() can replay a prior subscribe_user() call.
    // Arc<AtomicBool> allows mutation from &self in subscribe_user().
    user_subscribed: Arc<AtomicBool>,
    task_handle: TaskSlot<()>,
    subscribe_new_markets: bool,
    transport_backend: TransportBackend,
    proxy_url: Option<ProxyUrl>,
    socket_sink: Option<SocketStateSink>,
    socket_control: Option<SocketControl>,
}

#[derive(Clone, Debug)]
pub(crate) struct PolymarketWebSocketShutdownHandle {
    signal: Arc<AtomicBool>,
}

impl PolymarketWebSocketShutdownHandle {
    pub(crate) fn begin_shutdown(&self) {
        self.signal.store(true, Ordering::Relaxed);
    }
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
        Self::new_market_with_proxy(base_url, subscribe_new_markets, transport_backend, None)
    }

    /// Creates a new market-channel client with an optional validated proxy URL.
    #[must_use]
    pub fn new_market_with_proxy(
        base_url: Option<String>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        let url = base_url.unwrap_or_else(|| clob_ws_market_url().to_string());
        Self::new_inner(
            WsChannel::Market,
            url,
            None,
            subscribe_new_markets,
            transport_backend,
            proxy_url,
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
        Self::new_user_with_proxy(base_url, credential, transport_backend, None)
    }

    /// Creates a new user-channel client with an optional validated proxy URL.
    #[must_use]
    pub fn new_user_with_proxy(
        base_url: Option<String>,
        credential: Credential,
        transport_backend: TransportBackend,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        let url = base_url.unwrap_or_else(|| clob_ws_user_url().to_string());
        Self::new_inner(
            WsChannel::User,
            url,
            Some(credential),
            false,
            transport_backend,
            proxy_url,
        )
    }

    fn new_inner(
        channel: WsChannel,
        url: String,
        credential: Option<Credential>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
        proxy_url: Option<ProxyUrl>,
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
            discovery_subscribed: Arc::new(AtomicBool::new(false)),
            auth_tracker: AuthTracker::new(),
            user_subscribed: Arc::new(AtomicBool::new(false)),
            task_handle: TaskSlot::new(),
            subscribe_new_markets,
            transport_backend,
            proxy_url,
            socket_sink: None,
            socket_control: None,
        }
    }

    /// Configures socket state reporting for the underlying transport.
    #[must_use]
    pub fn with_state_sink(mut self, state_sink: SocketStateSink) -> Self {
        self.socket_sink = Some(state_sink);
        self
    }

    /// Configures state reporting and reconnect control for the underlying transport.
    #[must_use]
    pub(crate) fn with_socket_control(mut self, control: SocketControl) -> Self {
        self.socket_control = Some(control);
        self
    }

    #[cfg(test)]
    pub(crate) fn proxy_url(&self) -> Option<&ProxyUrl> {
        self.proxy_url.as_ref()
    }

    /// Establishes the WebSocket connection and spawns the message handler.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let mode = ConnectionMode::from_atomic(&self.connection_mode);
        if mode.is_active() || mode.is_reconnect() {
            log::warn!("Polymarket WebSocket already connected or reconnecting");
            return Ok(());
        }

        if self.task_handle.is_some() {
            self.disconnect().await?;
        }

        let (message_handler, raw_rx) = channel_epoch_message_handler();
        let cfg = self.websocket_config();

        let client = WebSocketClient::epoch_builder()
            .config(cfg)
            .epoch_handler(message_handler)
            .maybe_state_sink(
                self.socket_control
                    .as_ref()
                    .map(SocketControl::sink)
                    .or_else(|| self.socket_sink.clone()),
            )
            .connect()
            .await?;

        if let Some(control) = &self.socket_control {
            let handle = client.reconnect_handle();
            control.register(move || handle.request_reconnect());
        }
        let connection_epoch = client.connection_epoch();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<PolymarketWsMessage>();

        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);

        let client_mode = client.connection_mode_atomic();
        self.connection_mode = client_mode;

        log::debug!("Polymarket WebSocket connected: {}", self.url);

        // Replay retained state onto the new session. Unlike the RECONNECTED sentinel
        // path, a fresh connect() never fires resubscribe_all() inside the handler.
        let initial_market_replay = match self.channel {
            WsChannel::Market => {
                let topics = self.subscriptions.reset_after_reconnect();
                if !topics.is_empty() || self.discovery_subscribed.load(Ordering::Relaxed) {
                    log::debug!(
                        "Replaying market subscription state onto new session: assets={}, discovery={}",
                        topics.len(),
                        self.discovery_subscribed.load(Ordering::Relaxed),
                    );
                    Some((topics, connection_epoch))
                } else {
                    None
                }
            }
            WsChannel::User => {
                if self.user_subscribed.load(Ordering::Relaxed) {
                    log::debug!("Replaying user subscribe onto new session");
                    cmd_tx
                        .send(HandlerCommand::SubscribeUser)
                        .map_err(|e| anyhow::anyhow!("Failed to replay SubscribeUser: {e}"))?;
                }
                None
            }
        };

        let signal = Arc::clone(&self.signal);
        let channel = self.channel;
        let credential = self.credential.clone();
        let subscriptions = self.subscriptions.clone();
        let discovery_subscribed = Arc::clone(&self.discovery_subscribed);
        let auth_tracker = self.auth_tracker.clone();
        let user_subscribed = self.user_subscribed.load(Ordering::Relaxed);
        let subscribe_new_markets = self.subscribe_new_markets;

        if let Err(e) = self.task_handle.spawn(async move {
            let mut handler = FeedHandler::new(
                signal,
                channel,
                Some(client),
                cmd_rx,
                raw_rx,
                out_tx,
                credential,
                subscriptions,
                discovery_subscribed,
                initial_market_replay,
                auth_tracker,
                user_subscribed,
                subscribe_new_markets,
            );

            loop {
                match handler.next().await {
                    Some(PolymarketWsMessage::Reconnected) => {
                        log::info!("Polymarket WebSocket reconnected");

                        if handler.send(PolymarketWsMessage::Reconnected).is_err() {
                            if handler.is_stopped() {
                                log::debug!("Output channel closed, stopping handler");
                            } else {
                                log::error!("Output channel closed, stopping handler");
                            }
                            break;
                        }
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            if handler.is_stopped() {
                                log::debug!("Output channel closed, stopping handler");
                            } else {
                                log::error!("Output channel closed, stopping handler");
                            }
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
        }) {
            self.out_rx = None;
            anyhow::bail!("Failed to start Polymarket WebSocket handler task: {e}");
        }
        Ok(())
    }

    fn websocket_config(&self) -> WebSocketConfig {
        // The market endpoint rejects text PING before its initial subscription. Protocol pings
        // keep an idle socket alive until FeedHandler starts the required text heartbeat.
        let heartbeat_payload = match self.channel {
            WsChannel::Market => None,
            WsChannel::User => Some(POLYMARKET_HEARTBEAT_PAYLOAD.to_string()),
        };

        WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat_interval_secs: Some(POLYMARKET_HEARTBEAT_SECS),
            heartbeat_payload,
            connect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: Some(POLYMARKET_HEARTBEAT_TIMEOUT_SECS),
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.as_ref().map(|url| url.expose().to_string()),
        }
    }

    pub(crate) fn begin_shutdown(&self) {
        self.signal.store(true, Ordering::Relaxed);
    }

    pub(crate) fn shutdown_handle(&self) -> PolymarketWebSocketShutdownHandle {
        PolymarketWebSocketShutdownHandle {
            signal: Arc::clone(&self.signal),
        }
    }

    pub(crate) fn abort(&mut self) {
        self.begin_shutdown();
        self.connection_mode
            .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        self.task_handle.abort();
        self.auth_tracker.invalidate();

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
    }

    /// Disconnects the WebSocket connection.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::debug!("Disconnecting Polymarket WebSocket");
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send disconnect (handler may already be shut down): {e}");
        }

        let task_result = match finish_task(
            &mut self.task_handle,
            std::time::Duration::from_secs(2),
            std::time::Duration::from_secs(2),
        )
        .await
        {
            None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => Ok(()),
            Some(TaskJoinOutcome::Failed(error)) => Err(anyhow::anyhow!(
                "Polymarket WebSocket handler failed: {error}"
            )),
            Some(TaskJoinOutcome::Incomplete) => Err(anyhow::anyhow!(
                "Polymarket WebSocket handler did not stop after abort"
            )),
        };
        // Invalidate after the task has stopped so any in-flight auth_tracker.succeed()
        // calls from the handler cannot race with and survive the invalidation.
        self.auth_tracker.invalidate();

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
        log::debug!("Polymarket WebSocket disconnected");
        task_result
    }

    /// Returns `true` if the WebSocket is actively connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        ConnectionMode::from_atomic(&self.connection_mode).is_active()
    }

    pub(crate) fn has_task(&self) -> bool {
        self.task_handle.is_some()
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

    /// Clears retained subscription/auth replay state.
    ///
    /// Useful for hard resets where the caller wants reconnect to start from a
    /// clean slate rather than replaying a previous generation's topics.
    pub(crate) fn clear_reconnect_state(&self) {
        self.subscriptions.clear();
        self.discovery_subscribed.store(false, Ordering::Relaxed);
        self.user_subscribed.store(false, Ordering::Relaxed);
        self.auth_tracker.invalidate();
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

impl Drop for PolymarketWebSocketClient {
    fn drop(&mut self) {
        self.signal.store(true, Ordering::Relaxed);

        if let Some(handle) = self.task_handle.as_ref() {
            handle.abort();
        }

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        Router,
        extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
        response::Response,
        routing::get,
    };
    use nautilus_network::{
        RECONNECTED,
        websocket::{TransportBackend, WebSocketConfig, proxy::ProxyUrl},
    };
    use rstest::rstest;

    use super::*;

    async fn handle_upgrade(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(handle_socket)
    }

    async fn handle_socket(mut socket: WebSocket) {
        let _ = socket
            .send(AxumWsMessage::Text(RECONNECTED.to_string().into()))
            .await;
    }

    async fn start_test_server() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test websocket server");
        let addr = listener.local_addr().expect("test websocket address");
        let router = Router::new().route("/ws", get(handle_upgrade));

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test websocket server failed");
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        addr
    }

    #[tokio::test]
    async fn cancelled_disconnect_retains_handler_task() {
        let mut client = PolymarketWebSocketClient::new_market(
            Some("ws://127.0.0.1:0".to_string()),
            false,
            TransportBackend::default(),
        );
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        client.cmd_tx = Arc::new(tokio::sync::RwLock::new(cmd_tx));
        client
            .task_handle
            .insert(tokio::spawn(std::future::pending()));

        {
            let disconnect = client.disconnect();
            tokio::pin!(disconnect);
            tokio::select! {
                result = &mut disconnect => panic!("disconnect completed unexpectedly: {result:?}"),
                command = cmd_rx.recv() => assert!(command.is_some()),
            }
        }

        assert!(client.task_handle.is_some());
    }

    #[rstest]
    #[tokio::test]
    async fn connect_forwards_reconnected_message_to_receiver() {
        let addr = start_test_server().await;
        let mut client = PolymarketWebSocketClient::new_market(
            Some(format!("ws://{addr}/ws")),
            false,
            TransportBackend::default(),
        );

        client.connect().await.expect("connect websocket client");

        let message =
            tokio::time::timeout(tokio::time::Duration::from_secs(2), client.next_message())
                .await
                .expect("wait for websocket message");

        assert!(matches!(
            message,
            Some(super::super::messages::PolymarketWsMessage::Reconnected)
        ));

        client
            .disconnect()
            .await
            .expect("disconnect websocket client");
    }

    #[rstest]
    fn proxy_url_is_retained_for_market_and_user_clients() {
        const MARKET_PROXY: &str = "http://market-user:market-proxy-secret@127.0.0.1:18086";
        const USER_PROXY: &str = "https://user-user:user-proxy-secret@127.0.0.1:18087";
        let market = PolymarketWebSocketClient::new_market_with_proxy(
            Some("ws://market.example/ws".to_string()),
            false,
            TransportBackend::Tungstenite,
            Some(ProxyUrl::parse(MARKET_PROXY).unwrap()),
        );
        let credential = crate::common::credential::Credential::new(
            "fixture-key",
            "Zml4dHVyZQ==",
            "fixture-passphrase".to_string(),
        )
        .unwrap();
        let user = PolymarketWebSocketClient::new_user_with_proxy(
            Some("ws://user.example/ws".to_string()),
            credential,
            TransportBackend::Tungstenite,
            Some(ProxyUrl::parse(USER_PROXY).unwrap()),
        );
        let market_config = market.websocket_config();
        let user_config = user.websocket_config();
        let market_debug = format!("{market:?}");
        let user_debug = format!("{user:?}");
        let assert_common = |config: &WebSocketConfig| {
            assert_eq!(config.headers, Vec::<(String, String)>::new());
            assert_eq!(config.heartbeat_interval_secs, Some(10));
            assert_eq!(config.connect_timeout_ms, Some(15_000));
            assert_eq!(config.reconnect_delay_initial_ms, Some(250));
            assert_eq!(config.reconnect_delay_max_ms, Some(5_000));
            assert_eq!(config.reconnect_backoff_factor, Some(2.0));
            assert_eq!(config.reconnect_jitter_ms, Some(200));
            assert_eq!(config.reconnect_max_attempts, None);
            // No data-silence timer: `PONG` arrives as a text frame and would
            // refresh it, so liveness rests on the heartbeat timeout instead.
            assert_eq!(config.idle_timeout_ms, None);
            assert_eq!(config.backend, TransportBackend::Tungstenite);
        };

        assert_eq!(market.proxy_url.as_ref().unwrap().expose(), MARKET_PROXY);
        assert_eq!(user.proxy_url.as_ref().unwrap().expose(), USER_PROXY);
        assert_eq!(market_config.url, "ws://market.example/ws");
        assert_eq!(user_config.url, "ws://user.example/ws");
        assert_eq!(market_config.proxy_url.as_deref(), Some(MARKET_PROXY));
        assert_eq!(user_config.proxy_url.as_deref(), Some(USER_PROXY));
        assert_eq!(market_config.heartbeat_payload, None);
        assert_eq!(user_config.heartbeat_payload.as_deref(), Some("PING"));
        assert_common(&market_config);
        assert_common(&user_config);
        assert!(!market_debug.contains("market-proxy-secret"));
        assert!(!user_debug.contains("user-proxy-secret"));
    }
}

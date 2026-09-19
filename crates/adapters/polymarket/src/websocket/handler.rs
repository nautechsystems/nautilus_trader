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

//! WebSocket message handler for the Polymarket CLOB API.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use nautilus_core::string::secret::SecretString;
use nautilus_live::book::snapshot::SnapshotGate;
use nautilus_network::{
    RECONNECTED,
    error::SendError,
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use serde_json::value::RawValue;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // tokio-import-ok
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;
use zeroize::Zeroize;

use super::{
    client::{POLYMARKET_HEARTBEAT_PAYLOAD, POLYMARKET_HEARTBEAT_SECS, WsChannel},
    messages::{
        MarketInitialSubscribeRequest, MarketSubscribeRequest, MarketUnsubscribeRequest,
        MarketWsMessage, PolymarketWsAuth, PolymarketWsMessage, UserSubscribeRequest,
        UserWsMessage,
    },
};
use crate::{common::credential::Credential, http::error::sanitize_error_text};

const INITIAL_DUMP: bool = true;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Add asset IDs to the market-channel subscription set and send a subscribe message.
    SubscribeMarket(Vec<String>),
    /// Remove asset IDs from the subscription set and send an unsubscribe message.
    UnsubscribeMarket(Vec<String>),
    /// Cycle asset IDs (unsubscribe then subscribe) without changing desired
    /// subscription state, forcing the venue to emit fresh snapshots.
    CycleMarketSubscription {
        asset_ids: Vec<String>,
        cancel: CancellationToken,
        responder: tokio::sync::oneshot::Sender<CycleMarketOutcome>,
        gate: SnapshotGate,
    },
    /// Send the authenticated subscribe message on the user channel.
    SubscribeUser,
}

/// Outcome of a recovery subscription cycle.
#[derive(Clone, Debug)]
pub enum CycleMarketOutcome {
    /// Both legs wrote on one connection; a fresh snapshot should follow.
    Completed,
    /// The connection changed mid-cycle; reconnect replay owns restoration.
    ConnectionChanged,
    /// The attempt was abandoned; a written unsubscribe leg was restored.
    Cancelled,
    /// A genuine unsubscribe won; nothing was sent or restored.
    NotDesired,
    /// A wire write failed; a written unsubscribe leg was restored.
    SendFailed(SendError),
}

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    channel: WsChannel,
    client: Option<WebSocketClient>,
    cmd_rx: UnboundedReceiver<HandlerCommand>,
    raw_rx: UnboundedReceiver<(u64, Message)>,
    out_tx: UnboundedSender<PolymarketWsMessage>,
    credential: Option<Credential>,
    subscriptions: SubscriptionState,
    discovery_subscribed: Arc<AtomicBool>,
    initial_market_replay: Option<(Vec<String>, u64)>,
    auth_tracker: AuthTracker,
    // True once SubscribeUser has been explicitly requested by the caller
    user_subscribed: bool,
    // True once the current market-channel session has sent its initial subscribe payload.
    market_subscription_initialized: bool,
    market_heartbeat_next: Option<(tokio::time::Instant, u64)>,
    // Assets awaiting first authoritative data, keyed by the connection that wrote the subscribe.
    market_subscription_epochs: AHashMap<String, u64>,
    // Overflow buffer for batched frames, drained before reading the next raw message
    message_buffer: Vec<PolymarketWsMessage>,
    // Whether to include `custom_feature_enabled: true` in the initial subscribe
    subscribe_new_markets: bool,
}

impl FeedHandler {
    #[expect(clippy::too_many_arguments)]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        channel: WsChannel,
        client: Option<WebSocketClient>,
        cmd_rx: UnboundedReceiver<HandlerCommand>,
        raw_rx: UnboundedReceiver<(u64, Message)>,
        out_tx: UnboundedSender<PolymarketWsMessage>,
        credential: Option<Credential>,
        subscriptions: SubscriptionState,
        discovery_subscribed: Arc<AtomicBool>,
        initial_market_replay: Option<(Vec<String>, u64)>,
        auth_tracker: AuthTracker,
        user_subscribed: bool,
        subscribe_new_markets: bool,
    ) -> Self {
        Self {
            signal,
            channel,
            client,
            cmd_rx,
            raw_rx,
            out_tx,
            credential,
            subscriptions,
            discovery_subscribed,
            initial_market_replay,
            auth_tracker,
            user_subscribed,
            market_subscription_initialized: false,
            market_heartbeat_next: None,
            market_subscription_epochs: AHashMap::new(),
            message_buffer: Vec::new(),
            subscribe_new_markets,
        }
    }

    pub(super) fn send(&self, msg: PolymarketWsMessage) -> Result<(), String> {
        self.out_tx
            .send(msg)
            .map_err(|e| format!("Failed to send message: {e}"))
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    async fn send_subscribe_market(&mut self, asset_ids: &[String], connection_epoch: Option<u64>) {
        if let Err(e) = self
            .try_send_subscribe_market(asset_ids, connection_epoch)
            .await
        {
            log::error!("Failed to send market subscribe: {e}");
        }
    }

    async fn try_send_subscribe_market(
        &mut self,
        asset_ids: &[String],
        connection_epoch: Option<u64>,
    ) -> Result<(), SendError> {
        let Some(ref client) = self.client else {
            log::warn!("No client available for market subscribe");
            return Err(SendError::Closed);
        };

        let connection_epoch = connection_epoch.unwrap_or_else(|| client.connection_epoch());

        for id in asset_ids {
            self.market_subscription_epochs.remove(id);
            self.subscriptions.mark_subscribe(id);
        }

        let payload = if self.market_subscription_initialized {
            serde_json::to_string(&MarketSubscribeRequest {
                assets_ids: asset_ids.to_vec(),
                operation: "subscribe",
                initial_dump: INITIAL_DUMP,
                custom_feature_enabled: self.subscribe_new_markets,
            })
        } else {
            serde_json::to_string(&MarketInitialSubscribeRequest {
                assets_ids: asset_ids.to_vec(),
                msg_type: "market",
                initial_dump: INITIAL_DUMP,
                custom_feature_enabled: self.subscribe_new_markets,
            })
        };

        let payload = match payload {
            Ok(payload) => payload,
            Err(e) => {
                for id in asset_ids {
                    self.market_subscription_epochs.remove(id);
                    self.subscriptions.mark_failure(id);
                }

                return Err(SendError::InvalidInput(e.to_string()));
            }
        };

        match client
            .send_text_on_connection(payload, None, connection_epoch)
            .await
        {
            Ok(()) => {
                for id in asset_ids {
                    if self.market_subscription_pending(id) {
                        self.market_subscription_epochs
                            .insert(id.clone(), connection_epoch);
                    }
                }

                if !self.market_subscription_initialized {
                    self.market_subscription_initialized = true;
                    self.schedule_market_heartbeat(connection_epoch);
                }

                Ok(())
            }
            Err(e) => {
                for id in asset_ids {
                    self.market_subscription_epochs.remove(id);
                    self.subscriptions.mark_failure(id);
                }

                Err(e)
            }
        }
    }

    async fn send_unsubscribe_market(&self, asset_ids: &[String]) {
        let Some(epoch) = self.client.as_ref().map(|client| client.connection_epoch()) else {
            log::warn!("No client available for market unsubscribe");
            return;
        };

        // Epoch-bound: a reconnect drops the send instead of buffering it onto
        // the new connection, where the replayed subscription set no longer
        // includes these assets.
        match self.try_send_unsubscribe_market(asset_ids, epoch).await {
            Ok(()) => {}
            Err(SendError::ConnectionChanged) => {
                log::debug!(
                    "Dropped market unsubscribe during reconnect; replay excludes the assets"
                );
            }
            Err(e) => {
                log::error!("Failed to send market unsubscribe: {e}");
            }
        }
    }

    async fn try_send_unsubscribe_market(
        &self,
        asset_ids: &[String],
        connection_epoch: u64,
    ) -> Result<(), SendError> {
        let Some(ref client) = self.client else {
            return Err(SendError::Closed);
        };

        let req = MarketUnsubscribeRequest {
            assets_ids: asset_ids.to_vec(),
            operation: "unsubscribe",
        };

        let payload =
            serde_json::to_string(&req).map_err(|e| SendError::InvalidInput(e.to_string()))?;
        client
            .send_text_on_connection(payload, None, connection_epoch)
            .await
    }

    async fn cycle_market_subscription(
        &mut self,
        asset_ids: &[String],
        cancel: &CancellationToken,
        gate: &SnapshotGate,
    ) -> CycleMarketOutcome {
        if cancel.is_cancelled() {
            return CycleMarketOutcome::Cancelled;
        }

        let Some(epoch) = self.client.as_ref().map(|client| client.connection_epoch()) else {
            return CycleMarketOutcome::SendFailed(SendError::Closed);
        };

        if asset_ids
            .iter()
            .any(|id| !self.market_subscription_desired(id))
        {
            return CycleMarketOutcome::NotDesired;
        }

        // Leg 1: venue-side unsubscribe. Desired state is untouched: the pool
        // retains ownership throughout the cycle.
        match self.try_send_unsubscribe_market(asset_ids, epoch).await {
            Ok(()) | Err(SendError::WriteTimeout) => {}
            Err(SendError::ConnectionChanged) => {
                return CycleMarketOutcome::ConnectionChanged;
            }
            Err(e) => return CycleMarketOutcome::SendFailed(e),
        }

        if cancel.is_cancelled() {
            self.restore_market_subscription(asset_ids, epoch).await;
            return CycleMarketOutcome::Cancelled;
        }

        if asset_ids
            .iter()
            .any(|id| !self.market_subscription_desired(id))
        {
            // A genuine unsubscribe won after the first leg: the venue state
            // already matches desire, so no restore is needed.
            return CycleMarketOutcome::NotDesired;
        }

        let current = self.client.as_ref().map(|client| client.connection_epoch());

        if current != Some(epoch) {
            // Reconnect replay owns restoration on the new connection.
            return CycleMarketOutcome::ConnectionChanged;
        }

        // Leg 2: fresh subscribe on the same connection.
        match self.try_send_subscribe_market(asset_ids, Some(epoch)).await {
            Ok(()) => {
                // Open before returning so the gate precedes every later raw
                // read on this task; a queued fresh snapshot cannot miss it.
                gate.open();
                CycleMarketOutcome::Completed
            }
            Err(SendError::ConnectionChanged) => CycleMarketOutcome::ConnectionChanged,
            Err(e) => {
                self.restore_market_subscription(asset_ids, epoch).await;
                CycleMarketOutcome::SendFailed(e)
            }
        }
    }

    async fn restore_market_subscription(&mut self, asset_ids: &[String], epoch: u64) {
        // Best effort: on failure the next recovery attempt or reconnect
        // replay owns restoration from the retained desired state.
        if let Err(e) = self.try_send_subscribe_market(asset_ids, Some(epoch)).await {
            log::warn!("Failed to restore market subscription after cycle abort: {e}");
        }
    }

    fn market_subscription_desired(&self, asset_id: &str) -> bool {
        self.subscriptions
            .is_subscribed(&Ustr::from(asset_id), &Ustr::from(""))
    }

    async fn send_subscribe_user(&self) {
        let Some(ref client) = self.client else {
            log::warn!("No client available for user subscribe");
            return;
        };
        let Some(cred) = &self.credential else {
            log::error!("User channel subscribe requires credential");
            return;
        };

        let mut req = UserSubscribeRequest {
            auth: PolymarketWsAuth {
                api_key: SecretString::from(cred.api_key_str()),
                secret: cred.api_secret(),
                passphrase: SecretString::from(cred.passphrase()),
            },
            msg_type: "user",
        };

        // Begin auth tracking; discard receiver, state is queried via is_authenticated()
        drop(self.auth_tracker.begin());

        let payload = serde_json::to_string(&req);
        req.zeroize();

        match payload {
            Ok(payload) => {
                // auth_tracker.succeed() is NOT called here; sending the request only
                // confirms delivery to the server, not that the credentials were accepted.
                // succeed() is called in next() when the server actually sends user-channel
                // data, which is the real confirmation that authentication worked.
                if let Err(e) = client.send_text(payload, None).await {
                    self.auth_tracker.fail(e.to_string());
                    log::error!("Failed to send user subscribe: {e}");
                }
            }
            Err(e) => {
                self.auth_tracker.fail(format!("Serialize error: {e}"));
                log::error!("Failed to serialize user subscribe request: {e}");
            }
        }
    }

    async fn resubscribe_all(&mut self, connection_epoch: u64) {
        match self.channel {
            WsChannel::Market => {
                let ids = self.subscriptions.reset_after_reconnect();
                if ids.is_empty() && !self.discovery_subscribed.load(Ordering::Relaxed) {
                    return;
                }
                log::info!(
                    "Restoring market subscription state after reconnect: assets={}, discovery={}",
                    ids.len(),
                    self.discovery_subscribed.load(Ordering::Relaxed),
                );
                self.send_subscribe_market(&ids, Some(connection_epoch))
                    .await;
            }
            WsChannel::User => {
                if self.user_subscribed {
                    log::info!("Re-authenticating user channel after reconnect");
                    self.send_subscribe_user().await;
                }
            }
        }
    }

    fn parse_messages(&self, text: &str) -> Vec<PolymarketWsMessage> {
        // When `subscribe_new_markets` is enabled, Polymarket's WSS periodically
        // sends the plain-text string "NO NEW ASSETS" as a heartbeat/ack.
        if text == "NO NEW ASSETS" {
            return vec![];
        }

        // Reply to the application-level `PING` heartbeat, which is not JSON
        if text == "PONG" {
            return vec![];
        }

        match self.channel {
            WsChannel::Market => {
                if let Ok(msgs) = serde_json::from_str::<Vec<&RawValue>>(text) {
                    msgs.into_iter()
                        .filter_map(|raw| match MarketWsMessage::parse(raw.get()) {
                            Ok(msg) => Some(PolymarketWsMessage::Market(msg)),
                            Err(e) => {
                                log::warn!("Failed to parse market WS batch element: {e}");
                                None
                            }
                        })
                        .collect()
                } else {
                    match MarketWsMessage::parse(text) {
                        Ok(msg) => vec![PolymarketWsMessage::Market(msg)],
                        Err(e) => {
                            log::warn!(
                                "Failed to parse market WS message: {e}; payload={}",
                                sanitize_error_text(text)
                            );
                            vec![]
                        }
                    }
                }
            }
            WsChannel::User => {
                if let Ok(msgs) = UserWsMessage::parse_batch(text) {
                    msgs.into_iter().map(PolymarketWsMessage::User).collect()
                } else {
                    match UserWsMessage::parse(text) {
                        Ok(msg) => vec![PolymarketWsMessage::User(msg)],
                        Err(e) => {
                            log::warn!(
                                "Failed to parse user WS message: {e}; payload={}",
                                sanitize_error_text(text)
                            );
                            vec![]
                        }
                    }
                }
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<PolymarketWsMessage> {
        if !self.message_buffer.is_empty() {
            return Some(self.message_buffer.remove(0));
        }

        if let Some((asset_ids, connection_epoch)) = self.initial_market_replay.take() {
            self.send_subscribe_market(&asset_ids, Some(connection_epoch))
                .await;
        }

        loop {
            let market_heartbeat_next = self.market_heartbeat_next;

            tokio::select! {
                connection_epoch = async move {
                    if let Some((deadline, connection_epoch)) = market_heartbeat_next {
                        tokio::time::sleep_until(deadline).await;
                        connection_epoch
                    } else {
                        std::future::pending::<u64>().await
                    }
                } => {
                    self.send_market_heartbeat(connection_epoch).await;
                    self.schedule_market_heartbeat(connection_epoch);
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Setting WebSocket client in handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler received disconnect command");

                            if let Some(ref client) = self.client {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                        HandlerCommand::SubscribeMarket(ids) => {
                            if self.subscribe_new_markets && ids.is_empty() {
                                self.discovery_subscribed.store(true, Ordering::Relaxed);
                            }
                            self.send_subscribe_market(&ids, None).await;
                        }
                        HandlerCommand::UnsubscribeMarket(ids) => {
                            for id in &ids {
                                self.market_subscription_epochs.remove(id);
                                self.subscriptions.mark_unsubscribe(id);
                            }
                            self.send_unsubscribe_market(&ids).await;
                            for id in &ids {
                                self.subscriptions.confirm_unsubscribe(id);
                            }
                        }
                        HandlerCommand::CycleMarketSubscription {
                            asset_ids,
                            cancel,
                            responder,
                            gate,
                        } => {
                            let outcome = self
                                .cycle_market_subscription(&asset_ids, &cancel, &gate)
                                .await;
                            let _ = responder.send(outcome);
                        }
                        HandlerCommand::SubscribeUser => {
                            self.user_subscribed = true;
                            self.send_subscribe_user().await;
                        }
                    }
                }
                Some((connection_epoch, raw)) = self.raw_rx.recv() => {
                    match raw {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                self.market_subscription_initialized = false;
                                self.market_heartbeat_next = None;
                                self.resubscribe_all(connection_epoch).await;
                                return Some(PolymarketWsMessage::Reconnected { shard_id: None });
                            }
                            let msgs = self.parse_messages(&text);
                            if msgs.is_empty() {
                                continue;
                            }

                            if self.channel == WsChannel::Market {
                                self.confirm_market_subscriptions(connection_epoch, &msgs);
                            } else {
                                // Receiving any user-channel data confirms the server accepted the
                                // credentials; mark auth as successful on the first delivery.
                                self.auth_tracker.succeed();
                            }
                            // Buffer msgs[1..] so they are returned in order on subsequent
                            // next() calls; returning first directly preserves 0,1,2,...,n order
                            let mut iter = msgs.into_iter();
                            let first = iter.next().unwrap();
                            self.message_buffer.extend(iter);
                            return Some(first);
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                log::warn!("Failed to send pong: {e}");
                            }
                        }
                        Message::Close(_) => {
                            log::debug!("WebSocket close frame received");
                            return None;
                        }
                        _ => {}
                    }
                }
                else => return None,
            }
        }
    }

    fn schedule_market_heartbeat(&mut self, connection_epoch: u64) {
        self.market_heartbeat_next = Some((
            tokio::time::Instant::now() + Duration::from_secs(POLYMARKET_HEARTBEAT_SECS),
            connection_epoch,
        ));
    }

    async fn send_market_heartbeat(&self, connection_epoch: u64) {
        let Some(ref client) = self.client else {
            return;
        };

        if let Err(e) = client
            .send_text_on_connection(
                POLYMARKET_HEARTBEAT_PAYLOAD.to_string(),
                None,
                connection_epoch,
            )
            .await
        {
            log::debug!("Failed to send market heartbeat: {e}");
        }
    }

    fn confirm_market_subscriptions(
        &mut self,
        connection_epoch: u64,
        messages: &[PolymarketWsMessage],
    ) {
        for message in messages {
            let asset_id = match message {
                PolymarketWsMessage::Market(MarketWsMessage::Book(book)) => &book.asset_id,
                PolymarketWsMessage::Market(MarketWsMessage::LastTradePrice(trade)) => {
                    &trade.asset_id
                }
                _ => continue,
            };

            let was_sent_on_connection = self
                .market_subscription_epochs
                .get(asset_id.as_str())
                .is_some_and(|epoch| *epoch == connection_epoch);

            if was_sent_on_connection && self.market_subscription_pending(asset_id.as_str()) {
                self.subscriptions.confirm_subscribe(asset_id.as_str());
                self.market_subscription_epochs.remove(asset_id.as_str());
            }
        }
    }

    fn market_subscription_pending(&self, asset_id: &str) -> bool {
        let channel_level = Ustr::from("");
        let asset_id = Ustr::from(asset_id);
        self.subscriptions
            .pending_subscribe()
            .get(&asset_id)
            .is_some_and(|symbols| symbols.contains(&channel_level))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use nautilus_common::testing::wait_until_async;
    use nautilus_network::websocket::{TransportBackend, WebSocketConfig, channel_message_handler};
    use parking_lot::Mutex;
    use rstest::{fixture, rstest};
    use serde_json::{Value, json};

    use super::*;
    use crate::common::enums::PolymarketOrderSide;

    const MARKET_ASSET_ID: &str =
        "71321045679252212594626385532706912750332728571942532289631379312455583992563";

    #[fixture]
    fn market_handler() -> FeedHandler {
        feed_handler(WsChannel::Market)
    }

    #[fixture]
    fn user_handler() -> FeedHandler {
        feed_handler(WsChannel::User)
    }

    fn feed_handler(channel: WsChannel) -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            channel,
            None,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            Arc::new(AtomicBool::new(false)),
            None,
            AuthTracker::new(),
            false,
            false,
        )
    }

    async fn recording_server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind recording server");
        let addr = listener.local_addr().expect("recording server address");
        let messages = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&messages);

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept websocket client");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept websocket handshake");

            while let Some(message) = socket.next().await {
                match message.expect("read websocket message") {
                    Message::Text(text) => received.lock().push(text.to_string()),
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });

        (format!("ws://{addr}"), messages)
    }

    async fn recording_client(url: String) -> WebSocketClient {
        let config = WebSocketConfig::builder()
            .url(url)
            .backend(TransportBackend::Tungstenite)
            .build()
            .expect("valid websocket config");
        let (message_handler, _message_rx) = channel_message_handler();
        WebSocketClient::builder()
            .config(config)
            .message_handler(message_handler)
            .connect()
            .await
            .expect("connect websocket client")
    }

    fn market_handler_with(
        client: WebSocketClient,
    ) -> (
        FeedHandler,
        UnboundedSender<(u64, Message)>,
        SubscriptionState,
    ) {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');
        let handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            WsChannel::Market,
            Some(client),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            subscriptions.clone(),
            Arc::new(AtomicBool::new(false)),
            None,
            AuthTracker::new(),
            false,
            false,
        );

        (handler, raw_tx, subscriptions)
    }

    fn market_handler_with_cmd_tx(
        client: WebSocketClient,
    ) -> (
        FeedHandler,
        tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
        UnboundedSender<(u64, Message)>,
        SubscriptionState,
    ) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');

        let handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            WsChannel::Market,
            Some(client),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            subscriptions.clone(),
            Arc::new(AtomicBool::new(false)),
            None,
            AuthTracker::new(),
            false,
            false,
        );

        (handler, cmd_tx, raw_tx, subscriptions)
    }

    #[rstest]
    #[tokio::test]
    async fn initial_market_replay_recovers_on_current_connection_epoch() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;

        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            WsChannel::Market,
            Some(client),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            Arc::new(AtomicBool::new(true)),
            Some((vec![], 1)),
            AuthTracker::new(),
            false,
            true,
        );
        raw_tx
            .send((0, Message::Text(RECONNECTED.into())))
            .expect("queue reconnect notification");

        assert!(matches!(
            handler.next().await,
            Some(PolymarketWsMessage::Reconnected { .. }),
        ));
        handler
            .client
            .as_ref()
            .expect("websocket client")
            .send_text_on_connection("barrier".to_string(), None, 0)
            .await
            .expect("send barrier on current connection");

        wait_until_async(
            || {
                let messages = Arc::clone(&messages);
                async move { messages.lock().len() >= 2 }
            },
            Duration::from_secs(1),
        )
        .await;

        {
            let messages = messages.lock();
            assert_eq!(messages.len(), 2);
            assert_eq!(
                serde_json::from_str::<Value>(&messages[0]).expect("valid subscribe payload"),
                json!({
                    "assets_ids": [],
                    "type": "market",
                    "initial_dump": true,
                    "custom_feature_enabled": true,
                }),
            );
            assert_eq!(messages[1], "barrier");
        }

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn market_text_heartbeat_follows_initial_subscription() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let connection_epoch = client.connection_epoch();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            WsChannel::Market,
            Some(client),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            Arc::new(AtomicBool::new(false)),
            None,
            AuthTracker::new(),
            false,
            false,
        );

        handler
            .send_subscribe_market(&[MARKET_ASSET_ID.to_string()], None)
            .await;
        wait_for_recorded_messages(&messages, 1).await;

        let task = tokio::spawn(async move {
            let message = handler.next().await;
            (handler, message)
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(POLYMARKET_HEARTBEAT_SECS)).await;
        wait_for_recorded_messages(&messages, 2).await;

        raw_tx
            .send((connection_epoch, Message::Text(RECONNECTED.into())))
            .expect("queue reconnect notification");
        let (mut handler, message) = task.await.expect("join handler task");
        assert!(matches!(
            message,
            Some(PolymarketWsMessage::Reconnected { .. })
        ));
        wait_for_recorded_messages(&messages, 3).await;

        let task = tokio::spawn(async move {
            let message = handler.next().await;
            (handler, message)
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(POLYMARKET_HEARTBEAT_SECS)).await;
        wait_for_recorded_messages(&messages, 4).await;

        cmd_tx
            .send(HandlerCommand::Disconnect)
            .expect("queue disconnect");
        let (_, message) = task.await.expect("join handler task");
        assert!(message.is_none());

        let messages = messages.lock().clone();
        let expected_subscription = json!({
            "assets_ids": [MARKET_ASSET_ID],
            "type": "market",
            "initial_dump": true,
        });
        assert_eq!(messages.len(), 4);
        assert_eq!(
            serde_json::from_str::<Value>(&messages[0]).expect("valid subscribe payload"),
            expected_subscription,
        );
        assert_eq!(messages[1], POLYMARKET_HEARTBEAT_PAYLOAD);
        assert_eq!(
            serde_json::from_str::<Value>(&messages[2]).expect("valid replay payload"),
            expected_subscription,
        );
        assert_eq!(messages[3], POLYMARKET_HEARTBEAT_PAYLOAD);
    }

    #[rstest]
    #[tokio::test]
    async fn market_heartbeat_stays_bound_to_subscribed_connection() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let connection_epoch = client.connection_epoch();
        let connection_epoch_atomic = client.connection_epoch_atomic();
        let (mut handler, raw_tx, _) = market_handler_with(client);

        handler
            .send_subscribe_market(&[MARKET_ASSET_ID.to_string()], None)
            .await;
        wait_for_recorded_messages(&messages, 1).await;

        let replacement_epoch = connection_epoch + 1;
        connection_epoch_atomic.store(replacement_epoch, Ordering::Release);
        handler.send_market_heartbeat(connection_epoch).await;
        raw_tx
            .send((replacement_epoch, Message::Text(RECONNECTED.into())))
            .expect("queue reconnect notification");

        assert!(matches!(
            handler.next().await,
            Some(PolymarketWsMessage::Reconnected { .. }),
        ));
        handler
            .client
            .as_ref()
            .expect("websocket client")
            .send_text_on_connection("barrier".to_string(), None, replacement_epoch)
            .await
            .expect("send barrier on replacement connection");
        wait_until_async(
            || {
                let messages = Arc::clone(&messages);
                async move {
                    messages
                        .lock()
                        .last()
                        .is_some_and(|message| message == "barrier")
                }
            },
            Duration::from_secs(1),
        )
        .await;

        let messages = messages.lock().clone();
        let expected_subscription = json!({
            "assets_ids": [MARKET_ASSET_ID],
            "type": "market",
            "initial_dump": true,
        });
        assert_eq!(messages.len(), 3);
        assert_eq!(
            serde_json::from_str::<Value>(&messages[0]).expect("valid subscribe payload"),
            expected_subscription,
        );
        assert_eq!(
            serde_json::from_str::<Value>(&messages[1]).expect("valid replay payload"),
            expected_subscription,
        );
        assert_eq!(messages[2], "barrier");

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    async fn wait_for_recorded_messages(messages: &Arc<Mutex<Vec<String>>>, expected: usize) {
        wait_until_async(
            || {
                let messages = Arc::clone(messages);
                async move { messages.lock().len() == expected }
            },
            Duration::from_secs(1),
        )
        .await;
    }

    #[rstest]
    #[case(include_str!("../../test_data/ws_market_book_msg.json"))]
    #[case(include_str!("../../test_data/ws_market_last_trade_msg.json"))]
    #[tokio::test]
    async fn market_subscription_confirms_from_first_book_or_trade(#[case] payload: &str) {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, raw_tx, subscriptions) = market_handler_with(client);

        handler
            .send_subscribe_market(&[MARKET_ASSET_ID.to_string()], None)
            .await;
        wait_until_async(
            || {
                let messages = Arc::clone(&messages);
                async move { !messages.lock().is_empty() }
            },
            Duration::from_secs(1),
        )
        .await;

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(subscriptions.len(), 0);

        raw_tx
            .send((0, Message::Text(payload.into())))
            .expect("queue market data");
        assert!(matches!(
            handler.next().await,
            Some(PolymarketWsMessage::Market(_)),
        ));

        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 1);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    fn unsolicited_market_data_does_not_create_subscription(mut market_handler: FeedHandler) {
        let messages =
            market_handler.parse_messages(include_str!("../../test_data/ws_market_book_msg.json"));

        market_handler.confirm_market_subscriptions(0, &messages);

        assert!(market_handler.subscriptions.is_empty());
    }

    #[rstest]
    fn market_batch_confirms_trade_but_not_price_change(mut market_handler: FeedHandler) {
        let price_change_asset_id = "101";
        let trade_asset_id = "202";
        market_handler
            .subscriptions
            .mark_subscribe(price_change_asset_id);
        market_handler.subscriptions.mark_subscribe(trade_asset_id);
        market_handler
            .market_subscription_epochs
            .insert(price_change_asset_id.to_string(), 0);
        market_handler
            .market_subscription_epochs
            .insert(trade_asset_id.to_string(), 0);
        let messages = market_handler.parse_messages(include_str!(
            "../../test_data/ws_market_mixed_known_unknown.json"
        ));

        market_handler.confirm_market_subscriptions(0, &messages);

        assert_eq!(
            market_handler.subscriptions.pending_subscribe_topics(),
            vec![price_change_asset_id]
        );
        assert_eq!(market_handler.subscriptions.len(), 1);
    }

    #[rstest]
    fn market_subscription_confirmation_requires_sent_current_epoch(
        mut market_handler: FeedHandler,
    ) {
        market_handler.subscriptions.mark_subscribe(MARKET_ASSET_ID);
        let messages =
            market_handler.parse_messages(include_str!("../../test_data/ws_market_book_msg.json"));

        market_handler.confirm_market_subscriptions(0, &messages);
        assert_eq!(
            market_handler.subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(market_handler.subscriptions.len(), 0);

        market_handler
            .market_subscription_epochs
            .insert(MARKET_ASSET_ID.to_string(), 1);
        market_handler.confirm_market_subscriptions(0, &messages);
        assert_eq!(
            market_handler.subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(market_handler.subscriptions.len(), 0);

        market_handler.confirm_market_subscriptions(1, &messages);
        assert!(
            market_handler
                .subscriptions
                .pending_subscribe_topics()
                .is_empty()
        );
        assert_eq!(market_handler.subscriptions.len(), 1);
    }

    #[rstest]
    #[tokio::test]
    async fn reconnect_replay_requires_current_connection_data() {
        let cancelled_asset_id = "cancelled-asset";
        let (url, _) = recording_server().await;
        let client = recording_client(url).await;
        let connection_epoch = client.connection_epoch();
        let (mut handler, raw_tx, subscriptions) = market_handler_with(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);
        subscriptions.confirm_subscribe(MARKET_ASSET_ID);
        subscriptions.mark_subscribe(cancelled_asset_id);
        subscriptions.confirm_subscribe(cancelled_asset_id);
        subscriptions.mark_unsubscribe(cancelled_asset_id);

        handler.resubscribe_all(connection_epoch).await;

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 0);

        raw_tx
            .send((
                connection_epoch,
                Message::Text(include_str!("../../test_data/ws_market_book_msg.json").into()),
            ))
            .expect("queue market data");
        assert!(matches!(
            handler.next().await,
            Some(PolymarketWsMessage::Market(_)),
        ));

        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 1);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test]
    async fn failed_market_subscribe_stays_pending_for_reconnect_replay() {
        let (url, _) = recording_server().await;
        let client = recording_client(url).await;
        client.disconnect().await;
        let (mut handler, raw_tx, subscriptions) = market_handler_with(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);
        subscriptions.confirm_subscribe(MARKET_ASSET_ID);

        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 1);

        handler
            .send_subscribe_market(&[MARKET_ASSET_ID.to_string()], None)
            .await;

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(subscriptions.len(), 0);

        raw_tx
            .send((
                0,
                Message::Text(include_str!("../../test_data/ws_market_book_msg.json").into()),
            ))
            .expect("queue stale market data");
        assert!(matches!(
            handler.next().await,
            Some(PolymarketWsMessage::Market(_)),
        ));
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(subscriptions.len(), 0);

        let (replay_url, messages) = recording_server().await;
        let replay_client = recording_client(replay_url).await;
        let connection_epoch = replay_client.connection_epoch();
        handler.client = Some(replay_client);
        handler.resubscribe_all(connection_epoch).await;
        wait_until_async(
            || {
                let messages = Arc::clone(&messages);
                async move { !messages.lock().is_empty() }
            },
            Duration::from_secs(1),
        )
        .await;

        {
            let messages = messages.lock();
            assert_eq!(messages.len(), 1);
            assert_eq!(
                serde_json::from_str::<Value>(&messages[0]).expect("valid subscribe payload"),
                json!({
                    "assets_ids": [MARKET_ASSET_ID],
                    "type": "market",
                    "initial_dump": true,
                }),
            );
        }
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![MARKET_ASSET_ID]
        );
        assert_eq!(subscriptions.len(), 0);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    fn test_parse_market_batch_skips_unknown_event(market_handler: FeedHandler) {
        let messages = market_handler.parse_messages(include_str!(
            "../../test_data/ws_market_mixed_known_unknown.json"
        ));

        assert_eq!(messages.len(), 2);

        let PolymarketWsMessage::Market(MarketWsMessage::PriceChange(quotes)) = &messages[0] else {
            panic!("Expected first message to be a price change");
        };
        assert_eq!(
            quotes.market,
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(quotes.timestamp, "1700000000001");
        assert_eq!(quotes.price_changes.len(), 1);

        let quote = &quotes.price_changes[0];
        assert_eq!(quote.asset_id, "101");
        assert_eq!(quote.price, "0.37");
        assert_eq!(quote.side, PolymarketOrderSide::Buy);
        assert_eq!(quote.size, "12.5");
        assert_eq!(quote.hash, "price-change-hash");
        assert_eq!(quote.best_bid.as_deref(), Some("0.36"));
        assert_eq!(quote.best_ask.as_deref(), Some("0.38"));

        let PolymarketWsMessage::Market(MarketWsMessage::LastTradePrice(trade)) = &messages[1]
        else {
            panic!("Expected second message to be a last trade price");
        };
        assert_eq!(
            trade.market,
            "0x2222222222222222222222222222222222222222222222222222222222222222"
        );
        assert_eq!(trade.asset_id, "202");
        assert_eq!(trade.fee_rate_bps, "17");
        assert_eq!(trade.price, "0.63");
        assert_eq!(trade.side, PolymarketOrderSide::Sell);
        assert_eq!(trade.size, "4.25");
        assert_eq!(trade.timestamp, "1700000000003");
        assert_eq!(trade.transaction_hash.as_deref(), Some("0xtrade-hash"));
    }

    #[rstest]
    fn test_parse_market_single_message(market_handler: FeedHandler) {
        let messages = market_handler.parse_messages(include_str!(
            "../../test_data/ws_market_last_trade_msg.json"
        ));

        assert_eq!(messages.len(), 1);

        let PolymarketWsMessage::Market(MarketWsMessage::LastTradePrice(trade)) = &messages[0]
        else {
            panic!("Expected a last trade price");
        };
        assert_eq!(
            trade.market,
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        );
        assert_eq!(
            trade.asset_id,
            "71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(trade.fee_rate_bps, "0");
        assert_eq!(trade.price, "0.51");
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.size, "25.0");
        assert_eq!(trade.timestamp, "1703875202000");
        assert!(trade.transaction_hash.is_none());
    }

    #[rstest]
    fn test_parse_user_batch(user_handler: FeedHandler) {
        let messages =
            user_handler.parse_messages(include_str!("../../test_data/ws_user_batch_msg.json"));
        let actual: Vec<UserWsMessage> = messages
            .into_iter()
            .map(|message| match message {
                PolymarketWsMessage::User(message) => message,
                other => panic!("Expected user message, received {other:?}"),
            })
            .collect();
        let expected: Vec<UserWsMessage> =
            serde_json::from_str(include_str!("../../test_data/ws_user_batch_msg.json"))
                .expect("user batch fixture should deserialize");

        assert_eq!(actual, expected);
    }

    fn cycle_asset_ids() -> Vec<String> {
        vec![MARKET_ASSET_ID.to_string()]
    }

    fn assert_desired(handler: &FeedHandler, subscriptions: &SubscriptionState) {
        assert!(
            handler.market_subscription_desired(MARKET_ASSET_ID),
            "cycle must preserve desired ownership"
        );
        assert!(
            subscriptions.pending_unsubscribe_topics().is_empty(),
            "cycle must never mark desired assets for unsubscribe"
        );
    }

    /// Classifies client frames: `subscribe` covers both the initial
    /// `type: market` frame and later `operation` frames.
    fn frame_kind(frame: &str) -> &str {
        let value: Value = serde_json::from_str(frame).expect("client frame should be JSON");
        if value.get("operation").and_then(Value::as_str) == Some("unsubscribe") {
            return "unsubscribe";
        }

        if value.get("operation").and_then(Value::as_str) == Some("subscribe")
            || value.get("type").and_then(Value::as_str) == Some("market")
        {
            return "subscribe";
        }

        panic!("unexpected client frame: {frame}");
    }

    fn frame_assets(frame: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(frame).expect("client frame should be JSON");
        value
            .get("assets_ids")
            .and_then(Value::as_array)
            .expect("frame should carry assets_ids")
            .iter()
            .map(|id| {
                id.as_str()
                    .expect("asset id should be a string")
                    .to_string()
            })
            .collect()
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_market_subscription_resubscribes_without_changing_desired_state() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, _raw_tx, subscriptions) = market_handler_with(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);

        let gate = SnapshotGate::default();
        gate.lock().close();

        let outcome = handler
            .cycle_market_subscription(&cycle_asset_ids(), &CancellationToken::new(), &gate)
            .await;
        assert!(matches!(outcome, CycleMarketOutcome::Completed));
        assert!(!gate.lock().is_closed());

        wait_for_recorded_messages(&messages, 2).await;
        let frames = messages.lock().clone();
        assert_eq!(frame_kind(&frames[0]), "unsubscribe");
        assert_eq!(frame_kind(&frames[1]), "subscribe");

        for frame in &frames {
            assert_eq!(frame_assets(frame), cycle_asset_ids());
        }

        assert_desired(&handler, &subscriptions);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_market_subscription_precancelled_sends_nothing() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, _raw_tx, subscriptions) = market_handler_with(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);

        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = handler
            .cycle_market_subscription(&cycle_asset_ids(), &cancel, &SnapshotGate::default())
            .await;
        assert!(matches!(outcome, CycleMarketOutcome::Cancelled));

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(messages.lock().is_empty());
        assert_desired(&handler, &subscriptions);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_market_subscription_without_client_fails_closed(
        mut market_handler: FeedHandler,
    ) {
        let subscriptions = market_handler.subscriptions.clone();
        subscriptions.mark_subscribe(MARKET_ASSET_ID);
        let outcome = market_handler
            .cycle_market_subscription(
                &cycle_asset_ids(),
                &CancellationToken::new(),
                &SnapshotGate::default(),
            )
            .await;
        assert!(matches!(
            outcome,
            CycleMarketOutcome::SendFailed(SendError::Closed)
        ));
        assert_desired(&market_handler, &subscriptions);
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_market_subscription_rejects_undesired_asset() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, _raw_tx, _subscriptions) = market_handler_with(client);

        let outcome = handler
            .cycle_market_subscription(
                &cycle_asset_ids(),
                &CancellationToken::new(),
                &SnapshotGate::default(),
            )
            .await;
        assert!(matches!(outcome, CycleMarketOutcome::NotDesired));

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(messages.lock().is_empty());

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_market_subscription_after_disconnect_fails_send() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, _raw_tx, subscriptions) = market_handler_with(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;

        let outcome = handler
            .cycle_market_subscription(
                &cycle_asset_ids(),
                &CancellationToken::new(),
                &SnapshotGate::default(),
            )
            .await;
        assert!(matches!(
            outcome,
            CycleMarketOutcome::SendFailed(SendError::Closed)
        ));

        // Leg 1 never wrote, so desired state is retained for retry/replay
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(messages.lock().is_empty());
        assert_desired(&handler, &subscriptions);
    }

    /// Scripted venue: a subscribe only yields a snapshot when the venue-side
    /// state flips from unsubscribed. This reproduces the live finding that a
    /// duplicate mid-connection subscribe receives no snapshot.
    async fn snapshot_gated_server(snapshot: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind snapshot-gated server");
        let addr = listener.local_addr().expect("snapshot-gated address");

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept websocket client");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept websocket handshake");
            let mut subscribed = false;

            while let Some(message) = socket.next().await {
                match message.expect("read websocket message") {
                    Message::Text(text) => {
                        let Ok(value) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };

                        let operation = value.get("operation").and_then(Value::as_str);
                        let is_initial =
                            value.get("type").and_then(Value::as_str) == Some("market");

                        if operation == Some("unsubscribe") {
                            subscribed = false;
                        } else if (operation == Some("subscribe") || is_initial) && !subscribed {
                            subscribed = true;
                            socket
                                .send(Message::Text(snapshot.into()))
                                .await
                                .expect("send gated snapshot");
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });

        format!("ws://{addr}")
    }

    async fn next_market_message(handler: &mut FeedHandler) -> Option<PolymarketWsMessage> {
        loop {
            match handler.next().await {
                Some(message @ PolymarketWsMessage::Market(_)) => return Some(message),
                Some(_) => {}
                None => return None,
            }
        }
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_recovers_snapshot_that_duplicate_subscribe_misses() {
        let snapshot = include_str!("../../test_data/ws_market_book_msg.json");
        let url = snapshot_gated_server(snapshot).await;

        let config = WebSocketConfig::builder()
            .url(url)
            .backend(TransportBackend::Tungstenite)
            .build()
            .expect("valid websocket config");
        let (message_handler, mut message_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(config)
            .message_handler(message_handler)
            .connect()
            .await
            .expect("connect websocket client");
        let epoch = client.connection_epoch_atomic();
        let (mut handler, cmd_tx, raw_tx, subscriptions) = market_handler_with_cmd_tx(client);

        tokio::spawn(async move {
            while let Some(message) = message_rx.recv().await {
                let epoch = epoch.load(Ordering::SeqCst);
                if raw_tx.send((epoch, message)).is_err() {
                    break;
                }
            }
        });

        cmd_tx
            .send(HandlerCommand::SubscribeMarket(cycle_asset_ids()))
            .expect("send initial subscribe");
        tokio::time::timeout(Duration::from_secs(5), next_market_message(&mut handler))
            .await
            .expect("initial subscribe should yield a snapshot")
            .expect("handler should stay open");

        cmd_tx
            .send(HandlerCommand::SubscribeMarket(cycle_asset_ids()))
            .expect("send duplicate subscribe");
        assert!(
            tokio::time::timeout(
                Duration::from_millis(500),
                next_market_message(&mut handler)
            )
            .await
            .is_err(),
            "duplicate subscribe must receive no snapshot from the venue"
        );

        let (responder, response) = tokio::sync::oneshot::channel();
        cmd_tx
            .send(HandlerCommand::CycleMarketSubscription {
                asset_ids: cycle_asset_ids(),
                cancel: CancellationToken::new(),
                responder,
                gate: SnapshotGate::default(),
            })
            .expect("send cycle command");

        // The cycle runs inside next(), so pump the loop while awaiting the outcome
        tokio::pin!(response);
        let mut outcome = None;
        let mut snapshot_seen = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            while outcome.is_none() || !snapshot_seen {
                tokio::select! {
                    result = &mut response, if outcome.is_none() => {
                        outcome = Some(result.expect("cycle responder should stay open"));
                    }
                    message = next_market_message(&mut handler), if !snapshot_seen => {
                        message.expect("handler should stay open");
                        snapshot_seen = true;
                    }
                }
            }
        })
        .await
        .expect("cycle should respond and yield a fresh snapshot");

        assert!(matches!(outcome, Some(CycleMarketOutcome::Completed)));

        assert_desired(&handler, &subscriptions);

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }

    #[rstest]
    #[tokio::test]
    async fn cycle_followed_by_queued_unsubscribe_honors_the_unsubscribe() {
        let (url, messages) = recording_server().await;
        let client = recording_client(url).await;
        let (mut handler, cmd_tx, _raw_tx, subscriptions) = market_handler_with_cmd_tx(client);
        subscriptions.mark_subscribe(MARKET_ASSET_ID);

        // Commands serialize: the cycle completes first, then the unsubscribe wins
        let (responder, response) = tokio::sync::oneshot::channel();
        cmd_tx
            .send(HandlerCommand::CycleMarketSubscription {
                asset_ids: cycle_asset_ids(),
                cancel: CancellationToken::new(),
                responder,
                gate: SnapshotGate::default(),
            })
            .expect("queue cycle command");

        cmd_tx
            .send(HandlerCommand::UnsubscribeMarket(cycle_asset_ids()))
            .expect("queue unsubscribe command");

        tokio::pin!(response);

        let outcome = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    biased;
                    result = &mut response => return result.expect("cycle responder open"),
                    _ = handler.next() => {}
                }
            }
        })
        .await
        .expect("cycle should respond");

        assert!(matches!(outcome, CycleMarketOutcome::Completed));

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if messages.lock().len() >= 3 {
                    break;
                }

                tokio::select! {
                    _ = handler.next() => {}
                    () = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
        })
        .await
        .expect("queued unsubscribe should reach the wire");

        let frames = messages.lock().clone();
        assert_eq!(frame_kind(&frames[0]), "unsubscribe");
        assert_eq!(frame_kind(&frames[1]), "subscribe");
        assert_eq!(frame_kind(&frames[2]), "unsubscribe");

        assert!(!handler.market_subscription_desired(MARKET_ASSET_ID));

        handler
            .client
            .as_ref()
            .expect("websocket client")
            .disconnect()
            .await;
    }
}

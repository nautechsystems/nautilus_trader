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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use serde_json::value::RawValue;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // tokio-import-ok
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    client::WsChannel,
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
    /// Remove asset IDs from the subscription set (no wire message needed).
    UnsubscribeMarket(Vec<String>),
    /// Send the authenticated subscribe message on the user channel.
    SubscribeUser,
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
        let Some(ref client) = self.client else {
            log::warn!("No client available for market subscribe");
            return;
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

        match payload {
            Ok(payload) => {
                let result = client
                    .send_text_on_connection(payload, None, connection_epoch)
                    .await;

                if let Err(e) = result {
                    for id in asset_ids {
                        self.market_subscription_epochs.remove(id);
                        self.subscriptions.mark_failure(id);
                    }
                    log::error!("Failed to send market subscribe: {e}");
                } else {
                    for id in asset_ids {
                        if self.market_subscription_pending(id) {
                            self.market_subscription_epochs
                                .insert(id.clone(), connection_epoch);
                        }
                    }
                    self.market_subscription_initialized = true;
                }
            }
            Err(e) => {
                for id in asset_ids {
                    self.market_subscription_epochs.remove(id);
                    self.subscriptions.mark_failure(id);
                }
                log::error!("Failed to serialize market subscribe request: {e}");
            }
        }
    }

    async fn send_unsubscribe_market(&self, asset_ids: &[String]) {
        let Some(ref client) = self.client else {
            log::warn!("No client available for market unsubscribe");
            return;
        };

        let req = MarketUnsubscribeRequest {
            assets_ids: asset_ids.to_vec(),
            operation: "unsubscribe",
        };

        match serde_json::to_string(&req) {
            Ok(payload) => {
                if let Err(e) = client.send_text(payload, None).await {
                    log::error!("Failed to send market unsubscribe: {e}");
                }
            }
            Err(e) => log::error!("Failed to serialize market unsubscribe request: {e}"),
        }
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

        let req = UserSubscribeRequest {
            auth: PolymarketWsAuth {
                api_key: cred.api_key().to_string(),
                secret: cred.api_secret(),
                passphrase: cred.passphrase().to_string(),
            },
            msg_type: "user",
        };

        // Begin auth tracking; discard receiver, state is queried via is_authenticated()
        drop(self.auth_tracker.begin());

        match serde_json::to_string(&req) {
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
            tokio::select! {
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
                                self.resubscribe_all(connection_epoch).await;
                                return Some(PolymarketWsMessage::Reconnected);
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
    use std::{sync::Mutex, time::Duration};

    use futures_util::StreamExt;
    use nautilus_common::testing::wait_until_async;
    use nautilus_network::websocket::{TransportBackend, WebSocketConfig, channel_message_handler};
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
                    Message::Text(text) => received
                        .lock()
                        .expect("recording mutex poisoned")
                        .push(text.to_string()),
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
            Some(PolymarketWsMessage::Reconnected),
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
                async move { messages.lock().expect("recording mutex poisoned").len() >= 2 }
            },
            Duration::from_secs(1),
        )
        .await;

        {
            let messages = messages.lock().expect("recording mutex poisoned");
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
                async move {
                    !messages
                        .lock()
                        .expect("recording mutex poisoned")
                        .is_empty()
                }
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
                async move {
                    !messages
                        .lock()
                        .expect("recording mutex poisoned")
                        .is_empty()
                }
            },
            Duration::from_secs(1),
        )
        .await;

        {
            let messages = messages.lock().expect("recording mutex poisoned");
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
            quotes.market.as_str(),
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(quotes.timestamp, "1700000000001");
        assert_eq!(quotes.price_changes.len(), 1);

        let quote = &quotes.price_changes[0];
        assert_eq!(quote.asset_id.as_str(), "101");
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
            trade.market.as_str(),
            "0x2222222222222222222222222222222222222222222222222222222222222222"
        );
        assert_eq!(trade.asset_id.as_str(), "202");
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
            trade.market.as_str(),
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        );
        assert_eq!(
            trade.asset_id.as_str(),
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
}

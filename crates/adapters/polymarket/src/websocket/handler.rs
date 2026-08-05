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
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use serde_json::value::RawValue;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // tokio-import-ok
use tokio_tungstenite::tungstenite::Message;

use super::{
    client::WsChannel,
    messages::{
        MarketInitialSubscribeRequest, MarketSubscribeRequest, MarketUnsubscribeRequest,
        MarketWsMessage, PolymarketWsAuth, PolymarketWsMessage, UserSubscribeRequest,
        UserWsMessage,
    },
};
use crate::common::credential::Credential;

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
    /// Refresh asset snapshots without changing retained reconnect intent.
    RefreshMarket(Vec<String>),
    /// Send the authenticated subscribe message on the user channel.
    SubscribeUser,
}

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    channel: WsChannel,
    client: Option<WebSocketClient>,
    cmd_rx: UnboundedReceiver<HandlerCommand>,
    raw_rx: UnboundedReceiver<Message>,
    out_tx: UnboundedSender<PolymarketWsMessage>,
    credential: Option<Credential>,
    subscriptions: SubscriptionState,
    auth_tracker: AuthTracker,
    // True once SubscribeUser has been explicitly requested by the caller
    user_subscribed: bool,
    // True once the current market-channel session has sent its initial subscribe payload.
    market_subscription_initialized: bool,
    // Overflow buffer for batched frames, drained before reading the next raw message
    message_buffer: Vec<PolymarketWsMessage>,
    // Whether to include `custom_feature_enabled: true` in the initial subscribe
    subscribe_new_markets: bool,
    new_market_discovery_enabled: Arc<AtomicBool>,
    connection_epoch_state: Arc<AtomicU64>,
    connection_epoch: u64,
    shard_id: u64,
    awaiting_feed_resume_epoch: Option<u64>,
}

impl FeedHandler {
    #[expect(clippy::too_many_arguments)]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        channel: WsChannel,
        cmd_rx: UnboundedReceiver<HandlerCommand>,
        raw_rx: UnboundedReceiver<Message>,
        out_tx: UnboundedSender<PolymarketWsMessage>,
        credential: Option<Credential>,
        subscriptions: SubscriptionState,
        auth_tracker: AuthTracker,
        user_subscribed: bool,
        subscribe_new_markets: bool,
        new_market_discovery_enabled: Arc<AtomicBool>,
        connection_epoch_state: Arc<AtomicU64>,
        connection_epoch: u64,
        shard_id: u64,
    ) -> Self {
        Self {
            signal,
            channel,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            credential,
            subscriptions,
            auth_tracker,
            user_subscribed,
            market_subscription_initialized: false,
            message_buffer: Vec::new(),
            subscribe_new_markets,
            new_market_discovery_enabled,
            connection_epoch_state,
            connection_epoch,
            shard_id,
            awaiting_feed_resume_epoch: None,
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

    fn replay_epoch_is_current(&self, expected_epoch: u64) -> bool {
        expected_epoch == self.connection_epoch
            && expected_epoch == self.connection_epoch_state.load(Ordering::SeqCst)
    }

    async fn send_subscribe_market(
        &mut self,
        asset_ids: &[String],
        expected_epoch: Option<u64>,
    ) -> bool {
        if expected_epoch.is_some_and(|epoch| !self.replay_epoch_is_current(epoch)) {
            let active_epoch = self.connection_epoch_state.load(Ordering::SeqCst);
            log::warn!(
                "replay_rejected shard_id={} channel={:?} expected_connection_epoch={:?} handler_connection_epoch={} active_connection_epoch={}",
                self.shard_id,
                self.channel,
                expected_epoch,
                self.connection_epoch,
                active_epoch,
            );
            return false;
        }
        let Some(ref client) = self.client else {
            log::warn!("No client available for market subscribe");
            return false;
        };

        if asset_ids.is_empty() {
            self.new_market_discovery_enabled
                .store(true, Ordering::SeqCst);
        }

        for id in asset_ids {
            self.subscriptions.mark_subscribe(id);
        }

        let payload = if self.market_subscription_initialized {
            serde_json::to_string(&MarketSubscribeRequest {
                assets_ids: asset_ids.to_vec(),
                operation: "subscribe",
                custom_feature_enabled: self.subscribe_new_markets,
            })
        } else {
            serde_json::to_string(&MarketInitialSubscribeRequest {
                assets_ids: asset_ids.to_vec(),
                msg_type: "market",
                custom_feature_enabled: self.subscribe_new_markets,
            })
        };

        match payload {
            Ok(payload) => {
                if let Err(e) = client.send_text(payload, None).await {
                    for id in asset_ids {
                        self.subscriptions.mark_failure(id);
                    }
                    log::error!("Failed to send market subscribe: {e}");
                    return false;
                } else {
                    self.market_subscription_initialized = true;
                    // Polymarket has no server ACK, treat successful send as confirmation
                    for id in asset_ids {
                        self.subscriptions.confirm_subscribe(id);
                    }
                    return true;
                }
            }
            Err(e) => {
                for id in asset_ids {
                    self.subscriptions.mark_failure(id);
                }
                log::error!("Failed to serialize market subscribe request: {e}");
            }
        }
        false
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
            markets: vec![],
            assets_ids: vec![],
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

    async fn resubscribe_all(&mut self) {
        match self.channel {
            WsChannel::Market => {
                let ids = self.subscriptions.all_topics();
                let discovery_enabled = self.new_market_discovery_enabled.load(Ordering::SeqCst);
                let mut replay_frame_count = 0;
                if discovery_enabled
                    && self
                        .send_subscribe_market(&[], Some(self.connection_epoch))
                        .await
                {
                    replay_frame_count += 1;
                }
                if !ids.is_empty()
                    && self
                        .send_subscribe_market(&ids, Some(self.connection_epoch))
                        .await
                {
                    replay_frame_count += 1;
                }
                self.awaiting_feed_resume_epoch =
                    (replay_frame_count > 0).then_some(self.connection_epoch);
                log::info!(
                    "replay_sent shard_id={} channel={:?} connection_epoch={} desired_asset_count={} discovery_enabled={} replay_frame_count={}",
                    self.shard_id,
                    self.channel,
                    self.connection_epoch,
                    ids.len(),
                    discovery_enabled,
                    replay_frame_count,
                );
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

        match self.channel {
            WsChannel::Market => {
                if let Ok(msgs) = serde_json::from_str::<Vec<&RawValue>>(text) {
                    msgs.into_iter()
                        .filter_map(|raw| {
                            match serde_json::from_str::<MarketWsMessage>(raw.get()) {
                                Ok(msg) => Some(PolymarketWsMessage::Market(msg)),
                                Err(e) => {
                                    log::warn!("Failed to parse market WS batch element: {e}");
                                    None
                                }
                            }
                        })
                        .collect()
                } else if let Ok(msg) = serde_json::from_str::<MarketWsMessage>(text) {
                    vec![PolymarketWsMessage::Market(msg)]
                } else {
                    log::warn!("Failed to parse market WS message: {text}");
                    vec![]
                }
            }
            WsChannel::User => {
                if let Ok(msgs) = serde_json::from_str::<Vec<UserWsMessage>>(text) {
                    msgs.into_iter().map(PolymarketWsMessage::User).collect()
                } else if let Ok(msg) = serde_json::from_str::<UserWsMessage>(text) {
                    vec![PolymarketWsMessage::User(msg)]
                } else {
                    log::warn!("Failed to parse user WS message: {text}");
                    vec![]
                }
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<PolymarketWsMessage> {
        if !self.message_buffer.is_empty() {
            return Some(self.message_buffer.remove(0));
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
                            self.send_subscribe_market(&ids, None).await;
                        }
                        HandlerCommand::UnsubscribeMarket(ids) => {
                            for id in &ids {
                                self.subscriptions.mark_unsubscribe(id);
                            }
                            self.send_unsubscribe_market(&ids).await;
                            for id in &ids {
                                self.subscriptions.confirm_unsubscribe(id);
                            }
                        }
                        HandlerCommand::RefreshMarket(ids) => {
                            let retained_count = self.subscriptions.all_topics().len();
                            self.send_unsubscribe_market(&ids).await;
                            let sent = self.send_subscribe_market(&ids, None).await;
                            if sent {
                                log::info!(
                                    "refresh_sent shard_id={} channel={:?} connection_epoch={} asset_count={} retained_desired_count={}",
                                    self.shard_id,
                                    self.channel,
                                    self.connection_epoch,
                                    ids.len(),
                                    retained_count,
                                );
                            } else {
                                log::error!(
                                    "refresh_failed shard_id={} channel={:?} connection_epoch={} asset_count={} retained_desired_count={}",
                                    self.shard_id,
                                    self.channel,
                                    self.connection_epoch,
                                    ids.len(),
                                    retained_count,
                                );
                            }
                        }
                        HandlerCommand::SubscribeUser => {
                            self.user_subscribed = true;
                            self.send_subscribe_user().await;
                        }
                    }
                }
                Some(raw) = self.raw_rx.recv() => {
                    match raw {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                self.connection_epoch += 1;
                                self.connection_epoch_state
                                    .store(self.connection_epoch, Ordering::SeqCst);
                                self.market_subscription_initialized = false;
                                self.resubscribe_all().await;
                                return Some(PolymarketWsMessage::Reconnected);
                            }
                            let msgs = self.parse_messages(&text);
                            if msgs.is_empty() {
                                continue;
                            }
                            // Receiving any user-channel data confirms the server accepted the
                            // credentials; mark auth as successful on the first delivery.
                            if self.channel == WsChannel::User {
                                self.auth_tracker.succeed();
                            } else if let Some(epoch) = self.awaiting_feed_resume_epoch.take() {
                                log::info!(
                                    "feed_resumed shard_id={} channel={:?} connection_epoch={}",
                                    self.shard_id,
                                    self.channel,
                                    epoch,
                                );
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
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;
    use crate::common::enums::PolymarketOrderSide;

    #[fixture]
    fn market_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            WsChannel::Market,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            AuthTracker::new(),
            false,
            false,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(1)),
            1,
            1,
        )
    }

    #[rstest]
    #[tokio::test]
    async fn rejects_replay_from_old_connection_epoch(mut market_handler: FeedHandler) {
        assert!(
            !market_handler
                .send_subscribe_market(&["asset-1".to_string()], Some(0))
                .await
        );
        assert!(market_handler.subscriptions.all_topics().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn rejects_replay_when_shared_connection_epoch_has_advanced(market_handler: FeedHandler) {
        market_handler
            .connection_epoch_state
            .store(2, Ordering::SeqCst);

        assert!(!market_handler.replay_epoch_is_current(1));
        assert!(market_handler.subscriptions.all_topics().is_empty());
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
}

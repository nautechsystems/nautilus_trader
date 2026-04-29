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

//! Message handler for dYdX WebSocket streams.
//!
//! The handler owns the WebSocketClient exclusively and runs in a dedicated
//! Tokio task within the lock-free I/O boundary. It deserializes raw messages
//! into venue-specific types without converting to Nautilus domain objects.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    DydxWsError, DydxWsResult,
    client::DYDX_RATE_LIMIT_KEY_SUBSCRIPTION,
    enums::{DydxWsChannel, DydxWsMessage, DydxWsOutputMessage},
    error::DydxWebSocketError,
    messages::{
        DydxCandle, DydxMarketsContents, DydxOrderbookContents, DydxOrderbookSnapshotContents,
        DydxSubscription, DydxTradeContents, DydxWsBlockHeightMessage, DydxWsCandlesMessage,
        DydxWsChannelBatchDataMsg, DydxWsChannelDataMsg, DydxWsConnectedMsg, DydxWsFeedMessage,
        DydxWsGenericMsg, DydxWsMarketsMessage, DydxWsOrderbookMessage,
        DydxWsParentSubaccountsMessage, DydxWsSubaccountsChannelContents,
        DydxWsSubaccountsChannelData, DydxWsSubaccountsMessage, DydxWsSubaccountsSubscribed,
        DydxWsSubscriptionMsg, DydxWsTradesMessage,
    },
};

/// Commands sent to the feed handler.
#[derive(Debug, Clone)]
pub enum HandlerCommand {
    /// Registers a subscription message for replay.
    RegisterSubscription {
        topic: String,
        subscription: DydxSubscription,
    },
    /// Unregisters a subscription message.
    UnregisterSubscription { topic: String },
    /// Sends a text message via WebSocket.
    SendText(String),
    /// Disconnects the WebSocket client.
    Disconnect,
}

/// Deserializes incoming WebSocket messages into venue-specific types.
///
/// The handler owns the WebSocketClient exclusively within the lock-free I/O boundary,
/// eliminating RwLock contention on the hot path.
pub struct FeedHandler {
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    out_tx: tokio::sync::mpsc::UnboundedSender<DydxWsOutputMessage>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    client: WebSocketClient,
    signal: Arc<AtomicBool>,
    retry_manager: RetryManager<DydxWsError>,
    subscriptions: SubscriptionState,
    subscription_messages: AHashMap<String, DydxSubscription>,
    message_buffer: VecDeque<DydxWsOutputMessage>,
    book_sequence: AHashMap<String, u64>,
}

impl Debug for FeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(FeedHandler))
            .field("subscriptions", &self.subscriptions.len())
            .finish_non_exhaustive()
    }
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`].
    #[must_use]
    pub fn new(
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<DydxWsOutputMessage>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        client: WebSocketClient,
        signal: Arc<AtomicBool>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            cmd_rx,
            out_tx,
            raw_rx,
            client,
            signal,
            retry_manager: create_websocket_retry_manager(),
            subscriptions,
            subscription_messages: AHashMap::new(),
            message_buffer: VecDeque::new(),
            book_sequence: AHashMap::new(),
        }
    }

    async fn send_with_retry(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), DydxWsError> {
        let keys_owned: Option<Vec<Ustr>> = rate_limit_keys.map(|k| k.to_vec());
        self.retry_manager
            .execute_with_retry(
                "websocket_send",
                || {
                    let payload = payload.clone();
                    let keys = keys_owned.clone();
                    async move {
                        self.client
                            .send_text(payload, keys.as_deref())
                            .await
                            .map_err(|e| DydxWsError::ClientError(format!("Send failed: {e}")))
                    }
                },
                should_retry_dydx_error,
                create_dydx_timeout_error,
            )
            .await
    }

    /// Main processing loop for the handler.
    ///
    /// # Panics
    ///
    /// This method will not panic. The `expect` call on `iter.next()` is safe
    /// because we explicitly check that `msgs` is not empty before calling it.
    pub async fn run(&mut self) {
        log::debug!("WebSocket handler started");

        loop {
            // First drain any buffered messages
            if !self.message_buffer.is_empty() {
                let msg = self.message_buffer.pop_front().unwrap();
                if self.out_tx.send(msg).is_err() {
                    log::debug!("Receiver dropped, stopping handler");
                    break;
                }
                continue;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    if self.handle_command(cmd).await {
                        break;
                    }
                }

                Some(msg) = self.raw_rx.recv() => {
                    log::trace!("Handler received raw message");
                    let msgs = self.process_raw_message(msg).await;
                    if !msgs.is_empty() {
                        let mut iter = msgs.into_iter();
                        // We just checked that msgs is not empty
                        let first = iter.next().expect("non-empty vec has first element");
                        self.message_buffer.extend(iter);
                        log::trace!("Handler sending message: {:?}", std::mem::discriminant(&first));
                        if self.out_tx.send(first).is_err() {
                            log::debug!("Receiver dropped, stopping handler");
                            break;
                        }
                    }
                }

                else => {
                    log::debug!("Handler shutting down: channels closed");
                    break;
                }
            }

            if self.signal.load(Ordering::Acquire) {
                log::debug!("Handler received stop signal");
                break;
            }
        }
    }

    async fn process_raw_message(&mut self, msg: Message) -> Vec<DydxWsOutputMessage> {
        match msg {
            Message::Text(txt) => {
                if txt == RECONNECTED {
                    self.clear_state();

                    if let Err(e) = self.replay_subscriptions().await {
                        log::error!("Failed to replay subscriptions after reconnect: {e}");
                    }
                    return vec![DydxWsOutputMessage::Reconnected];
                }

                // Hot path: zero-copy parse for feed messages (orderbook/trades/candles)
                match serde_json::from_str::<DydxWsFeedMessage>(&txt) {
                    Ok(feed_msg) => {
                        return self.handle_feed_message(feed_msg);
                    }
                    Err(e) => {
                        if txt.contains("v4_subaccounts") {
                            log::warn!(
                                "[WS_DESER] Failed to parse v4_subaccounts as DydxWsFeedMessage: {e}\nRaw: {txt}"
                            );
                        }
                    }
                }

                // Cold path: infrequent control messages (connected/subscribed/error)
                match serde_json::from_str::<serde_json::Value>(&txt) {
                    Ok(val) => match serde_json::from_value::<DydxWsGenericMsg>(val.clone()) {
                        Ok(meta) => {
                            let result = if meta.is_connected() {
                                serde_json::from_value::<DydxWsConnectedMsg>(val)
                                    .map(DydxWsMessage::Connected)
                            } else if meta.is_subscribed() {
                                log::debug!("Processing subscribed message via fallback path");

                                if let Ok(sub_msg) =
                                    serde_json::from_value::<DydxWsSubscriptionMsg>(val.clone())
                                {
                                    if sub_msg.channel == DydxWsChannel::Subaccounts {
                                        log::debug!("Parsing subaccounts subscription (fallback)");
                                        serde_json::from_value::<DydxWsSubaccountsSubscribed>(val)
                                            .map(DydxWsMessage::SubaccountsSubscribed)
                                            .or_else(|e| {
                                                log::warn!(
                                                    "Failed to parse subaccounts subscription: {e}"
                                                );
                                                Ok(DydxWsMessage::Subscribed(sub_msg))
                                            })
                                    } else {
                                        Ok(DydxWsMessage::Subscribed(sub_msg))
                                    }
                                } else {
                                    serde_json::from_value::<DydxWsSubscriptionMsg>(val)
                                        .map(DydxWsMessage::Subscribed)
                                }
                            } else if meta.is_unsubscribed() {
                                serde_json::from_value::<DydxWsSubscriptionMsg>(val)
                                    .map(DydxWsMessage::Unsubscribed)
                            } else if meta.is_error() {
                                serde_json::from_value::<DydxWebSocketError>(val)
                                    .map(DydxWsMessage::Error)
                            } else if meta.is_unknown() {
                                log::warn!("Received unknown WebSocket message type: {txt}",);
                                Ok(DydxWsMessage::Raw(val))
                            } else {
                                Ok(DydxWsMessage::Raw(val))
                            };

                            match result {
                                Ok(dydx_msg) => self.handle_dydx_message(dydx_msg).await,
                                Err(e) => {
                                    log::error!(
                                        "Failed to parse WebSocket message: {e}. Message type: {:?}, Channel: {:?}. Raw: {txt}",
                                        meta.msg_type,
                                        meta.channel,
                                    );
                                    vec![]
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to parse WebSocket message envelope (DydxWsGenericMsg): {e}\nRaw JSON:\n{txt}"
                            );
                            vec![]
                        }
                    },
                    Err(e) => {
                        let err = DydxWebSocketError::from_message(e.to_string());
                        vec![DydxWsOutputMessage::Error(err)]
                    }
                }
            }
            Message::Pong(_data) => vec![],
            Message::Ping(_data) => vec![],
            Message::Binary(_bin) => vec![],
            Message::Close(_frame) => {
                log::info!("WebSocket close frame received");
                vec![]
            }
            Message::Frame(_) => vec![],
        }
    }

    async fn handle_dydx_message(&mut self, msg: DydxWsMessage) -> Vec<DydxWsOutputMessage> {
        match self.handle_message(msg).await {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error handling message: {e}");
                vec![]
            }
        }
    }

    fn handle_feed_message(&mut self, feed_msg: DydxWsFeedMessage) -> Vec<DydxWsOutputMessage> {
        log::trace!(
            "Handling feed message: {:?}",
            std::mem::discriminant(&feed_msg)
        );

        match feed_msg {
            DydxWsFeedMessage::Subaccounts(msg) => self.handle_subaccounts(msg),
            DydxWsFeedMessage::Orderbook(msg) => self.handle_orderbook(msg),
            DydxWsFeedMessage::Trades(msg) => self.handle_trades(msg),
            DydxWsFeedMessage::Markets(msg) => self.handle_markets_feed(msg),
            DydxWsFeedMessage::Candles(msg) => self.handle_candles_feed(msg),
            DydxWsFeedMessage::ParentSubaccounts(msg) => self.handle_parent_subaccounts(msg),
            DydxWsFeedMessage::BlockHeight(msg) => self.handle_block_height_feed(msg),
        }
    }

    fn handle_subaccounts(&self, msg: DydxWsSubaccountsMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsSubaccountsMessage::Subscribed(data) => {
                let topic =
                    self.topic_from_msg(&DydxWsChannel::Subaccounts, &Some(data.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);
                log::debug!("Forwarding subaccount subscription to execution client");
                vec![DydxWsOutputMessage::SubaccountSubscribed(Box::new(data))]
            }
            DydxWsSubaccountsMessage::ChannelData(data) => {
                let has_orders = data.contents.orders.as_ref().is_some_and(|o| !o.is_empty());
                let has_fills = data.contents.fills.as_ref().is_some_and(|f| !f.is_empty());

                if has_orders || has_fills {
                    log::debug!(
                        "Received {} order(s), {} fill(s) - forwarding to execution client",
                        data.contents.orders.as_ref().map_or(0, |o| o.len()),
                        data.contents.fills.as_ref().map_or(0, |f| f.len())
                    );
                    vec![DydxWsOutputMessage::SubaccountsChannelData(Box::new(data))]
                } else {
                    vec![]
                }
            }
            DydxWsSubaccountsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Subaccounts, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_orderbook(&mut self, msg: DydxWsOrderbookMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsOrderbookMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Orderbook, &data.id);
                self.subscriptions.confirm_subscribe(&topic);

                if let Some(id) = &data.id {
                    self.book_sequence.insert(id.clone(), data.message_id);
                }

                self.deserialize_orderbook_snapshot(&data)
            }
            DydxWsOrderbookMessage::ChannelData(data) => {
                if let Some(id) = &data.id {
                    if let Some(last_id) = self.book_sequence.get(id)
                        && data.message_id <= *last_id
                    {
                        log::warn!(
                            "Orderbook sequence regression for {id}: last {last_id}, received {}",
                            data.message_id
                        );
                    }
                    self.book_sequence.insert(id.clone(), data.message_id);
                }
                self.deserialize_orderbook_update(&data)
            }
            DydxWsOrderbookMessage::ChannelBatchData(data) => {
                if let Some(id) = &data.id {
                    if let Some(last_id) = self.book_sequence.get(id)
                        && data.message_id <= *last_id
                    {
                        log::warn!(
                            "Orderbook batch sequence regression for {id}: last {last_id}, received {}",
                            data.message_id
                        );
                    }
                    self.book_sequence.insert(id.clone(), data.message_id);
                }
                self.deserialize_orderbook_batch(&data)
            }
            DydxWsOrderbookMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Orderbook, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);

                if let Some(id) = &data.id {
                    self.book_sequence.remove(id);
                }
                vec![]
            }
        }
    }

    fn handle_trades(&self, msg: DydxWsTradesMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsTradesMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Trades, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.deserialize_trades(&data)
            }
            DydxWsTradesMessage::ChannelData(data) => self.deserialize_trades(&data),
            DydxWsTradesMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Trades, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_markets_feed(&self, msg: DydxWsMarketsMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsMarketsMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Markets, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.deserialize_markets(&data)
            }
            DydxWsMarketsMessage::ChannelData(data) => self.deserialize_markets(&data),
            DydxWsMarketsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Markets, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_candles_feed(&self, msg: DydxWsCandlesMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsCandlesMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Candles, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                vec![]
            }
            DydxWsCandlesMessage::ChannelData(data) => self.deserialize_candles(&data),
            DydxWsCandlesMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Candles, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_parent_subaccounts(
        &self,
        msg: DydxWsParentSubaccountsMessage,
    ) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsParentSubaccountsMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::ParentSubaccounts, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.deserialize_parent_subaccounts(&data)
            }
            DydxWsParentSubaccountsMessage::ChannelData(data) => {
                self.deserialize_parent_subaccounts(&data)
            }
            DydxWsParentSubaccountsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::ParentSubaccounts, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_block_height_feed(&self, msg: DydxWsBlockHeightMessage) -> Vec<DydxWsOutputMessage> {
        match msg {
            DydxWsBlockHeightMessage::Subscribed(data) => {
                let topic =
                    self.topic_from_msg(&DydxWsChannel::BlockHeight, &Some(data.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);

                match data.contents.height.parse::<u64>() {
                    Ok(height) => vec![DydxWsOutputMessage::BlockHeight {
                        height,
                        time: data.contents.time,
                    }],
                    Err(e) => {
                        log::warn!("Failed to parse block height from subscription: {e}");
                        vec![]
                    }
                }
            }
            DydxWsBlockHeightMessage::ChannelData(data) => {
                match data.contents.block_height.parse::<u64>() {
                    Ok(height) => vec![DydxWsOutputMessage::BlockHeight {
                        height,
                        time: data.contents.time,
                    }],
                    Err(e) => {
                        log::warn!("Failed to parse block height from channel data: {e}");
                        vec![]
                    }
                }
            }
            DydxWsBlockHeightMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::BlockHeight, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn deserialize_trades(&self, data: &DydxWsChannelDataMsg) -> Vec<DydxWsOutputMessage> {
        let Some(id) = data.id.clone() else {
            log::error!("Missing id for trades channel");
            return vec![];
        };

        match serde_json::from_value::<DydxTradeContents>(data.contents.clone()) {
            Ok(contents) => vec![DydxWsOutputMessage::Trades { id, contents }],
            Err(e) => {
                log::error!("Failed to deserialize trade contents: {e}");
                vec![]
            }
        }
    }

    fn deserialize_orderbook_snapshot(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> Vec<DydxWsOutputMessage> {
        let Some(id) = data.id.clone() else {
            log::error!("Missing id for orderbook snapshot");
            return vec![];
        };

        match serde_json::from_value::<DydxOrderbookSnapshotContents>(data.contents.clone()) {
            Ok(contents) => vec![DydxWsOutputMessage::OrderbookSnapshot { id, contents }],
            Err(e) => {
                log::error!("Failed to deserialize orderbook snapshot: {e}");
                vec![]
            }
        }
    }

    fn deserialize_orderbook_update(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> Vec<DydxWsOutputMessage> {
        let Some(id) = data.id.clone() else {
            log::error!("Missing id for orderbook update");
            return vec![];
        };

        match serde_json::from_value::<DydxOrderbookContents>(data.contents.clone()) {
            Ok(contents) => vec![DydxWsOutputMessage::OrderbookUpdate { id, contents }],
            Err(e) => {
                log::error!("Failed to deserialize orderbook contents: {e}");
                vec![]
            }
        }
    }

    fn deserialize_orderbook_batch(
        &self,
        data: &DydxWsChannelBatchDataMsg,
    ) -> Vec<DydxWsOutputMessage> {
        let Some(id) = data.id.clone() else {
            log::error!("Missing id for orderbook batch");
            return vec![];
        };

        match serde_json::from_value::<Vec<DydxOrderbookContents>>(data.contents.clone()) {
            Ok(updates) => vec![DydxWsOutputMessage::OrderbookBatch { id, updates }],
            Err(e) => {
                log::error!("Failed to deserialize orderbook batch: {e}");
                vec![]
            }
        }
    }

    fn deserialize_candles(&self, data: &DydxWsChannelDataMsg) -> Vec<DydxWsOutputMessage> {
        let Some(id) = data.id.clone() else {
            log::error!("Missing id for candles channel");
            return vec![];
        };

        match serde_json::from_value::<DydxCandle>(data.contents.clone()) {
            Ok(contents) => vec![DydxWsOutputMessage::Candles { id, contents }],
            Err(e) => {
                log::error!("Failed to deserialize candle contents: {e}");
                vec![]
            }
        }
    }

    fn deserialize_markets(&self, data: &DydxWsChannelDataMsg) -> Vec<DydxWsOutputMessage> {
        match serde_json::from_value::<DydxMarketsContents>(data.contents.clone()) {
            Ok(contents) => vec![DydxWsOutputMessage::Markets(contents)],
            Err(e) => {
                log::error!("Failed to deserialize markets contents: {e}");
                vec![]
            }
        }
    }

    fn deserialize_parent_subaccounts(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> Vec<DydxWsOutputMessage> {
        match serde_json::from_value::<DydxWsSubaccountsChannelContents>(data.contents.clone()) {
            Ok(contents) => {
                let has_orders = contents.orders.as_ref().is_some_and(|o| !o.is_empty());
                let has_fills = contents.fills.as_ref().is_some_and(|f| !f.is_empty());

                if has_orders || has_fills {
                    let channel_data = DydxWsSubaccountsChannelData {
                        connection_id: data.connection_id.clone(),
                        message_id: data.message_id,
                        id: data.id.clone().unwrap_or_default(),
                        version: data.version.clone().unwrap_or_default(),
                        contents,
                    };
                    vec![DydxWsOutputMessage::SubaccountsChannelData(Box::new(
                        channel_data,
                    ))]
                } else {
                    vec![]
                }
            }
            Err(e) => {
                log::error!("Failed to deserialize parent subaccounts contents: {e}");
                vec![]
            }
        }
    }

    async fn handle_command(&mut self, command: HandlerCommand) -> bool {
        match command {
            HandlerCommand::RegisterSubscription {
                topic,
                subscription,
            } => {
                self.subscription_messages.insert(topic, subscription);
            }
            HandlerCommand::UnregisterSubscription { topic } => {
                self.subscription_messages.remove(&topic);
            }
            HandlerCommand::SendText(text) => {
                if let Err(e) = self
                    .send_with_retry(text, Some(DYDX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
                    .await
                {
                    log::error!("Failed to send WebSocket text after retries: {e}");
                }
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");
                self.client.disconnect().await;
                return true;
            }
        }
        false
    }

    fn topic_from_msg(&self, channel: &DydxWsChannel, id: &Option<String>) -> String {
        match id {
            Some(id) => format!(
                "{}{}{}",
                channel.as_ref(),
                self.subscriptions.delimiter(),
                id
            ),
            None => channel.as_ref().to_string(),
        }
    }

    fn clear_state(&mut self) {
        let buffer_count = self.message_buffer.len();
        let seq_count = self.book_sequence.len();
        self.message_buffer.clear();
        self.book_sequence.clear();
        log::debug!(
            "Cleared reconnect state: message_buffer={buffer_count}, book_sequence={seq_count}"
        );
    }

    async fn replay_subscriptions(&self) -> DydxWsResult<()> {
        let topics = self.subscriptions.all_topics();
        for topic in topics {
            let Some(subscription) = self.subscription_messages.get(&topic).cloned() else {
                log::warn!("No preserved subscription message for topic: {topic}");
                continue;
            };

            let payload = serde_json::to_string(&subscription)?;
            self.subscriptions.mark_subscribe(&topic);

            if let Err(e) = self
                .send_with_retry(payload, Some(DYDX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
                .await
            {
                self.subscriptions.mark_failure(&topic);
                return Err(e);
            }
        }

        Ok(())
    }

    /// Handles control messages from the fallback parsing path.
    ///
    /// Channel data is handled directly via `handle_feed_message()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be processed.
    pub async fn handle_message(
        &mut self,
        msg: DydxWsMessage,
    ) -> DydxWsResult<Vec<DydxWsOutputMessage>> {
        match msg {
            DydxWsMessage::Connected(_) => {
                log::info!("dYdX WebSocket connected");
                Ok(vec![])
            }
            DydxWsMessage::Subscribed(sub) => {
                log::debug!("Subscribed to {} (id: {:?})", sub.channel, sub.id);
                let topic = self.topic_from_msg(&sub.channel, &sub.id);
                self.subscriptions.confirm_subscribe(&topic);
                Ok(vec![])
            }
            DydxWsMessage::SubaccountsSubscribed(msg) => {
                log::debug!("Subaccounts subscribed with initial state (fallback path)");
                let topic = self.topic_from_msg(&DydxWsChannel::Subaccounts, &Some(msg.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);
                Ok(vec![DydxWsOutputMessage::SubaccountSubscribed(Box::new(
                    msg,
                ))])
            }
            DydxWsMessage::Unsubscribed(unsub) => {
                log::debug!("Unsubscribed from {} (id: {:?})", unsub.channel, unsub.id);
                let topic = self.topic_from_msg(&unsub.channel, &unsub.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                Ok(vec![])
            }
            DydxWsMessage::Error(err) => Ok(vec![DydxWsOutputMessage::Error(err)]),
            DydxWsMessage::Reconnected => {
                self.clear_state();

                if let Err(e) = self.replay_subscriptions().await {
                    log::error!("Failed to replay subscriptions after reconnect message: {e}");
                }
                Ok(vec![DydxWsOutputMessage::Reconnected])
            }
            DydxWsMessage::Pong => Ok(vec![]),
            DydxWsMessage::Raw(_) => Ok(vec![]),
        }
    }
}

/// Determines if a dYdX WebSocket error should trigger a retry.
fn should_retry_dydx_error(error: &DydxWsError) -> bool {
    match error {
        DydxWsError::Transport(_) => true,
        DydxWsError::Send(_) => true,
        DydxWsError::ClientError(msg) => {
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        DydxWsError::NotConnected
        | DydxWsError::Json(_)
        | DydxWsError::Parse(_)
        | DydxWsError::Authentication(_)
        | DydxWsError::Subscription(_)
        | DydxWsError::Venue(_) => false,
    }
}

/// Creates a timeout error for the retry manager.
fn create_dydx_timeout_error(msg: String) -> DydxWsError {
    DydxWsError::ClientError(msg)
}

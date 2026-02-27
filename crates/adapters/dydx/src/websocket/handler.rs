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
//! This module processes incoming WebSocket messages and converts them into
//! Nautilus domain objects.
//!
//! The handler owns the WebSocketClient exclusively and runs in a dedicated
//! Tokio task within the lock-free I/O boundary.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        Bar, BarType, Data, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas,
    },
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{SubscriptionState, WebSocketClient},
};
use rust_decimal::Decimal;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    DydxWsError, DydxWsResult,
    client::DYDX_RATE_LIMIT_KEY_SUBSCRIPTION,
    enums::{DydxWsChannel, DydxWsMessage, NautilusWsMessage},
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
    parse as ws_parse,
};
use crate::common::parse::parse_instrument_id;

/// Commands sent to the feed handler.
#[derive(Debug, Clone)]
pub enum HandlerCommand {
    /// Update a single instrument in the cache.
    UpdateInstrument(Box<InstrumentAny>),
    /// Initialize instruments in bulk.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Register a bar type for candle subscriptions.
    RegisterBarType { topic: String, bar_type: BarType },
    /// Unregister a bar type for candle subscriptions.
    UnregisterBarType { topic: String },
    /// Register a subscription message for replay.
    RegisterSubscription {
        topic: String,
        subscription: DydxSubscription,
    },
    /// Unregister a subscription message.
    UnregisterSubscription { topic: String },
    /// Send a text message via WebSocket.
    SendText(String),
}

/// Processes incoming WebSocket messages and converts them to Nautilus domain objects.
///
/// The handler owns the WebSocketClient exclusively within the lock-free I/O boundary,
/// eliminating RwLock contention on the hot path.
pub struct FeedHandler {
    /// Account ID for parsing account-specific messages.
    account_id: Option<AccountId>,
    /// Command receiver from outer client.
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    /// Output sender for Nautilus messages.
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    /// Raw WebSocket message receiver.
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    /// Owned WebSocket client (no RwLock).
    client: WebSocketClient,
    /// Manual disconnect signal.
    signal: Arc<AtomicBool>,
    /// Retry manager for WebSocket send operations.
    retry_manager: RetryManager<DydxWsError>,
    /// Cached instruments for parsing market data.
    instruments: AHashMap<Ustr, InstrumentAny>,
    /// Cached bar types by topic (e.g., "BTC-USD/1MIN").
    bar_types: AHashMap<String, BarType>,
    /// Subscription state shared with the outer client for replay/acks.
    subscriptions: SubscriptionState,
    /// Original subscription messages by topic (for replay without reconstruction).
    subscription_messages: AHashMap<String, DydxSubscription>,
    /// Buffer for multiple messages produced from a single raw message.
    message_buffer: VecDeque<NautilusWsMessage>,
    /// Tracks last seen message_id per orderbook topic for gap detection.
    book_sequence: AHashMap<String, u64>,
    /// Pending (incomplete) bars per candle topic for emit-on-next logic.
    pending_bars: AHashMap<String, Bar>,
    /// Whether to timestamp bars at close time (open + interval).
    bars_timestamp_on_close: bool,
    /// High-resolution clock for timestamps.
    clock: &'static AtomicTime,
}

impl Debug for FeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(FeedHandler))
            .field("account_id", &self.account_id)
            .field("instruments_count", &self.instruments.len())
            .field("bar_types_count", &self.bar_types.len())
            .finish_non_exhaustive()
    }
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: Option<AccountId>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        client: WebSocketClient,
        signal: Arc<AtomicBool>,
        subscriptions: SubscriptionState,
        bars_timestamp_on_close: bool,
    ) -> Self {
        Self {
            account_id,
            cmd_rx,
            out_tx,
            raw_rx,
            client,
            signal,
            retry_manager: create_websocket_retry_manager(),
            instruments: AHashMap::new(),
            bar_types: AHashMap::new(),
            subscriptions,
            subscription_messages: AHashMap::new(),
            message_buffer: VecDeque::new(),
            book_sequence: AHashMap::new(),
            pending_bars: AHashMap::new(),
            bars_timestamp_on_close,
            clock: get_atomic_clock_realtime(),
        }
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
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
    /// because we explicitly check that `nautilus_msgs` is not empty before
    /// calling it.
    pub async fn run(&mut self) {
        log::debug!("WebSocket handler started");
        loop {
            // First drain any buffered messages
            if !self.message_buffer.is_empty() {
                let nautilus_msg = self.message_buffer.pop_front().unwrap();
                if self.out_tx.send(nautilus_msg).is_err() {
                    log::debug!("Receiver dropped, stopping handler");
                    break;
                }
                continue;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                Some(msg) = self.raw_rx.recv() => {
                    log::trace!("Handler received raw message");
                    let nautilus_msgs = self.process_raw_message(msg).await;
                    if !nautilus_msgs.is_empty() {
                        let mut iter = nautilus_msgs.into_iter();
                        // We just checked that nautilus_msgs is not empty
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

            if self.signal.load(Ordering::Relaxed) {
                log::debug!("Handler received stop signal");
                break;
            }
        }
    }

    async fn process_raw_message(&mut self, msg: Message) -> Vec<NautilusWsMessage> {
        match msg {
            Message::Text(txt) => {
                if txt == RECONNECTED {
                    self.clear_state();

                    if let Err(e) = self.replay_subscriptions().await {
                        log::error!("Failed to replay subscriptions after reconnect: {e}");
                    }
                    return vec![NautilusWsMessage::Reconnected];
                }

                // Hot path: zero-copy parse for feed messages (orderbook/trades/candles)
                match serde_json::from_str::<DydxWsFeedMessage>(&txt) {
                    Ok(feed_msg) => {
                        return self.handle_feed_message(feed_msg);
                    }
                    Err(e) => {
                        // Log subaccounts channel failures at warn level for diagnosis
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
                        vec![NautilusWsMessage::Error(err)]
                    }
                }
            }
            Message::Pong(_data) => vec![],
            Message::Ping(_data) => vec![], // Handled by lower layers
            Message::Binary(_bin) => vec![], // dYdX uses text frames
            Message::Close(_frame) => {
                log::info!("WebSocket close frame received");
                vec![]
            }
            Message::Frame(_) => vec![],
        }
    }

    async fn handle_dydx_message(&mut self, msg: DydxWsMessage) -> Vec<NautilusWsMessage> {
        match self.handle_message(msg).await {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error handling message: {e}");
                vec![]
            }
        }
    }

    fn handle_feed_message(&mut self, feed_msg: DydxWsFeedMessage) -> Vec<NautilusWsMessage> {
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

    fn handle_subaccounts(&self, msg: DydxWsSubaccountsMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsSubaccountsMessage::Subscribed(data) => {
                let topic =
                    self.topic_from_msg(&DydxWsChannel::Subaccounts, &Some(data.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);
                self.process_subaccounts_subscribed(&data)
            }
            DydxWsSubaccountsMessage::ChannelData(data) => {
                self.process_subaccounts_channel_data(data)
            }
            DydxWsSubaccountsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Subaccounts, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_orderbook(&mut self, msg: DydxWsOrderbookMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsOrderbookMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Orderbook, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                // Reset sequence tracking on snapshot
                if let Some(id) = &data.id {
                    self.book_sequence.insert(id.clone(), data.message_id);
                }
                self.parse_orderbook_from_data(&data, true)
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
                self.parse_orderbook_from_data(&data, false)
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
                self.parse_orderbook_batch_from_data(&data)
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

    fn handle_trades(&self, msg: DydxWsTradesMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsTradesMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Trades, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.parse_trades_from_data(&data)
            }
            DydxWsTradesMessage::ChannelData(data) => self.parse_trades_from_data(&data),
            DydxWsTradesMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Trades, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_markets_feed(&self, msg: DydxWsMarketsMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsMarketsMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Markets, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.parse_markets_from_data(&data)
            }
            DydxWsMarketsMessage::ChannelData(data) => self.parse_markets_from_data(&data),
            DydxWsMarketsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Markets, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_candles_feed(&mut self, msg: DydxWsCandlesMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsCandlesMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::Candles, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                vec![]
            }
            DydxWsCandlesMessage::ChannelData(data) => self.parse_candles_from_data(&data),
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
    ) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsParentSubaccountsMessage::Subscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::ParentSubaccounts, &data.id);
                self.subscriptions.confirm_subscribe(&topic);
                self.parse_parent_subaccounts_from_data(&data)
            }
            DydxWsParentSubaccountsMessage::ChannelData(data) => {
                self.parse_parent_subaccounts_from_data(&data)
            }
            DydxWsParentSubaccountsMessage::Unsubscribed(data) => {
                let topic = self.topic_from_msg(&DydxWsChannel::ParentSubaccounts, &data.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                vec![]
            }
        }
    }

    fn handle_block_height_feed(&self, msg: DydxWsBlockHeightMessage) -> Vec<NautilusWsMessage> {
        match msg {
            DydxWsBlockHeightMessage::Subscribed(data) => {
                let topic =
                    self.topic_from_msg(&DydxWsChannel::BlockHeight, &Some(data.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);
                match data.contents.height.parse::<u64>() {
                    Ok(height) => vec![NautilusWsMessage::BlockHeight {
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
                    Ok(height) => vec![NautilusWsMessage::BlockHeight {
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

    fn process_subaccounts_subscribed(
        &self,
        msg: &DydxWsSubaccountsSubscribed,
    ) -> Vec<NautilusWsMessage> {
        log::debug!("Forwarding subaccount subscription to execution client");
        vec![NautilusWsMessage::SubaccountSubscribed(Box::new(
            msg.clone(),
        ))]
    }

    fn process_subaccounts_channel_data(
        &self,
        data: DydxWsSubaccountsChannelData,
    ) -> Vec<NautilusWsMessage> {
        let has_orders = data.contents.orders.as_ref().is_some_and(|o| !o.is_empty());
        let has_fills = data.contents.fills.as_ref().is_some_and(|f| !f.is_empty());

        if has_orders || has_fills {
            log::debug!(
                "Received {} order(s), {} fill(s) - forwarding to execution client",
                data.contents.orders.as_ref().map_or(0, |o| o.len()),
                data.contents.fills.as_ref().map_or(0, |f| f.len())
            );
            vec![NautilusWsMessage::SubaccountsChannelData(Box::new(data))]
        } else {
            vec![]
        }
    }

    fn parse_trades_from_data(&self, data: &DydxWsChannelDataMsg) -> Vec<NautilusWsMessage> {
        match self.parse_trades(data) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing trades: {e}");
                vec![]
            }
        }
    }

    fn parse_orderbook_from_data(
        &mut self,
        data: &DydxWsChannelDataMsg,
        is_snapshot: bool,
    ) -> Vec<NautilusWsMessage> {
        match self.parse_orderbook(data, is_snapshot) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing orderbook: {e}");
                vec![]
            }
        }
    }

    fn parse_orderbook_batch_from_data(
        &mut self,
        data: &DydxWsChannelBatchDataMsg,
    ) -> Vec<NautilusWsMessage> {
        match self.parse_orderbook_batch(data) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing orderbook batch: {e}");
                vec![]
            }
        }
    }

    fn parse_markets_from_data(&self, data: &DydxWsChannelDataMsg) -> Vec<NautilusWsMessage> {
        match self.parse_markets(data) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing markets: {e}");
                vec![]
            }
        }
    }

    fn parse_candles_from_data(&mut self, data: &DydxWsChannelDataMsg) -> Vec<NautilusWsMessage> {
        match self.parse_candles(data) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing candles: {e}");
                vec![]
            }
        }
    }

    fn parse_parent_subaccounts_from_data(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> Vec<NautilusWsMessage> {
        match self.parse_subaccounts(data) {
            Ok(msgs) => msgs,
            Err(e) => {
                log::error!("Error parsing parent subaccounts: {e}");
                vec![]
            }
        }
    }

    async fn handle_command(&mut self, command: HandlerCommand) {
        match command {
            HandlerCommand::UpdateInstrument(instrument) => {
                let symbol = instrument.id().symbol.inner();
                self.instruments.insert(symbol, *instrument);
            }
            HandlerCommand::InitializeInstruments(instruments) => {
                log::debug!(
                    "Initializing {} instruments in WebSocket handler",
                    instruments.len()
                );
                for instrument in instruments {
                    let symbol = instrument.id().symbol.inner();
                    self.instruments.insert(symbol, instrument);
                }
                log::debug!(
                    "Handler now has {} instruments cached",
                    self.instruments.len()
                );
            }
            HandlerCommand::RegisterBarType { topic, bar_type } => {
                self.bar_types.insert(topic, bar_type);
            }
            HandlerCommand::UnregisterBarType { topic } => {
                self.bar_types.remove(&topic);
            }
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
        }
    }

    /// Registers a bar type for a specific topic (e.g., "BTC-USD/1MIN").
    pub fn register_bar_type(&mut self, topic: String, bar_type: BarType) {
        self.bar_types.insert(topic, bar_type);
    }

    /// Unregisters a bar type for a specific topic.
    pub fn unregister_bar_type(&mut self, topic: &str) {
        self.bar_types.remove(topic);
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
        let bars_count = self.pending_bars.len();
        self.message_buffer.clear();
        self.book_sequence.clear();
        self.pending_bars.clear();
        log::debug!(
            "Cleared reconnect state: message_buffer={buffer_count}, \
             book_sequence={seq_count}, pending_bars={bars_count}"
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
    #[allow(clippy::result_large_err)]
    pub async fn handle_message(
        &mut self,
        msg: DydxWsMessage,
    ) -> DydxWsResult<Vec<NautilusWsMessage>> {
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
                Ok(self.process_subaccounts_subscribed(&msg))
            }
            DydxWsMessage::Unsubscribed(unsub) => {
                log::debug!("Unsubscribed from {} (id: {:?})", unsub.channel, unsub.id);
                let topic = self.topic_from_msg(&unsub.channel, &unsub.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                Ok(vec![])
            }
            DydxWsMessage::Error(err) => Ok(vec![NautilusWsMessage::Error(err)]),
            DydxWsMessage::Reconnected => {
                self.clear_state();

                if let Err(e) = self.replay_subscriptions().await {
                    log::error!("Failed to replay subscriptions after reconnect message: {e}");
                }
                Ok(vec![NautilusWsMessage::Reconnected])
            }
            DydxWsMessage::Pong => Ok(vec![]),
            DydxWsMessage::Raw(_) => Ok(vec![]),
        }
    }

    fn parse_trades(&self, data: &DydxWsChannelDataMsg) -> DydxWsResult<Vec<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for trades channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let contents: DydxTradeContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade contents: {e}")))?;

        let ts_init = self.generate_ts_init();
        let ticks = ws_parse::parse_trade_ticks(instrument_id, instrument, &contents, ts_init)?;

        if ticks.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![NautilusWsMessage::Data(ticks)])
        }
    }

    fn parse_orderbook(
        &mut self,
        data: &DydxWsChannelDataMsg,
        is_snapshot: bool,
    ) -> DydxWsResult<Vec<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;
        let price_prec = instrument.price_precision();
        let size_prec = instrument.size_precision();

        let ts_init = self.generate_ts_init();

        if is_snapshot {
            let contents: DydxOrderbookSnapshotContents =
                serde_json::from_value(data.contents.clone()).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook snapshot: {e}"))
                })?;

            let deltas = ws_parse::parse_orderbook_snapshot(
                &instrument_id,
                &contents,
                price_prec,
                size_prec,
                ts_init,
            )?;

            Ok(vec![NautilusWsMessage::Deltas(Box::new(deltas))])
        } else {
            let contents: DydxOrderbookContents = serde_json::from_value(data.contents.clone())
                .map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook contents: {e}"))
                })?;

            let deltas = ws_parse::parse_orderbook_deltas(
                &instrument_id,
                &contents,
                price_prec,
                size_prec,
                ts_init,
            )?;

            Ok(vec![NautilusWsMessage::Deltas(Box::new(deltas))])
        }
    }

    fn parse_orderbook_batch(
        &mut self,
        data: &DydxWsChannelBatchDataMsg,
    ) -> DydxWsResult<Vec<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook batch channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;
        let price_prec = instrument.price_precision();
        let size_prec = instrument.size_precision();

        let contents: Vec<DydxOrderbookContents> = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse orderbook batch: {e}")))?;

        let ts_init = self.generate_ts_init();
        let mut all_deltas = Vec::new();

        let num_messages = contents.len();
        for (idx, content) in contents.iter().enumerate() {
            let is_last_message = idx == num_messages - 1;
            let deltas = ws_parse::parse_orderbook_deltas_with_flag(
                &instrument_id,
                content,
                price_prec,
                size_prec,
                ts_init,
                is_last_message,
            )?;
            all_deltas.extend(deltas);
        }

        let deltas = OrderBookDeltas::new(instrument_id, all_deltas);
        Ok(vec![NautilusWsMessage::Deltas(Box::new(deltas))])
    }

    fn parse_candles(
        &mut self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Vec<NautilusWsMessage>> {
        let topic = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for candles channel".into()))?;

        let bar_type = *self.bar_types.get(topic).ok_or_else(|| {
            DydxWsError::Parse(format!("No bar type registered for topic: {topic}"))
        })?;

        let candle: DydxCandle = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse candle contents: {e}")))?;

        let instrument_id = self.parse_instrument_id(&candle.ticker)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let ts_init = self.generate_ts_init();
        let bar = ws_parse::parse_candle_bar(
            bar_type,
            instrument,
            &candle,
            self.bars_timestamp_on_close,
            ts_init,
        )?;

        // Emit-on-next: only emit a bar when a new candle period arrives,
        // confirming the previous bar is closed
        let topic_key = topic.clone();
        if let Some(pending) = self.pending_bars.get(&topic_key) {
            if pending.ts_event != bar.ts_event {
                // New candle period - emit the previous (now closed) bar
                let closed_bar = *pending;
                self.pending_bars.insert(topic_key, bar);
                return Ok(vec![NautilusWsMessage::Data(vec![Data::Bar(closed_bar)])]);
            }
            // Same candle period - update pending with latest values
            self.pending_bars.insert(topic_key, bar);
            return Ok(vec![]);
        }

        // No pending bar yet - store as pending, emit nothing
        self.pending_bars.insert(topic_key, bar);
        Ok(vec![])
    }

    fn parse_markets(&self, data: &DydxWsChannelDataMsg) -> DydxWsResult<Vec<NautilusWsMessage>> {
        let contents: DydxMarketsContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse markets contents: {e}")))?;

        let mut messages = Vec::new();
        let ts_init = self.generate_ts_init();

        // Parse oracle prices → MarkPriceUpdate + IndexPriceUpdate
        if let Some(oracle_prices) = &contents.oracle_prices {
            for (symbol_str, oracle_market) in oracle_prices {
                let Ok(instrument_id) = self.parse_instrument_id(symbol_str) else {
                    continue;
                };
                let Some(instrument) = self.instruments.get(&instrument_id.symbol.inner()) else {
                    continue;
                };
                let Ok(oracle_price_dec) = oracle_market.oracle_price.parse::<Decimal>() else {
                    log::error!("Failed to parse oracle price: market={symbol_str}");
                    continue;
                };
                let Ok(price) =
                    Price::from_decimal_dp(oracle_price_dec, instrument.price_precision())
                else {
                    log::error!("Failed to create Price: market={symbol_str}");
                    continue;
                };

                messages.push(NautilusWsMessage::MarkPrice(MarkPriceUpdate::new(
                    instrument_id,
                    price,
                    ts_init,
                    ts_init,
                )));
                messages.push(NautilusWsMessage::IndexPrice(IndexPriceUpdate::new(
                    instrument_id,
                    price,
                    ts_init,
                    ts_init,
                )));
            }
        }

        // Parse trading data → FundingRateUpdate (and detect new instruments)
        if let Some(trading) = &contents.trading {
            for (symbol_str, trading_data) in trading {
                let Ok(instrument_id) = self.parse_instrument_id(symbol_str) else {
                    continue;
                };

                // Check if this is a new instrument not in our cache
                if !self.instruments.contains_key(&instrument_id.symbol.inner()) {
                    log::info!("New instrument discovered via WebSocket: {symbol_str}");
                    messages.push(NautilusWsMessage::NewInstrumentDiscovered {
                        ticker: symbol_str.clone(),
                    });
                    continue;
                }

                // Existing instrument - emit funding rate if available
                let Some(rate_str) = &trading_data.next_funding_rate else {
                    continue;
                };
                let Ok(rate) = rate_str.parse::<Decimal>() else {
                    log::error!(
                        "Failed to parse funding rate: market={symbol_str}, rate={rate_str}"
                    );
                    continue;
                };

                messages.push(NautilusWsMessage::FundingRate(FundingRateUpdate::new(
                    instrument_id,
                    rate,
                    None,
                    ts_init,
                    ts_init,
                )));
            }
        }

        Ok(messages)
    }

    fn parse_subaccounts(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Vec<NautilusWsMessage>> {
        log::debug!(
            "Parsing subaccounts channel data (msg_type={:?})",
            data.msg_type
        );
        let contents: DydxWsSubaccountsChannelContents =
            serde_json::from_value(data.contents.clone()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to parse subaccounts contents: {e}"))
            })?;

        let has_orders = contents.orders.as_ref().is_some_and(|o| !o.is_empty());
        let has_fills = contents.fills.as_ref().is_some_and(|f| !f.is_empty());

        if has_orders || has_fills {
            // Forward raw channel data to execution client for parsing
            // The execution client has the clob_pair_id and instrument mappings needed
            log::debug!(
                "Received {} order(s), {} fill(s) - forwarding to execution client",
                contents.orders.as_ref().map_or(0, |o| o.len()),
                contents.fills.as_ref().map_or(0, |f| f.len())
            );

            let channel_data = DydxWsSubaccountsChannelData {
                connection_id: data.connection_id.clone(),
                message_id: data.message_id,
                id: data.id.clone().unwrap_or_default(),
                version: data.version.clone().unwrap_or_default(),
                contents,
            };

            return Ok(vec![NautilusWsMessage::SubaccountsChannelData(Box::new(
                channel_data,
            ))]);
        }

        Ok(vec![])
    }

    fn parse_instrument_id(&self, symbol: &str) -> DydxWsResult<InstrumentId> {
        // dYdX WS uses raw symbols (e.g., "BTC-USD")
        // Need to append "-PERP" to match Nautilus instrument IDs
        let symbol_with_perp = format!("{symbol}-PERP");
        Ok(parse_instrument_id(&symbol_with_perp))
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> DydxWsResult<&InstrumentAny> {
        self.instruments
            .get(&instrument_id.symbol.inner())
            .ok_or_else(|| DydxWsError::Parse(format!("No instrument cached for {instrument_id}")))
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

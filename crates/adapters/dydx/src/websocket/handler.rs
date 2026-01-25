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
    fmt::Debug,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, Data, OrderBookDelta, OrderBookDeltas, TradeTick,
        bar::get_bar_interval_ns,
    },
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{AccountId, InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
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
    enums::{DydxWsChannel, DydxWsMessage, DydxWsMessageType, NautilusWsMessage},
    error::DydxWebSocketError,
    messages::{
        DydxBlockHeightChannelContents, DydxCandle, DydxMarketsContents, DydxOrderbookContents,
        DydxOrderbookSnapshotContents, DydxSubscription, DydxTradeContents,
        DydxWsBlockHeightMessage, DydxWsCandlesMessage, DydxWsChannelBatchDataMsg,
        DydxWsChannelDataMsg, DydxWsConnectedMsg, DydxWsFeedMessage, DydxWsGenericMsg,
        DydxWsMarketsMessage, DydxWsOrderbookMessage, DydxWsSubaccountsChannelContents,
        DydxWsSubaccountsChannelData, DydxWsSubaccountsMessage, DydxWsSubaccountsSubscribed,
        DydxWsSubscriptionMsg, DydxWsTradesMessage,
    },
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
    pub fn new(
        account_id: Option<AccountId>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        client: WebSocketClient,
        signal: Arc<AtomicBool>,
        subscriptions: SubscriptionState,
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
        }
    }

    /// Sends a WebSocket message with retry logic.
    ///
    /// Uses the configured [`RetryManager`] to handle transient failures.
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
    pub async fn run(&mut self) {
        log::debug!("WebSocket handler started");
        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                Some(msg) = self.raw_rx.recv() => {
                    log::trace!("Handler received raw message");
                    if let Some(nautilus_msg) = self.process_raw_message(msg).await {
                        log::trace!("Handler sending message: {:?}", std::mem::discriminant(&nautilus_msg));
                        if self.out_tx.send(nautilus_msg).is_err() {
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

    /// Processes a raw WebSocket message.
    async fn process_raw_message(&self, msg: Message) -> Option<NautilusWsMessage> {
        match msg {
            Message::Text(txt) => {
                if txt == RECONNECTED {
                    if let Err(e) = self.replay_subscriptions().await {
                        log::error!("Failed to replay subscriptions after reconnect: {e}");
                    }
                    return Some(NautilusWsMessage::Reconnected);
                }

                match serde_json::from_str::<serde_json::Value>(&txt) {
                    Ok(val) => {
                        let val_clone = val.clone();

                        // Try two-level parsing first (channel → type)
                        match serde_json::from_value::<DydxWsFeedMessage>(val.clone()) {
                            Ok(feed_msg) => {
                                return self.handle_feed_message(feed_msg).await;
                            }
                            Err(e) => {
                                // Log the raw message for debugging feed parsing failures
                                if let Some(channel) = val.get("channel") {
                                    // Only log if it has a channel field but failed to parse as feed
                                    log::debug!(
                                        "Feed message parse failed for channel {channel:?}: {e}"
                                    );
                                }
                            }
                        }

                        // Fall back to single-level parsing for non-channel messages
                        // (connected, error, subscribed/unsubscribed without channel data)
                        match serde_json::from_value::<DydxWsGenericMsg>(val.clone()) {
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
                                            log::debug!(
                                                "Parsing subaccounts subscription (fallback)"
                                            );
                                            serde_json::from_value::<DydxWsSubaccountsSubscribed>(
                                                val.clone(),
                                            )
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
                                    log::warn!(
                                        "Received unknown WebSocket message type: {}",
                                        serde_json::to_string(&val_clone)
                                            .unwrap_or_else(|_| "<invalid json>".into())
                                    );
                                    Ok(DydxWsMessage::Raw(val))
                                } else {
                                    Ok(DydxWsMessage::Raw(val))
                                };

                                match result {
                                    Ok(dydx_msg) => self.handle_dydx_message(dydx_msg).await,
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse WebSocket message: {e}. Message type: {:?}, Channel: {:?}. Raw: {}",
                                            meta.msg_type,
                                            meta.channel,
                                            serde_json::to_string(&val_clone)
                                                .unwrap_or_else(|_| "<invalid json>".into())
                                        );
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                let raw_json = serde_json::to_string_pretty(&val_clone)
                                    .unwrap_or_else(|_| format!("{val_clone:?}"));
                                log::error!(
                                    "Failed to parse WebSocket message envelope (DydxWsGenericMsg): {e}\nRaw JSON:\n{raw_json}"
                                );
                                None
                            }
                        }
                    }
                    Err(e) => {
                        let err = DydxWebSocketError::from_message(e.to_string());
                        Some(NautilusWsMessage::Error(err))
                    }
                }
            }
            Message::Pong(_data) => None,
            Message::Ping(_data) => None,  // Handled by lower layers
            Message::Binary(_bin) => None, // dYdX uses text frames
            Message::Close(_frame) => {
                log::info!("WebSocket close frame received");
                None
            }
            Message::Frame(_) => None,
        }
    }

    /// Handles a parsed dYdX WebSocket message.
    async fn handle_dydx_message(&self, msg: DydxWsMessage) -> Option<NautilusWsMessage> {
        match self.handle_message(msg).await {
            Ok(opt_msg) => opt_msg,
            Err(e) => {
                log::error!("Error handling message: {e}");
                None
            }
        }
    }

    /// Handles a two-level channel-tagged feed message.
    async fn handle_feed_message(&self, feed_msg: DydxWsFeedMessage) -> Option<NautilusWsMessage> {
        log::trace!(
            "Handling feed message: {:?}",
            std::mem::discriminant(&feed_msg)
        );
        match feed_msg {
            DydxWsFeedMessage::Subaccounts(msg) => match msg {
                DydxWsSubaccountsMessage::Subscribed(data) => {
                    self.handle_dydx_message(DydxWsMessage::SubaccountsSubscribed(data))
                        .await
                }
                DydxWsSubaccountsMessage::ChannelData(data) => {
                    // Explicitly set channel since we know it's subaccounts from outer tag
                    self.handle_dydx_message(DydxWsMessage::ChannelData(DydxWsChannelDataMsg {
                        msg_type: data.msg_type,
                        connection_id: data.connection_id,
                        message_id: data.message_id,
                        channel: DydxWsChannel::Subaccounts,
                        id: Some(data.id),
                        contents: serde_json::to_value(&data.contents)
                            .unwrap_or(serde_json::Value::Null),
                        version: Some(data.version),
                    }))
                    .await
                }
            },
            DydxWsFeedMessage::Orderbook(msg) => match msg {
                DydxWsOrderbookMessage::Subscribed(mut data) => {
                    data.channel = DydxWsChannel::Orderbook;
                    data.msg_type = DydxWsMessageType::Subscribed;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
                DydxWsOrderbookMessage::ChannelData(mut data) => {
                    data.channel = DydxWsChannel::Orderbook;
                    data.msg_type = DydxWsMessageType::ChannelData;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
                DydxWsOrderbookMessage::ChannelBatchData(mut data) => {
                    data.channel = DydxWsChannel::Orderbook;
                    data.msg_type = DydxWsMessageType::ChannelBatchData;
                    self.handle_dydx_message(DydxWsMessage::ChannelBatchData(data))
                        .await
                }
            },
            DydxWsFeedMessage::Trades(msg) => match msg {
                DydxWsTradesMessage::Subscribed(mut data)
                | DydxWsTradesMessage::ChannelData(mut data) => {
                    data.channel = DydxWsChannel::Trades;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
            },
            DydxWsFeedMessage::Markets(msg) => match msg {
                DydxWsMarketsMessage::Subscribed(mut data)
                | DydxWsMarketsMessage::ChannelData(mut data) => {
                    data.channel = DydxWsChannel::Markets;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
            },
            DydxWsFeedMessage::Candles(msg) => match msg {
                DydxWsCandlesMessage::Subscribed(mut data)
                | DydxWsCandlesMessage::ChannelData(mut data) => {
                    data.channel = DydxWsChannel::Candles;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
            },
            DydxWsFeedMessage::ParentSubaccounts(msg) => match msg {
                super::messages::DydxWsParentSubaccountsMessage::Subscribed(mut data)
                | super::messages::DydxWsParentSubaccountsMessage::ChannelData(mut data) => {
                    data.channel = DydxWsChannel::ParentSubaccounts;
                    self.handle_dydx_message(DydxWsMessage::ChannelData(data))
                        .await
                }
            },
            DydxWsFeedMessage::BlockHeight(msg) => match msg {
                DydxWsBlockHeightMessage::Subscribed(data) => {
                    // Subscribed message uses "height" field, parse directly
                    match data.contents.height.parse::<u64>() {
                        Ok(height) => Some(NautilusWsMessage::BlockHeight(height)),
                        Err(e) => {
                            log::warn!("Failed to parse block height from subscription: {e}");
                            None
                        }
                    }
                }
                DydxWsBlockHeightMessage::ChannelData(data) => {
                    // Channel data uses "blockHeight" field, parse directly
                    match data.contents.block_height.parse::<u64>() {
                        Ok(height) => Some(NautilusWsMessage::BlockHeight(height)),
                        Err(e) => {
                            log::warn!("Failed to parse block height from channel data: {e}");
                            None
                        }
                    }
                }
            },
        }
    }

    /// Handles a command to update the internal state.
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

    /// Processes a WebSocket message and converts it to Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if message parsing fails.
    #[allow(clippy::result_large_err)]
    pub async fn handle_message(
        &self,
        msg: DydxWsMessage,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        match msg {
            DydxWsMessage::Connected(_) => {
                log::info!("dYdX WebSocket connected");
                Ok(None)
            }
            DydxWsMessage::Subscribed(sub) => {
                log::debug!("Subscribed to {} (id: {:?})", sub.channel, sub.id);
                let topic = self.topic_from_msg(&sub.channel, &sub.id);
                self.subscriptions.confirm_subscribe(&topic);
                Ok(None)
            }
            DydxWsMessage::SubaccountsSubscribed(msg) => {
                log::debug!("Subaccounts subscribed with initial state");
                let topic = self.topic_from_msg(&msg.channel, &Some(msg.id.clone()));
                self.subscriptions.confirm_subscribe(&topic);
                self.parse_subaccounts_subscribed(&msg)
            }
            DydxWsMessage::Unsubscribed(unsub) => {
                log::debug!("Unsubscribed from {} (id: {:?})", unsub.channel, unsub.id);
                let topic = self.topic_from_msg(&unsub.channel, &unsub.id);
                self.subscriptions.confirm_unsubscribe(&topic);
                Ok(None)
            }
            DydxWsMessage::ChannelData(data) => self.handle_channel_data(data),
            DydxWsMessage::ChannelBatchData(data) => self.handle_channel_batch_data(data),
            DydxWsMessage::BlockHeight(height) => Ok(Some(NautilusWsMessage::BlockHeight(height))),
            DydxWsMessage::Error(err) => Ok(Some(NautilusWsMessage::Error(err))),
            DydxWsMessage::Reconnected => {
                if let Err(e) = self.replay_subscriptions().await {
                    log::error!("Failed to replay subscriptions after reconnect message: {e}");
                }
                Ok(Some(NautilusWsMessage::Reconnected))
            }
            DydxWsMessage::Pong => Ok(None),
            DydxWsMessage::Raw(_) => Ok(None),
        }
    }

    fn handle_channel_data(
        &self,
        data: DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        log::trace!(
            "Handling channel data: channel={:?}, id={:?}, msg_type={:?}",
            data.channel,
            data.id,
            data.msg_type
        );
        match data.channel {
            DydxWsChannel::Trades => self.parse_trades(&data),
            DydxWsChannel::Orderbook => {
                // Subscribed messages contain snapshot data (object format)
                // ChannelData messages contain delta updates (tuple format)
                let is_snapshot = matches!(data.msg_type, DydxWsMessageType::Subscribed);
                self.parse_orderbook(&data, is_snapshot)
            }
            DydxWsChannel::Candles => self.parse_candles(&data),
            DydxWsChannel::Markets => self.parse_markets(&data),
            DydxWsChannel::Subaccounts | DydxWsChannel::ParentSubaccounts => {
                self.parse_subaccounts(&data)
            }
            DydxWsChannel::BlockHeight => self.parse_block_height(&data),
            DydxWsChannel::Unknown => {
                log::warn!(
                    "Unknown channel data received: id={:?}, msg_type={:?}",
                    data.id,
                    data.msg_type
                );
                Ok(None)
            }
        }
    }

    fn handle_channel_batch_data(
        &self,
        data: DydxWsChannelBatchDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        match data.channel {
            DydxWsChannel::Orderbook => self.parse_orderbook_batch(&data),
            _ => {
                log::warn!(
                    "Unexpected batch data for channel: {:?}, id={:?}",
                    data.channel,
                    data.id
                );
                Ok(None)
            }
        }
    }

    fn parse_block_height(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let contents: DydxBlockHeightChannelContents =
            serde_json::from_value(data.contents.clone()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to parse block height contents: {e}"))
            })?;

        let height = contents
            .block_height
            .parse::<u64>()
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse block height: {e}")))?;

        Ok(Some(NautilusWsMessage::BlockHeight(height)))
    }

    fn parse_trades(&self, data: &DydxWsChannelDataMsg) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for trades channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let contents: DydxTradeContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade contents: {e}")))?;

        let mut ticks = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        for trade in contents.trades {
            let aggressor_side = match trade.side {
                OrderSide::Buy => AggressorSide::Buyer,
                OrderSide::Sell => AggressorSide::Seller,
                _ => continue, // Skip NoOrderSide
            };

            let price = Decimal::from_str(&trade.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade price: {e}")))?;

            let size = Decimal::from_str(&trade.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade size: {e}")))?;

            let trade_ts = trade.created_at.timestamp_nanos_opt().ok_or_else(|| {
                DydxWsError::Parse(format!("Timestamp out of range for trade {}", trade.id))
            })?;

            let tick = TradeTick::new(
                instrument_id,
                Price::from_decimal_dp(price, instrument.price_precision()).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
                })?,
                Quantity::from_decimal_dp(size, instrument.size_precision()).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
                })?,
                aggressor_side,
                TradeId::new(&trade.id),
                UnixNanos::from(trade_ts as u64),
                ts_init,
            );
            ticks.push(Data::Trade(tick));
        }

        if ticks.is_empty() {
            Ok(None)
        } else {
            Ok(Some(NautilusWsMessage::Data(ticks)))
        }
    }

    fn parse_orderbook(
        &self,
        data: &DydxWsChannelDataMsg,
        is_snapshot: bool,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();

        if is_snapshot {
            let contents: DydxOrderbookSnapshotContents =
                serde_json::from_value(data.contents.clone()).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook snapshot: {e}"))
                })?;

            let deltas = self.parse_orderbook_snapshot(
                &instrument_id,
                &contents,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;

            Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
        } else {
            let contents: DydxOrderbookContents = serde_json::from_value(data.contents.clone())
                .map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook contents: {e}"))
                })?;

            let deltas = self.parse_orderbook_deltas(
                &instrument_id,
                &contents,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;

            Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
        }
    }

    fn parse_orderbook_batch(
        &self,
        data: &DydxWsChannelBatchDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook batch channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let contents: Vec<DydxOrderbookContents> = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse orderbook batch: {e}")))?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut all_deltas = Vec::new();

        let num_messages = contents.len();
        for (idx, content) in contents.iter().enumerate() {
            let is_last_message = idx == num_messages - 1;
            let deltas = self.parse_orderbook_deltas_with_flag(
                &instrument_id,
                content,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
                is_last_message,
            )?;
            all_deltas.extend(deltas);
        }

        let deltas = OrderBookDeltas::new(instrument_id, all_deltas);
        Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
    }

    fn parse_orderbook_snapshot(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookSnapshotContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> DydxWsResult<OrderBookDeltas> {
        let mut deltas = Vec::new();
        deltas.push(OrderBookDelta::clear(*instrument_id, 0, ts_init, ts_init));

        let bids = contents.bids.as_deref().unwrap_or(&[]);
        let asks = contents.asks.as_deref().unwrap_or(&[]);

        let bids_len = bids.len();
        let asks_len = asks.len();

        for (idx, bid) in bids.iter().enumerate() {
            let is_last = idx == bids_len - 1 && asks_len == 0;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(&bid.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

            let size = Decimal::from_str(&bid.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

            let order = BookOrder::new(
                OrderSide::Buy,
                Price::from_decimal_dp(price, price_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
                })?,
                Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
                })?,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        for (idx, ask) in asks.iter().enumerate() {
            let is_last = idx == asks_len - 1;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(&ask.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

            let size = Decimal::from_str(&ask.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

            let order = BookOrder::new(
                OrderSide::Sell,
                Price::from_decimal_dp(price, price_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
                })?,
                Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
                })?,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(OrderBookDeltas::new(*instrument_id, deltas))
    }

    fn parse_orderbook_deltas(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> DydxWsResult<OrderBookDeltas> {
        let deltas = self.parse_orderbook_deltas_with_flag(
            instrument_id,
            contents,
            price_precision,
            size_precision,
            ts_init,
            true, // Mark as last message by default
        )?;
        Ok(OrderBookDeltas::new(*instrument_id, deltas))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_orderbook_deltas_with_flag(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
        is_last_message: bool,
    ) -> DydxWsResult<Vec<OrderBookDelta>> {
        let mut deltas = Vec::new();

        let bids = contents.bids.as_deref().unwrap_or(&[]);
        let asks = contents.asks.as_deref().unwrap_or(&[]);

        let bids_len = bids.len();
        let asks_len = asks.len();

        for (idx, (price_str, size_str)) in bids.iter().enumerate() {
            let is_last = is_last_message && idx == bids_len - 1 && asks_len == 0;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(price_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

            let size = Decimal::from_str(size_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

            let qty = Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
            })?;
            let action = if qty.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(
                OrderSide::Buy,
                Price::from_decimal_dp(price, price_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
                })?,
                qty,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                action,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        for (idx, (price_str, size_str)) in asks.iter().enumerate() {
            let is_last = is_last_message && idx == asks_len - 1;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(price_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

            let size = Decimal::from_str(size_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

            let qty = Quantity::from_decimal_dp(size, size_precision).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create Quantity from decimal: {e}"))
            })?;
            let action = if qty.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(
                OrderSide::Sell,
                Price::from_decimal_dp(price, price_precision).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to create Price from decimal: {e}"))
                })?,
                qty,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                action,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(deltas)
    }

    fn parse_candles(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let topic = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for candles channel".into()))?;

        let bar_type = self.bar_types.get(topic).ok_or_else(|| {
            DydxWsError::Parse(format!("No bar type registered for topic: {topic}"))
        })?;

        let candle: DydxCandle = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse candle contents: {e}")))?;

        let instrument_id = self.parse_instrument_id(&candle.ticker)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let open = Decimal::from_str(&candle.open)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse open: {e}")))?;
        let high = Decimal::from_str(&candle.high)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse high: {e}")))?;
        let low = Decimal::from_str(&candle.low)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse low: {e}")))?;
        let close = Decimal::from_str(&candle.close)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse close: {e}")))?;
        let volume = Decimal::from_str(&candle.base_token_volume)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse volume: {e}")))?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let started_at_nanos = candle.started_at.timestamp_nanos_opt().ok_or_else(|| {
            DydxWsError::Parse(format!(
                "Timestamp out of range for candle at {}",
                candle.started_at
            ))
        })?;
        let interval_nanos = get_bar_interval_ns(bar_type);
        let ts_event = UnixNanos::from(started_at_nanos as u64) + interval_nanos;

        let bar = Bar::new(
            *bar_type,
            Price::from_decimal_dp(open, instrument.price_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create open Price from decimal: {e}"))
            })?,
            Price::from_decimal_dp(high, instrument.price_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create high Price from decimal: {e}"))
            })?,
            Price::from_decimal_dp(low, instrument.price_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create low Price from decimal: {e}"))
            })?,
            Price::from_decimal_dp(close, instrument.price_precision()).map_err(|e| {
                DydxWsError::Parse(format!("Failed to create close Price from decimal: {e}"))
            })?,
            Quantity::from_decimal_dp(volume, instrument.size_precision()).map_err(|e| {
                DydxWsError::Parse(format!(
                    "Failed to create volume Quantity from decimal: {e}"
                ))
            })?,
            ts_event,
            ts_init,
        );

        Ok(Some(NautilusWsMessage::Data(vec![Data::Bar(bar)])))
    }

    fn parse_markets(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let contents: DydxMarketsContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse markets contents: {e}")))?;

        // Markets channel provides oracle price updates needed for margin calculations
        // Forward to execution client to update oracle_prices map
        if let Some(oracle_prices) = contents.oracle_prices {
            log::debug!(
                "Forwarding oracle price updates for {} markets to execution client",
                oracle_prices.len()
            );
            return Ok(Some(NautilusWsMessage::OraclePrices(oracle_prices)));
        }

        Ok(None)
    }

    fn parse_subaccounts(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
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
                msg_type: data.msg_type,
                connection_id: data.connection_id.clone(),
                message_id: data.message_id,
                id: data.id.clone().unwrap_or_default(),
                channel: data.channel,
                version: data.version.clone().unwrap_or_default(),
                contents,
            };

            return Ok(Some(NautilusWsMessage::SubaccountsChannelData(Box::new(
                channel_data,
            ))));
        }

        Ok(None)
    }

    fn parse_subaccounts_subscribed(
        &self,
        msg: &DydxWsSubaccountsSubscribed,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        // Pass raw subaccount subscription to execution client for parsing
        // The execution client has access to instruments and oracle prices needed for margin calculations
        log::debug!("Forwarding subaccount subscription to execution client");
        Ok(Some(NautilusWsMessage::SubaccountSubscribed(Box::new(
            msg.clone(),
        ))))
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

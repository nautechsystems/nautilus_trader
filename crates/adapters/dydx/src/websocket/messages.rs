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

//! WebSocket message types for dYdX public and private channels.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::enums::OrderSide;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use super::enums::{DydxWsChannel, DydxWsMessageType, DydxWsOperation};
use crate::common::enums::{
    DydxCandleResolution, DydxFillType, DydxLiquidity, DydxOrderStatus, DydxOrderType,
    DydxPositionSide, DydxPositionStatus, DydxTickerType, DydxTimeInForce, DydxTradeType,
};

/// dYdX WebSocket subscription message.
///
/// # References
///
/// <https://docs.dydx.trade/developers/indexer/websockets>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxSubscription {
    /// The operation type (subscribe/unsubscribe).
    #[serde(rename = "type")]
    pub op: DydxWsOperation,
    /// The channel to subscribe to.
    pub channel: DydxWsChannel,
    /// Optional channel-specific identifier (e.g., market symbol).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Generic subscription/unsubscription confirmation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubscriptionMsg {
    /// The message type ("subscribed" or "unsubscribed").
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    /// The connection ID.
    pub connection_id: String,
    /// The message sequence number.
    pub message_id: u64,
    /// The channel name.
    pub channel: DydxWsChannel,
    /// Optional channel-specific identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Connection established message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsConnectedMsg {
    /// The message type ("connected").
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    /// The connection ID assigned by the server.
    pub connection_id: String,
    /// The message sequence number.
    pub message_id: u64,
}

/// Single channel data update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsChannelDataMsg {
    /// The message type (may be absent for channel updates).
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    /// The connection ID.
    pub connection_id: String,
    /// The message sequence number.
    pub message_id: u64,
    /// The channel name (optional since serde tag parsing may consume it).
    #[serde(default)]
    pub channel: DydxWsChannel,
    /// Optional channel-specific identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The payload data (format depends on channel).
    pub contents: Value,
    /// API version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Batch channel data update message (multiple updates in one message).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsChannelBatchDataMsg {
    /// The message type (may be absent for batch channel updates).
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    /// The connection ID.
    pub connection_id: String,
    /// The message sequence number.
    pub message_id: u64,
    /// The channel name (optional since serde tag parsing may consume it).
    #[serde(default)]
    pub channel: DydxWsChannel,
    /// Optional channel-specific identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Array of payload data.
    pub contents: Value,
    /// API version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// General WebSocket message structure for routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsMessageGeneral {
    #[serde(rename = "type")]
    pub msg_type: Option<DydxWsMessageType>,
    pub connection_id: Option<String>,
    pub message_id: Option<u64>,
    pub channel: Option<DydxWsChannel>,
    pub id: Option<String>,
    pub message: Option<String>,
}

/// Two-level WebSocket message envelope matching dYdX protocol.
///
/// First level: Routes by channel field (v4_subaccounts, v4_orderbook, etc.)
/// Second level: Each channel variant contains type-tagged messages
///
/// # References
///
/// <https://github.com/dydxprotocol/v4-clients/blob/main/v4-client-rs/client/src/indexer/sock/messages.rs#L253>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel")]
pub enum DydxWsFeedMessage {
    /// Subaccount updates (orders, fills, positions).
    #[serde(rename = "v4_subaccounts")]
    Subaccounts(DydxWsSubaccountsMessage),
    /// Order book snapshots and updates.
    #[serde(rename = "v4_orderbook")]
    Orderbook(DydxWsOrderbookMessage),
    /// Trade stream for specific market.
    #[serde(rename = "v4_trades")]
    Trades(DydxWsTradesMessage),
    /// Market data for all markets.
    #[serde(rename = "v4_markets")]
    Markets(DydxWsMarketsMessage),
    /// Candlestick/kline data.
    #[serde(rename = "v4_candles")]
    Candles(DydxWsCandlesMessage),
    /// Parent subaccount updates (for isolated positions).
    #[serde(rename = "v4_parent_subaccounts")]
    ParentSubaccounts(DydxWsParentSubaccountsMessage),
    /// Block height updates from chain.
    #[serde(rename = "v4_block_height")]
    BlockHeight(DydxWsBlockHeightMessage),
}

/// Subaccounts channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsSubaccountsMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsSubaccountsSubscribed),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsSubaccountsChannelData),
}

/// Orderbook channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsOrderbookMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsChannelDataMsg),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsChannelDataMsg),
    /// Batch channel data.
    #[serde(rename = "channel_batch_data")]
    ChannelBatchData(DydxWsChannelBatchDataMsg),
}

/// Trades channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsTradesMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsChannelDataMsg),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsChannelDataMsg),
}

/// Markets channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsMarketsMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsChannelDataMsg),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsChannelDataMsg),
}

/// Candles channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsCandlesMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsChannelDataMsg),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsChannelDataMsg),
}

/// Parent subaccounts channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsParentSubaccountsMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsChannelDataMsg),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsChannelDataMsg),
}

/// Block height channel messages (second level, type-tagged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DydxWsBlockHeightMessage {
    /// Initial subscription confirmation.
    #[serde(rename = "subscribed")]
    Subscribed(DydxWsBlockHeightSubscribedData),
    /// Channel data update.
    #[serde(rename = "channel_data")]
    ChannelData(DydxWsBlockHeightChannelData),
}

/// Generic message structure for initial classification (fallback for non-channel messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsGenericMsg {
    /// The message type.
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    /// Optional connection ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    /// Optional message sequence number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<u64>,
    /// Optional channel name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<DydxWsChannel>,
    /// Optional channel-specific identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Optional error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl DydxWsGenericMsg {
    /// Returns `true` if this message is an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.msg_type == DydxWsMessageType::Error
    }

    /// Returns `true` if this message is a subscription confirmation.
    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.msg_type == DydxWsMessageType::Subscribed
    }

    /// Returns `true` if this message is an unsubscription confirmation.
    #[must_use]
    pub fn is_unsubscribed(&self) -> bool {
        self.msg_type == DydxWsMessageType::Unsubscribed
    }

    /// Returns `true` if this message is a connection notification.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.msg_type == DydxWsMessageType::Connected
    }

    /// Returns `true` if this message is channel data.
    #[must_use]
    pub fn is_channel_data(&self) -> bool {
        self.msg_type == DydxWsMessageType::ChannelData
    }

    /// Returns `true` if this message is batch channel data.
    #[must_use]
    pub fn is_channel_batch_data(&self) -> bool {
        self.msg_type == DydxWsMessageType::ChannelBatchData
    }

    /// Returns `true` if this message is an unknown/unrecognized type.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.msg_type == DydxWsMessageType::Unknown
    }
}

/// Block height subscription confirmed contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxBlockHeightSubscribedContents {
    pub height: String,
    pub time: DateTime<Utc>,
}

/// Block height subscription confirmed message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsBlockHeightSubscribedData {
    /// The message type (may be absent due to serde tag parsing).
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    /// The channel name (may be absent due to serde tag parsing).
    #[serde(default)]
    pub channel: DydxWsChannel,
    pub id: String,
    pub contents: DydxBlockHeightSubscribedContents,
}

/// Block height channel data contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxBlockHeightChannelContents {
    #[serde(rename = "blockHeight")]
    pub block_height: String,
    pub time: DateTime<Utc>,
}

/// Block height channel data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsBlockHeightChannelData {
    /// The message type (may be absent due to serde tag parsing).
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub id: String,
    /// The channel name (may be absent due to serde tag parsing).
    #[serde(default)]
    pub channel: DydxWsChannel,
    pub version: String,
    pub contents: DydxBlockHeightChannelContents,
}

/// Oracle price data for a market (full format from subscribed message).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOraclePriceMarketFull {
    #[serde(rename = "oraclePrice")]
    pub oracle_price: String,
    #[serde(rename = "effectiveAt")]
    pub effective_at: String,
    #[serde(rename = "effectiveAtHeight")]
    pub effective_at_height: String,
    #[serde(rename = "marketId")]
    pub market_id: u32,
}

/// Oracle price data for a market (simple format from channel_data).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxOraclePriceMarket {
    /// Oracle price.
    pub oracle_price: String,
}

/// Market message contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxMarketMessageContents {
    #[serde(rename = "oraclePrices")]
    pub oracle_prices: Option<HashMap<String, DydxOraclePriceMarketFull>>,
    pub trading: Option<Value>,
}

/// Markets channel data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsMarketChannelData {
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub channel: DydxWsChannel,
    pub contents: DydxMarketMessageContents,
    pub version: String,
    pub message_id: u64,
    pub connection_id: Option<String>,
    pub id: Option<String>,
}

/// Markets subscription confirmed message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsMarketSubscribed {
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub channel: DydxWsChannel,
    pub contents: Value,
}

/// Contents of v4_markets channel_data message (simple format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxMarketsContents {
    /// Oracle prices by market symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oracle_prices: Option<HashMap<String, DydxOraclePriceMarket>>,
}

/// Trade message from v4_trades channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTrade {
    /// Trade ID.
    pub id: String,
    /// Order side (BUY/SELL).
    pub side: OrderSide,
    /// Trade size.
    pub size: String,
    /// Trade price.
    pub price: String,
    /// Trade timestamp.
    pub created_at: DateTime<Utc>,
    /// Trade type.
    #[serde(rename = "type")]
    pub trade_type: DydxTradeType,
    /// Block height (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_height: Option<String>,
}

/// Contents of v4_trades channel_data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxTradeContents {
    /// Array of trades.
    pub trades: Vec<DydxTrade>,
}

/// Candle/bar data from v4_candles channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxCandle {
    /// Base token volume.
    pub base_token_volume: String,
    /// Close price.
    pub close: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Open price.
    pub open: String,
    /// Resolution/timeframe.
    pub resolution: DydxCandleResolution,
    /// Start time.
    pub started_at: DateTime<Utc>,
    /// Starting open interest.
    pub starting_open_interest: String,
    /// Market ticker.
    pub ticker: String,
    /// Number of trades.
    pub trades: i64,
    /// USD volume.
    pub usd_volume: String,
    /// Orderbook mid price at close (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orderbook_mid_price_close: Option<String>,
    /// Orderbook mid price at open (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orderbook_mid_price_open: Option<String>,
}

/// Order book price level (price, size tuple).
pub type PriceLevel = (String, String);

/// Contents of v4_orderbook channel_data/channel_batch_data messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOrderbookContents {
    /// Bid price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<PriceLevel>>,
    /// Ask price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<PriceLevel>>,
}

/// Price level for orderbook snapshot (structured format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxPriceLevel {
    /// Price.
    pub price: String,
    /// Size.
    pub size: String,
}

/// Contents of v4_orderbook subscribed (snapshot) message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOrderbookSnapshotContents {
    /// Bid price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<DydxPriceLevel>>,
    /// Ask price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<DydxPriceLevel>>,
}

/// Subaccount balance update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAssetBalance {
    pub symbol: Ustr,
    pub side: DydxPositionSide,
    pub size: String,
    #[serde(rename = "assetId")]
    pub asset_id: String,
}

/// Subaccount perpetual position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxPerpetualPosition {
    pub market: Ustr,
    pub status: DydxPositionStatus,
    pub side: DydxPositionSide,
    pub size: String,
    #[serde(rename = "maxSize")]
    pub max_size: String,
    #[serde(rename = "entryPrice")]
    pub entry_price: String,
    #[serde(rename = "exitPrice")]
    pub exit_price: Option<String>,
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: String,
    #[serde(rename = "unrealizedPnl")]
    pub unrealized_pnl: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "closedAt")]
    pub closed_at: Option<String>,
    #[serde(rename = "sumOpen")]
    pub sum_open: String,
    #[serde(rename = "sumClose")]
    pub sum_close: String,
    #[serde(rename = "netFunding")]
    pub net_funding: String,
}

/// Subaccount information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxSubaccountInfo {
    pub address: String,
    #[serde(rename = "subaccountNumber")]
    pub subaccount_number: u32,
    pub equity: String,
    #[serde(rename = "freeCollateral")]
    pub free_collateral: String,
    #[serde(rename = "openPerpetualPositions")]
    pub open_perpetual_positions: Option<HashMap<String, DydxPerpetualPosition>>,
    #[serde(rename = "assetPositions")]
    pub asset_positions: Option<HashMap<String, DydxAssetBalance>>,
    #[serde(rename = "marginEnabled")]
    pub margin_enabled: bool,
    #[serde(rename = "updatedAtHeight")]
    pub updated_at_height: String,
    #[serde(rename = "latestProcessedBlockHeight")]
    pub latest_processed_block_height: String,
}

/// Order message from WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsOrderSubaccountMessageContents {
    pub id: String,
    #[serde(rename = "subaccountId")]
    pub subaccount_id: String,
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "clobPairId")]
    pub clob_pair_id: String,
    pub side: OrderSide,
    pub size: String,
    pub price: String,
    pub status: DydxOrderStatus,
    #[serde(rename = "type")]
    pub order_type: DydxOrderType,
    #[serde(rename = "timeInForce")]
    pub time_in_force: DydxTimeInForce,
    #[serde(rename = "postOnly")]
    pub post_only: bool,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    #[serde(rename = "orderFlags")]
    pub order_flags: String,
    #[serde(rename = "goodTilBlock")]
    pub good_til_block: Option<String>,
    #[serde(rename = "goodTilBlockTime")]
    pub good_til_block_time: Option<String>,
    #[serde(rename = "createdAtHeight")]
    pub created_at_height: Option<String>,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: Option<String>,
    #[serde(rename = "triggerPrice")]
    pub trigger_price: Option<String>,
    #[serde(rename = "totalFilled")]
    pub total_filled: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "updatedAtHeight")]
    pub updated_at_height: Option<String>,
}

/// Fill message from WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsFillSubaccountMessageContents {
    pub id: String,
    #[serde(rename = "subaccountId")]
    pub subaccount_id: String,
    pub side: OrderSide,
    pub liquidity: DydxLiquidity,
    #[serde(rename = "type")]
    pub fill_type: DydxFillType,
    pub market: Ustr,
    #[serde(rename = "marketType")]
    pub market_type: DydxTickerType,
    pub price: String,
    pub size: String,
    pub fee: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "createdAtHeight")]
    pub created_at_height: Option<String>,
    #[serde(rename = "orderId")]
    pub order_id: Option<String>,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: Option<String>,
}

/// Subaccount subscription contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsSubscribedContents {
    pub subaccount: DydxSubaccountInfo,
}

/// Subaccounts subscription confirmed message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsSubscribed {
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    #[serde(default)]
    pub channel: DydxWsChannel,
    pub id: String,
    pub contents: DydxWsSubaccountsSubscribedContents,
}

/// Subaccounts channel data contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsChannelContents {
    pub orders: Option<Vec<DydxWsOrderSubaccountMessageContents>>,
    pub fills: Option<Vec<DydxWsFillSubaccountMessageContents>>,
}

/// Subaccounts channel data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsChannelData {
    #[serde(rename = "type", default)]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub id: String,
    #[serde(default)]
    pub channel: DydxWsChannel,
    pub version: String,
    pub contents: DydxWsSubaccountsChannelContents,
}

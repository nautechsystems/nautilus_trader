// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! WebSocket message schemas for dYdX v4.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::enums::{OrderSide, PositionSide};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{AsRefStr, Display, EnumString, FromRepr};
use ustr::Ustr;

use crate::{
    common::enums::{
        DydxFillType, DydxLiquidity, DydxOrderStatus, DydxOrderType, DydxPositionStatus,
        DydxTickerType, DydxTimeInForce,
    },
    websocket::enums::DydxWsChannel,
};

/// WebSocket message types for dYdX.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsMessageType {
    /// Connection established.
    Connected,
    /// Subscription confirmed.
    Subscribed,
    /// Unsubscription confirmed.
    Unsubscribed,
    /// Channel data update.
    ChannelData,
    /// Batch channel data update.
    ChannelBatchData,
    /// Error message.
    Error,
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

/// Block height subscription confirmed contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxBlockHeightSubscribedContents {
    pub height: String,
    pub time: DateTime<Utc>,
}

/// Block height subscription confirmed message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsBlockHeightSubscribedData {
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
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
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub id: String,
    pub channel: DydxWsChannel,
    pub version: String,
    pub contents: DydxBlockHeightChannelContents,
}

/// Oracle price data for a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOraclePriceMarket {
    #[serde(rename = "oraclePrice")]
    pub oracle_price: String,
    #[serde(rename = "effectiveAt")]
    pub effective_at: String,
    #[serde(rename = "effectiveAtHeight")]
    pub effective_at_height: String,
    #[serde(rename = "marketId")]
    pub market_id: u32,
}

/// Market message contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxMarketMessageContents {
    #[serde(rename = "oraclePrices")]
    pub oracle_prices: Option<HashMap<String, DydxOraclePriceMarket>>,
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
pub struct DydxWsMarketSubscribedData {
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub channel: DydxWsChannel,
    pub contents: Value, // Full market data structure
}

/// Subaccount balance update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAssetBalance {
    pub symbol: Ustr,
    pub side: OrderSide,
    pub size: String,
    #[serde(rename = "assetId")]
    pub asset_id: String,
}

/// Subaccount perpetual position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxPerpetualPosition {
    pub market: Ustr,
    pub status: DydxPositionStatus,
    pub side: PositionSide,
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
    pub created_at_height: String,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: String,
    #[serde(rename = "triggerPrice")]
    pub trigger_price: Option<String>,
    #[serde(rename = "totalFilled")]
    pub total_filled: String,
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
    pub created_at_height: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: String,
}

/// Subaccount subscription contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsSubscribedContents {
    pub subaccount: DydxSubaccountInfo,
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

/// Subaccounts subscription confirmed message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxWsSubaccountsSubscribed {
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
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
    #[serde(rename = "type")]
    pub msg_type: DydxWsMessageType,
    pub connection_id: String,
    pub message_id: u64,
    pub id: String,
    pub channel: DydxWsChannel,
    pub version: String,
    pub contents: DydxWsSubaccountsChannelContents,
}

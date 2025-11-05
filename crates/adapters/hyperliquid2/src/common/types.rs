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

//! Hyperliquid data type definitions.

use serde::{Deserialize, Serialize};

/// Hyperliquid asset info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidAsset {
    /// Asset name (e.g., "BTC")
    pub name: String,
    /// Size decimals
    pub sz_decimals: u8,
    /// Max leverage
    #[serde(default)]
    pub max_leverage: Option<u32>,
    /// Only isolated margin
    #[serde(default)]
    pub only_isolated: Option<bool>,
}

/// Hyperliquid universe info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUniverse {
    /// List of assets
    pub universe: Vec<HyperliquidAsset>,
}

/// Hyperliquid meta info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidMetaInfo {
    /// Universe data
    pub universe: Vec<HyperliquidAsset>,
}

/// Hyperliquid L2 book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidL2Book {
    /// Asset name
    pub coin: String,
    /// Timestamp
    pub time: u64,
    /// Bid levels [price, size]
    pub levels: Vec<Vec<HyperliquidLevel>>,
}

/// Order book level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidLevel {
    /// Price level
    pub px: String,
    /// Size at level
    pub sz: String,
    /// Number of orders
    pub n: u32,
}

/// Hyperliquid trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidTrade {
    /// Asset name
    pub coin: String,
    /// Side ("A" for buy, "B" for sell)
    pub side: String,
    /// Price
    pub px: String,
    /// Size
    pub sz: String,
    /// Timestamp
    pub time: u64,
    /// Trade hash
    #[serde(default)]
    pub hash: Option<String>,
}

/// Hyperliquid candle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidCandle {
    /// Timestamp
    #[serde(rename = "t")]
    pub time: u64,
    /// Open price
    #[serde(rename = "o")]
    pub open: String,
    /// High price
    #[serde(rename = "h")]
    pub high: String,
    /// Low price
    #[serde(rename = "l")]
    pub low: String,
    /// Close price
    #[serde(rename = "c")]
    pub close: String,
    /// Volume
    #[serde(rename = "v")]
    pub volume: String,
}

/// Hyperliquid order request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOrderRequest {
    /// Asset index
    pub asset: u32,
    /// Is buy order
    pub is_buy: bool,
    /// Limit price
    pub limit_px: String,
    /// Order size
    pub sz: String,
    /// Reduce only
    pub reduce_only: bool,
    /// Order type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<String>,
    /// Client order ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
}

/// Hyperliquid order response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOrderResponse {
    /// Status
    pub status: String,
    /// Response data
    #[serde(default)]
    pub response: Option<HyperliquidOrderResponseData>,
}

/// Hyperliquid order response data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOrderResponseData {
    /// Order type
    #[serde(rename = "type")]
    pub order_type: String,
    /// Status data
    pub data: Option<serde_json::Value>,
}

/// Hyperliquid user state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidUserState {
    /// Asset positions
    pub asset_positions: Vec<HyperliquidPosition>,
    /// Cross margin summary
    #[serde(default)]
    pub cross_margin_summary: Option<HyperliquidMarginSummary>,
    /// Margin summary
    #[serde(default)]
    pub margin_summary: Option<HyperliquidMarginSummary>,
}

/// Hyperliquid position
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidPosition {
    /// Position data
    pub position: HyperliquidPositionData,
}

/// Hyperliquid position data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidPositionData {
    /// Asset name
    pub coin: String,
    /// Size (positive for long, negative for short)
    pub szi: String,
    /// Leverage
    pub leverage: HyperliquidLeverage,
    /// Entry price
    #[serde(default)]
    pub entry_px: Option<String>,
    /// Position value
    #[serde(default)]
    pub position_value: Option<String>,
    /// Unrealized PnL
    #[serde(default)]
    pub unrealized_pnl: Option<String>,
}

/// Hyperliquid leverage info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidLeverage {
    /// Leverage type
    #[serde(rename = "type")]
    pub leverage_type: String,
    /// Leverage value
    pub value: u32,
}

/// Hyperliquid margin summary
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidMarginSummary {
    /// Account value
    pub account_value: String,
    /// Total notional position value
    pub total_ntl_pos: String,
    /// Total raw usd
    pub total_raw_usd: String,
}

/// Hyperliquid open order
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOpenOrder {
    /// Asset name
    pub coin: String,
    /// Side
    pub side: String,
    /// Limit price
    pub limit_px: String,
    /// Size
    pub sz: String,
    /// Order ID
    pub oid: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Client order ID
    #[serde(default)]
    pub cloid: Option<String>,
}

/// Hyperliquid user fill
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidUserFill {
    /// Asset name
    pub coin: String,
    /// Price
    pub px: String,
    /// Size
    pub sz: String,
    /// Side
    pub side: String,
    /// Timestamp
    pub time: u64,
    /// Order ID
    pub oid: u64,
    /// Client order ID
    #[serde(default)]
    pub cloid: Option<String>,
    /// Fee paid
    pub fee: String,
    /// Fee token
    #[serde(default)]
    pub fee_token: Option<String>,
}

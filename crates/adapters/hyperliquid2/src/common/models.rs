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

//! Hyperliquid data models and structures.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::enums::{
    HyperliquidOrderSide, HyperliquidOrderType, HyperliquidTimeInForce,
};

/// Hyperliquid asset information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidAsset {
    pub name: String,
    pub sz_decimals: i32,
    pub max_leverage: Option<i32>,
    pub only_isolated: Option<bool>,
}

/// Hyperliquid universe information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidUniverse {
    pub universe: Vec<HyperliquidAsset>,
}

/// Hyperliquid market mid prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidAllMids {
    pub mids: HashMap<String, String>,
}

/// Hyperliquid L2 orderbook level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidLevel {
    pub px: String,  // Price
    pub sz: String,  // Size
    pub n: i32,      // Number of orders
}

/// Hyperliquid L2 orderbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidL2Book {
    pub coin: String,
    pub levels: Vec<Vec<HyperliquidLevel>>, // [bids, asks]
    pub time: u64,
}

/// Hyperliquid trade information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidTrade {
    pub coin: String,
    pub side: HyperliquidOrderSide,
    pub px: String,    // Price
    pub sz: String,    // Size
    pub time: u64,
    pub hash: String,
}

/// Hyperliquid candle data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidCandle {
    #[serde(rename = "t")]
    pub time: u64,
    #[serde(rename = "T")]
    pub close_time: u64,
    #[serde(rename = "s")]
    pub coin: String,
    #[serde(rename = "i")]
    pub interval: String,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "v")]
    pub volume: String,
    #[serde(rename = "n")]
    pub trades: i64,
}

/// Hyperliquid user state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidUserState {
    pub asset_positions: Vec<HyperliquidAssetPosition>,
    pub cross_margin_summary: HyperliquidMarginSummary,
    pub cross_maintenance_margin_used: String,
    pub withdrawable: String,
    pub time: u64,
}

/// Hyperliquid asset position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidAssetPosition {
    pub position: HyperliquidPosition,
    pub type_: String, // "oneWay"
    pub margin_used: String,
    pub unrealized_pnl: String,
    pub return_on_equity: String,
}

/// Hyperliquid position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]  
pub struct HyperliquidPosition {
    pub coin: String,
    pub entry_px: Option<String>,
    pub leverage: HyperliquidLeverage,
    pub liquidation_px: Option<String>,
    pub margin_used: String,
    pub position_value: String,
    pub return_on_equity: String,
    pub szi: String, // Position size
    pub unrealized_pnl: String,
}

/// Hyperliquid leverage information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidLeverage {
    pub type_: String,
    pub value: i32,
}

/// Hyperliquid margin summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidMarginSummary {
    pub account_value: String,
    pub total_margin_used: String,
    pub total_ntl_pos: String,
    pub total_raw_usd: String,
}

/// Hyperliquid order information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOrder {
    pub coin: String,
    pub limit_px: String,
    pub oid: u64, // Order ID
    pub side: HyperliquidOrderSide,
    pub sz: String,
    pub timestamp: u64,
    pub tif: HyperliquidTimeInForce,
    pub order_type: HyperliquidOrderType,
    pub reduce_only: bool,
    pub is_trigger: bool,
    pub trigger_condition: String,
    pub trigger_px: String,
}

/// Hyperliquid fill information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidFill {
    pub coin: String,
    pub px: String,    // Fill price
    pub sz: String,    // Fill size
    pub side: HyperliquidOrderSide,
    pub time: u64,
    pub start_position: String,
    pub dir: String,
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,      // Order ID
    pub crossed: bool,
    pub fee: String,
    pub liquidation: Option<bool>,
}

/// Hyperliquid WebSocket subscription message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidWsSubscription {
    pub method: String, // "subscribe" or "unsubscribe"  
    pub subscription: HyperliquidWsSubscriptionData,
}

/// Hyperliquid WebSocket subscription data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]  
pub struct HyperliquidWsSubscriptionData {
    #[serde(rename = "type")]
    pub type_: String,
    pub coin: Option<String>,
    pub user: Option<String>,
}

/// Generic Hyperliquid API response wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidApiResponse<T> {
    pub data: Option<T>,
    pub status: String,
    pub error: Option<String>,
}

// Trading request models

/// Order request for placing new orders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderRequest {
    pub asset: String,
    pub is_buy: bool,
    pub limit_px: String,
    pub sz: String,
    pub reduce_only: bool,
    pub order_type: HyperliquidOrderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<HyperliquidTimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
}

/// Cancel order request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidCancelOrderRequest {
    pub asset: String,
    pub oid: u64,
}

/// Cancel all orders request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidCancelAllOrdersRequest {
    pub asset: String,
}

/// Modify order request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidModifyOrderRequest {
    pub oid: u64,
    pub order: HyperliquidOrderRequest,
}

/// Update leverage request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUpdateLeverageRequest {
    pub asset: String,
    pub is_cross: bool,
    pub leverage: i32,
}

/// Update isolated margin request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUpdateIsolatedMarginRequest {
    pub asset: String,
    pub is_cross: bool,
    pub ntli: String,
}

/// USDC transfer request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUsdcTransferRequest {
    pub destination: String,
    pub amount: String,
    pub time: u64,
}

// Info endpoint request/response models

/// Portfolio request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidPortfolioRequest {
    #[serde(rename = "type")]
    pub type_: String, // "portfolio"
    pub user: String,
}

/// User fills request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUserFillsRequest {
    #[serde(rename = "type")]
    pub type_: String, // "userFills"
    pub user: String,
}

/// User fills by time request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUserFillsByTimeRequest {
    #[serde(rename = "type")]
    pub type_: String, // "userFillsByTime"
    pub user: String,
    pub start_time: u64,
    pub end_time: u64,
}

/// Open orders request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOpenOrdersRequest {
    #[serde(rename = "type")]
    pub type_: String, // "openOrders"
    pub user: String,
}

/// Historical orders request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidHistoricalOrdersRequest {
    #[serde(rename = "type")]
    pub type_: String, // "historicalOrders"
    pub user: String,
}

/// User state request (clearinghouse state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidUserStateRequest {
    #[serde(rename = "type")]
    pub type_: String, // "clearinghouseState"
    pub user: String,
}

/// Portfolio data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidPortfolioDataPoint {
    pub time: u64,
    pub value: String,
}

/// Portfolio response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidPortfolio {
    pub account_value: Vec<HyperliquidPortfolioDataPoint>,
    pub pnl_history: Vec<HyperliquidPortfolioDataPoint>,
    pub volume_history: Vec<HyperliquidPortfolioDataPoint>,
}

/// User fills response (simplified).
pub type HyperliquidUserFills = Vec<HyperliquidFill>;

/// Open orders response.
pub type HyperliquidOpenOrders = Vec<HyperliquidOrder>;

/// Historical orders response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidHistoricalOrder {
    pub order: HyperliquidOrder,
    pub status: String,
    pub status_timestamp: u64,
    pub fills: Vec<HyperliquidFill>,
}

pub type HyperliquidHistoricalOrders = Vec<HyperliquidHistoricalOrder>;

/// Error response from API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidApiError {
    pub error: String,
    pub msg: Option<String>,
}

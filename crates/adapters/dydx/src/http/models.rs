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

//! Data models for dYdX v4 Indexer REST API responses.
//!
//! This module contains Rust types that mirror the JSON structures returned
//! by the dYdX v4 Indexer API endpoints.
//!
//! # API Documentation
//!
//! - Indexer HTTP API: <https://docs.dydx.exchange/api_integration-indexer/indexer_api>
//! - Markets: <https://docs.dydx.exchange/api_integration-indexer/indexer_api#markets>
//! - Accounts: <https://docs.dydx.exchange/api_integration-indexer/indexer_api#accounts>

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::enums::OrderSide;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Deserializes an empty string as None, otherwise as Some(String).
fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    Ok(s.filter(|s| !s.is_empty()))
}
use serde_with::{DisplayFromStr, serde_as};
use ustr::Ustr;

use crate::common::enums::{
    DydxCandleResolution, DydxConditionType, DydxFillType, DydxLiquidity, DydxMarketStatus,
    DydxOrderExecution, DydxOrderStatus, DydxPositionStatus, DydxTickerType, DydxTimeInForce,
    DydxTradeType,
};

////////////////////////////////////////////////////////////////////////////////
// Markets
////////////////////////////////////////////////////////////////////////////////

/// Response wrapper for markets endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketsResponse {
    /// Map of market ticker to perpetual market data.
    pub markets: HashMap<String, PerpetualMarket>,
}

/// Perpetual market definition.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpetualMarket {
    /// Unique identifier for the CLOB pair.
    #[serde_as(as = "DisplayFromStr")]
    pub clob_pair_id: u32,
    /// Market ticker (e.g., "BTC-USD").
    pub ticker: String,
    /// Market status (ACTIVE, PAUSED, etc.).
    pub status: DydxMarketStatus,
    /// Base asset symbol (optional, not always returned by API).
    #[serde(default)]
    pub base_asset: Option<String>,
    /// Quote asset symbol (optional, not always returned by API).
    #[serde(default)]
    pub quote_asset: Option<String>,
    /// Step size for order quantities (minimum increment).
    #[serde_as(as = "DisplayFromStr")]
    pub step_size: Decimal,
    /// Tick size for order prices (minimum increment).
    #[serde_as(as = "DisplayFromStr")]
    pub tick_size: Decimal,
    /// Index price for the market (optional, not always returned by API).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub index_price: Option<Decimal>,
    /// Oracle price for the market.
    #[serde_as(as = "DisplayFromStr")]
    pub oracle_price: Decimal,
    /// Price change over 24 hours.
    #[serde(rename = "priceChange24H")]
    #[serde_as(as = "DisplayFromStr")]
    pub price_change_24h: Decimal,
    /// Next funding rate.
    #[serde_as(as = "DisplayFromStr")]
    pub next_funding_rate: Decimal,
    /// Next funding time (ISO8601, optional).
    #[serde(default)]
    pub next_funding_at: Option<DateTime<Utc>>,
    /// Minimum order size in base currency (optional).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub min_order_size: Option<Decimal>,
    /// Market type (always PERPETUAL for dYdX v4, optional).
    #[serde(rename = "type", default)]
    pub market_type: Option<DydxTickerType>,
    /// Initial margin fraction.
    #[serde_as(as = "DisplayFromStr")]
    pub initial_margin_fraction: Decimal,
    /// Maintenance margin fraction.
    #[serde_as(as = "DisplayFromStr")]
    pub maintenance_margin_fraction: Decimal,
    /// Base position notional value (optional).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub base_position_notional: Option<Decimal>,
    /// Incremental position size for margin scaling (optional).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub incremental_position_size: Option<Decimal>,
    /// Incremental initial margin fraction (optional).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub incremental_initial_margin_fraction: Option<Decimal>,
    /// Maximum position size (optional).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub max_position_size: Option<Decimal>,
    /// Open interest in base currency.
    #[serde_as(as = "DisplayFromStr")]
    pub open_interest: Decimal,
    /// Atomic resolution (power of 10 for quantum conversion).
    pub atomic_resolution: i32,
    /// Quantum conversion exponent (deprecated, use atomic_resolution).
    pub quantum_conversion_exponent: i32,
    /// Subticks per tick.
    pub subticks_per_tick: u32,
    /// Step base quantums.
    pub step_base_quantums: u64,
    /// Is the market in reduce-only mode.
    #[serde(default)]
    pub is_reduce_only: bool,
}

/// Orderbook snapshot response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookResponse {
    /// Bids (buy orders).
    pub bids: Vec<OrderbookLevel>,
    /// Asks (sell orders).
    pub asks: Vec<OrderbookLevel>,
}

/// Single level in the orderbook.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookLevel {
    /// Price level.
    #[serde_as(as = "DisplayFromStr")]
    pub price: Decimal,
    /// Size at this level.
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
}

/// Response wrapper for trades endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradesResponse {
    /// List of trades.
    pub trades: Vec<Trade>,
}

/// Individual trade.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    /// Unique trade ID.
    pub id: String,
    /// Order side that was the taker.
    pub side: OrderSide,
    /// Trade size in base currency.
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
    /// Trade price.
    #[serde_as(as = "DisplayFromStr")]
    pub price: Decimal,
    /// Trade timestamp.
    pub created_at: DateTime<Utc>,
    /// Height of block containing this trade.
    #[serde_as(as = "DisplayFromStr")]
    pub created_at_height: u64,
    /// Trade type (LIMIT, MARKET, LIQUIDATED, etc.).
    #[serde(rename = "type")]
    pub trade_type: DydxTradeType,
}

/// Response wrapper for candles endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandlesResponse {
    /// List of candles.
    pub candles: Vec<Candle>,
}

/// OHLCV candle data.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candle {
    /// Candle start time.
    pub started_at: DateTime<Utc>,
    /// Market ticker.
    pub ticker: String,
    /// Candle resolution.
    pub resolution: DydxCandleResolution,
    /// Opening price.
    #[serde_as(as = "DisplayFromStr")]
    pub open: Decimal,
    /// Highest price.
    #[serde_as(as = "DisplayFromStr")]
    pub high: Decimal,
    /// Lowest price.
    #[serde_as(as = "DisplayFromStr")]
    pub low: Decimal,
    /// Closing price.
    #[serde_as(as = "DisplayFromStr")]
    pub close: Decimal,
    /// Base asset volume.
    #[serde_as(as = "DisplayFromStr")]
    pub base_token_volume: Decimal,
    /// Quote asset volume (USD).
    #[serde_as(as = "DisplayFromStr")]
    pub usd_volume: Decimal,
    /// Number of trades in this candle.
    pub trades: u64,
    /// Block height at candle start.
    #[serde_as(as = "DisplayFromStr")]
    pub starting_open_interest: Decimal,
}

////////////////////////////////////////////////////////////////////////////////
// Accounts
////////////////////////////////////////////////////////////////////////////////

/// Response for subaccount endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubaccountResponse {
    /// Subaccount data.
    pub subaccount: Subaccount,
}

/// Subaccount information.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subaccount {
    /// Subaccount address (dydx...).
    pub address: String,
    /// Subaccount number.
    pub subaccount_number: u32,
    /// Account equity in USD.
    #[serde_as(as = "DisplayFromStr")]
    pub equity: Decimal,
    /// Free collateral.
    #[serde_as(as = "DisplayFromStr")]
    pub free_collateral: Decimal,
    /// Open perpetual positions.
    #[serde(default)]
    pub open_perpetual_positions: HashMap<String, PerpetualPosition>,
    /// Asset positions (e.g., USDC).
    #[serde(default)]
    pub asset_positions: HashMap<String, AssetPosition>,
    /// Margin enabled flag.
    #[serde(default)]
    pub margin_enabled: bool,
    /// Last updated height.
    #[serde_as(as = "DisplayFromStr")]
    pub updated_at_height: u64,
    /// Latest processed block height (present in API response).
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub latest_processed_block_height: Option<u64>,
}

/// Perpetual position.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpetualPosition {
    /// Market ticker.
    pub market: String,
    /// Position status.
    pub status: DydxPositionStatus,
    /// Position side (determined by size sign).
    pub side: OrderSide,
    /// Position size (negative for short).
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
    /// Maximum size reached.
    #[serde_as(as = "DisplayFromStr")]
    pub max_size: Decimal,
    /// Average entry price.
    #[serde_as(as = "DisplayFromStr")]
    pub entry_price: Decimal,
    /// Exit price (if closed).
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_price: Option<Decimal>,
    /// Realized PnL.
    #[serde_as(as = "DisplayFromStr")]
    pub realized_pnl: Decimal,
    /// Creation height.
    #[serde_as(as = "DisplayFromStr")]
    pub created_at_height: u64,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Sum of all open order sizes.
    #[serde_as(as = "DisplayFromStr")]
    pub sum_open: Decimal,
    /// Sum of all close order sizes.
    #[serde_as(as = "DisplayFromStr")]
    pub sum_close: Decimal,
    /// Net funding paid/received.
    #[serde_as(as = "DisplayFromStr")]
    pub net_funding: Decimal,
    /// Unrealized PnL.
    #[serde_as(as = "DisplayFromStr")]
    pub unrealized_pnl: Decimal,
    /// Closed time (if closed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
}

/// Asset position (e.g., USDC balance).
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    /// Asset symbol.
    pub symbol: Ustr,
    /// Position side (API returns "LONG" but we store as string).
    pub side: String,
    /// Asset size (balance).
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
    /// Asset ID.
    pub asset_id: String,
    /// Subaccount number (present in API response).
    #[serde(default)]
    pub subaccount_number: u32,
}

/// Response for orders endpoint - API returns array directly, not wrapped.
pub type OrdersResponse = Vec<Order>;

/// Order information.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    /// Unique order ID.
    pub id: String,
    /// Subaccount ID.
    pub subaccount_id: String,
    /// Client-provided order ID.
    pub client_id: String,
    /// CLOB pair ID.
    #[serde_as(as = "DisplayFromStr")]
    pub clob_pair_id: u32,
    /// Order side.
    pub side: OrderSide,
    /// Order size.
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
    /// Total filled size.
    #[serde_as(as = "DisplayFromStr")]
    pub total_filled: Decimal,
    /// Limit price.
    #[serde_as(as = "DisplayFromStr")]
    pub price: Decimal,
    /// Order status.
    pub status: DydxOrderStatus,
    /// Order type (LIMIT, MARKET, etc.).
    #[serde(rename = "type")]
    pub order_type: String,
    /// Time-in-force.
    pub time_in_force: DydxTimeInForce,
    /// Reduce-only flag.
    pub reduce_only: bool,
    /// Post-only flag.
    pub post_only: bool,
    /// Order flags (bitfield).
    #[serde_as(as = "DisplayFromStr")]
    pub order_flags: u32,
    /// Good-til-block (for short-term orders).
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block: Option<u64>,
    /// Good-til-time (ISO8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block_time: Option<DateTime<Utc>>,
    /// Creation height (not present for BEST_EFFORT_OPENED orders).
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_height: Option<u64>,
    /// Client metadata.
    #[serde_as(as = "DisplayFromStr")]
    pub client_metadata: u32,
    /// Trigger price (for conditional orders).
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<Decimal>,
    /// Condition type (STOP_LOSS, TAKE_PROFIT, UNSPECIFIED).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_type: Option<DydxConditionType>,
    /// Conditional order trigger in subticks.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_order_trigger_subticks: Option<u64>,
    /// Order execution type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<DydxOrderExecution>,
    /// Updated timestamp (not present for BEST_EFFORT_OPENED orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    /// Updated height (not present for BEST_EFFORT_OPENED orders).
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_height: Option<u64>,
    /// Ticker symbol (e.g., "BTC-USD").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    /// Subaccount number.
    #[serde(default)]
    pub subaccount_number: u32,
    /// Order router address (empty string treated as None).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_router_address: Option<String>,
}

/// Response for fills endpoint - API returns wrapped in {"fills": [...]}.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillsResponse {
    /// Array of fills.
    pub fills: Vec<Fill>,
}

/// Order fill information.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    /// Unique fill ID.
    pub id: String,
    /// Order side.
    pub side: OrderSide,
    /// Liquidity side (MAKER/TAKER).
    pub liquidity: DydxLiquidity,
    /// Fill type.
    #[serde(rename = "type")]
    pub fill_type: DydxFillType,
    /// Market ticker.
    pub market: String,
    /// Market type.
    pub market_type: DydxTickerType,
    /// Fill price.
    #[serde_as(as = "DisplayFromStr")]
    pub price: Decimal,
    /// Fill size.
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
    /// Fee paid.
    #[serde_as(as = "DisplayFromStr")]
    pub fee: Decimal,
    /// Fill timestamp.
    pub created_at: DateTime<Utc>,
    /// Fill height.
    #[serde_as(as = "DisplayFromStr")]
    pub created_at_height: u64,
    /// Order ID.
    pub order_id: String,
    /// Client order ID.
    #[serde_as(as = "DisplayFromStr")]
    pub client_metadata: u32,
}

/// Response for transfers endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransfersResponse {
    /// List of transfers.
    pub transfers: Vec<Transfer>,
}

/// Transfer information.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    /// Unique transfer ID.
    pub id: String,
    /// Transfer type (DEPOSIT, WITHDRAWAL, TRANSFER_OUT, TRANSFER_IN).
    #[serde(rename = "type")]
    pub transfer_type: String,
    /// Sender address.
    pub sender: TransferAccount,
    /// Recipient address.
    pub recipient: TransferAccount,
    /// Asset symbol.
    pub asset: String,
    /// Transfer amount.
    #[serde_as(as = "DisplayFromStr")]
    pub amount: Decimal,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Creation height.
    #[serde_as(as = "DisplayFromStr")]
    pub created_at_height: u64,
    /// Transaction hash.
    pub transaction_hash: String,
}

/// Transfer account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferAccount {
    /// Address.
    pub address: String,
    /// Subaccount number.
    pub subaccount_number: u32,
}

////////////////////////////////////////////////////////////////////////////////
// Utility
////////////////////////////////////////////////////////////////////////////////

/// Response for time endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeResponse {
    /// Current ISO8601 timestamp.
    pub iso: DateTime<Utc>,
    /// Current Unix timestamp in milliseconds.
    #[serde(rename = "epoch")]
    pub epoch_ms: i64,
}

/// Response for height endpoint.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeightResponse {
    /// Current blockchain height.
    #[serde_as(as = "DisplayFromStr")]
    pub height: u64,
    /// Timestamp of the block.
    pub time: DateTime<Utc>,
}

////////////////////////////////////////////////////////////////////////////////
// Execution Models (Node API)
////////////////////////////////////////////////////////////////////////////////

/// Request to place an order via Node API.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceOrderRequest {
    /// Subaccount placing the order.
    pub subaccount: SubaccountId,
    /// Client-generated order ID.
    pub client_id: u32,
    /// Order type flags (bitfield for short-term, reduce-only, etc.).
    pub order_flags: u32,
    /// CLOB pair ID.
    pub clob_pair_id: u32,
    /// Order side.
    pub side: OrderSide,
    /// Order size in quantums.
    pub quantums: u64,
    /// Order subticks (price representation).
    pub subticks: u64,
    /// Time-in-force.
    pub time_in_force: DydxTimeInForce,
    /// Good-til-block (for short-term orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block: Option<u32>,
    /// Good-til-block-time (Unix seconds, for stateful orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block_time: Option<u32>,
    /// Reduce-only flag.
    pub reduce_only: bool,
    /// Optional authenticator IDs for permissioned keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticator_ids: Option<Vec<u64>>,
}

/// Subaccount identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubaccountId {
    /// Owner address.
    pub owner: String,
    /// Subaccount number.
    pub number: u32,
}

/// Request to cancel an order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderRequest {
    /// Subaccount ID.
    pub subaccount_id: SubaccountId,
    /// Client order ID to cancel.
    pub client_id: u32,
    /// CLOB pair ID.
    pub clob_pair_id: u32,
    /// Order flags.
    pub order_flags: u32,
    /// Good-til-block or good-til-block-time for the cancel.
    pub good_til_block: Option<u32>,
    pub good_til_block_time: Option<u32>,
}

/// Transaction response from Node.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResponse {
    /// Transaction hash.
    pub tx_hash: String,
    /// Block height.
    #[serde_as(as = "DisplayFromStr")]
    pub height: u64,
    /// Result code (0 = success).
    pub code: u32,
    /// Raw log output.
    pub raw_log: String,
}

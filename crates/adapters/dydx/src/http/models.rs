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

//! Data transfer objects for deserializing dYdX v4 Indexer REST API payloads.
//!
//! This module contains Rust types that mirror the JSON structures returned by the
//! dYdX v4 Indexer API. All numeric values are kept as strings to preserve precision,
//! following the official dYdX v4 client convention.
//!
//! # References
//!
//! - [dYdX v4 Indexer API Documentation](https://docs.dydx.exchange/api_integration-indexer/indexer_api)
//! - [Official v4-client-rs](https://github.com/dydxprotocol/v4-clients/tree/main/v4-client-rs)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ustr::Ustr;

// ================================================================================================
// Market Data Models
// ================================================================================================

/// Response payload returned by `GET /v4/perpetualMarkets`.
///
/// Contains a map of all perpetual markets available on dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-perpetual-markets>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxPerpetualMarketsResponse {
    /// Map of market ticker to perpetual market data.
    pub markets: HashMap<Ustr, DydxPerpetualMarket>,
}

/// Perpetual market information from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#perpetualmarketresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxPerpetualMarket {
    /// Market ticker (e.g., "BTC-USD").
    pub ticker: Ustr,
    /// Market status (ACTIVE, PAUSED, CANCEL_ONLY, POST_ONLY, INITIALIZING).
    pub status: DydxMarketStatus,
    /// Base asset symbol (e.g., "BTC").
    pub base_asset: Ustr,
    /// Quote asset symbol (always "USD" for perpetuals).
    pub quote_asset: Ustr,
    /// Minimum order size increment.
    pub step_size: String,
    /// Minimum price increment (tick size).
    pub tick_size: String,
    /// Current index price.
    pub index_price: String,
    /// Current oracle price.
    pub oracle_price: String,
    /// Initial margin fraction required to open positions.
    pub initial_margin_fraction: String,
    /// Maintenance margin fraction required to maintain positions.
    pub maintenance_margin_fraction: String,
    /// Minimum notional value for base position.
    pub base_position_notional: String,
    /// Incremental position size.
    pub incremental_position_size: String,
    /// Maximum position size allowed.
    pub max_position_size: String,
    /// Total open interest for this market.
    pub open_interest: String,
    /// Atomic resolution (exponent for quantities). dYdX-specific: used for on-chain precision.
    pub atomic_resolution: i32,
    /// Quantum conversion exponent. dYdX-specific: blockchain quantization parameter.
    pub quantum_conversion_exponent: i32,
    /// Number of subticks per tick. dYdX-specific: order book price precision control.
    pub subticks_per_tick: u32,
    /// Step size in base quantums. dYdX-specific: minimum size in blockchain units.
    pub step_base_quantums: u64,
    /// CLOB pair ID for this market. dYdX-specific: on-chain CLOB identifier.
    pub clob_pair_id: u32,
    /// Market ID.
    pub market_id: u32,
    /// Next funding rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_funding_rate: Option<String>,
}

/// Market status enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#perpetualmarketresponseobject>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxMarketStatus {
    /// Market is active and accepting all order types.
    Active,
    /// Market is paused, no new orders accepted.
    Paused,
    /// Only order cancellations are allowed.
    CancelOnly,
    /// Only post-only orders are allowed.
    PostOnly,
    /// Market is being initialized.
    Initializing,
}

/// Response payload returned by `GET /v4/orderbooks/{ticker}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-orderbook>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxOrderBookResponse {
    /// List of bid price levels [price, size].
    pub bids: Vec<DydxOrderBookLevel>,
    /// List of ask price levels [price, size].
    pub asks: Vec<DydxOrderBookLevel>,
}

/// Order book price level.
///
/// A tuple of [price, size] representing a single level in the order book.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DydxOrderBookLevel {
    /// Price level.
    pub price: String,
    /// Total size at this price level.
    pub size: String,
}

/// Response payload returned by `GET /v4/trades/{ticker}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-trades>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTradesResponse {
    /// List of recent trades.
    pub trades: Vec<DydxTrade>,
}

/// Individual trade from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#traderesponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTrade {
    /// Unique trade identifier.
    pub id: Ustr,
    /// Market ticker for this trade.
    pub ticker: Ustr,
    /// Order side (BUY or SELL).
    pub side: DydxOrderSide,
    /// Trade size.
    pub size: String,
    /// Trade price.
    pub price: String,
    /// Trade type (LIMIT, LIQUIDATION, DELEVERAGED).
    #[serde(rename = "type")]
    pub trade_type: DydxTradeType,
    /// ISO 8601 timestamp when trade was created.
    pub created_at: String,
    /// Block height when trade was created.
    pub created_at_height: String,
}

/// Trade type enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#traderesponseobject>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTradeType {
    /// Normal limit order fill.
    Limit,
    /// Trade from liquidation.
    Liquidation,
    /// Trade from deleveraging event.
    Deleveraged,
}

/// Response payload returned by `GET /v4/candles/{ticker}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxCandlesResponse {
    /// List of OHLCV candles.
    pub candles: Vec<DydxCandle>,
}

/// OHLCV candlestick data.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#candleresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxCandle {
    /// ISO 8601 timestamp for candle start time.
    pub started_at: String,
    /// Market ticker.
    pub ticker: Ustr,
    /// Candle resolution (1MIN, 5MINS, 15MINS, 30MINS, 1HOUR, 4HOURS, 1DAY).
    pub resolution: DydxCandleResolution,
    /// Lowest price during the period.
    pub low: String,
    /// Highest price during the period.
    pub high: String,
    /// Opening price.
    pub open: String,
    /// Closing price.
    pub close: String,
    /// Base asset volume.
    pub base_token_volume: String,
    /// USD volume.
    pub usd_volume: String,
    /// Number of trades in this period.
    pub trades: u32,
    /// Open interest at period start.
    pub starting_open_interest: String,
}

/// Candle resolution enumeration.
///
/// Represents different time periods for OHLCV candles.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#candleresolution>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DydxCandleResolution {
    /// 1 minute candle.
    #[serde(rename = "1MIN")]
    OneMin,
    /// 5 minute candle.
    #[serde(rename = "5MINS")]
    FiveMins,
    /// 15 minute candle.
    #[serde(rename = "15MINS")]
    FifteenMins,
    /// 30 minute candle.
    #[serde(rename = "30MINS")]
    ThirtyMins,
    /// 1 hour candle.
    #[serde(rename = "1HOUR")]
    OneHour,
    /// 4 hour candle.
    #[serde(rename = "4HOURS")]
    FourHours,
    /// 1 day candle.
    #[serde(rename = "1DAY")]
    OneDay,
}

// ================================================================================================
// Account Data Models
// ================================================================================================

/// Response payload returned by `GET /v4/addresses/{address}/subaccountNumber/{subaccountNumber}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-subaccount>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxSubaccountResponse {
    /// Subaccount details.
    pub subaccount: DydxSubaccount,
}

/// Subaccount information including positions and balances.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#subaccountresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxSubaccount {
    /// Wallet address (dydx1...).
    pub address: Ustr,
    /// Subaccount number (0-127,999).
    pub subaccount_number: u32,
    /// Total equity in USD.
    pub equity: String,
    /// Free collateral available for trading.
    pub free_collateral: String,
    /// Open perpetual positions mapped by market ticker.
    pub open_perpetual_positions: HashMap<Ustr, DydxPerpetualPosition>,
    /// Asset positions (typically USDC) mapped by symbol.
    pub asset_positions: HashMap<Ustr, DydxAssetPosition>,
    /// Whether margin trading is enabled.
    pub margin_enabled: bool,
}

/// Perpetual position information.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#perpetualpositionresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxPerpetualPosition {
    /// Market ticker for this position.
    pub market: Ustr,
    /// Position status (OPEN or CLOSED).
    pub status: DydxPositionStatus,
    /// Position side (LONG or SHORT).
    pub side: DydxPositionSide,
    /// Current position size.
    pub size: String,
    /// Maximum size this position has reached.
    pub max_size: String,
    /// Average entry price.
    pub entry_price: String,
    /// Realized profit and loss.
    pub realized_pnl: String,
    /// Unrealized profit and loss.
    pub unrealized_pnl: String,
    /// ISO 8601 timestamp when position was opened.
    pub created_at: String,
    /// Block height when position was opened.
    pub created_at_height: String,
    /// Sum of sizes for open orders (increasing position).
    pub sum_open: String,
    /// Sum of sizes for close orders (decreasing position).
    pub sum_close: String,
    /// Net funding payments.
    pub net_funding: String,
    /// Exit price (only for closed positions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_price: Option<String>,
}

/// Asset position (typically USDC balance).
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#assetpositionresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxAssetPosition {
    /// Asset symbol (typically "USDC").
    pub symbol: Ustr,
    /// Position side (LONG for positive balance, SHORT for negative).
    pub side: DydxPositionSide,
    /// Asset size/balance.
    pub size: String,
    /// Asset ID (0 for USDC).
    pub asset_id: String,
}

/// Position status enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxPositionStatus {
    /// Position is currently open.
    Open,
    /// Position has been closed.
    Closed,
}

/// Position side enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxPositionSide {
    /// Long position (or positive balance for assets).
    Long,
    /// Short position (or negative balance for assets).
    Short,
}

/// Response payload returned by `GET /v4/orders`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#list-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxOrdersResponse {
    /// List of orders.
    pub orders: Vec<DydxOrder>,
}

/// Order information from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#orderresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxOrder {
    /// Unique order identifier.
    pub id: Ustr,
    /// Subaccount ID in format "address/number". dYdX-specific: combines wallet address with subaccount.
    pub subaccount_id: Ustr,
    /// Client-assigned order ID.
    pub client_id: String,
    /// CLOB pair ID for this order. dYdX-specific: on-chain CLOB identifier.
    pub clob_pair_id: u32,
    /// Order side (BUY or SELL).
    pub side: DydxOrderSide,
    /// Order size.
    pub size: String,
    /// Total filled amount.
    pub total_filled: String,
    /// Order price.
    pub price: String,
    /// Order type (LIMIT, MARKET, STOP_LIMIT, STOP_MARKET, etc.).
    #[serde(rename = "type")]
    pub order_type: DydxOrderType,
    /// Current order status.
    pub status: DydxOrderStatus,
    /// Time in force instruction (GTT, FOK, IOC).
    pub time_in_force: DydxTimeInForce,
    /// Whether this is a post-only order.
    pub post_only: bool,
    /// Whether this is a reduce-only order.
    pub reduce_only: bool,
    /// Block number when order expires (for short-term orders). dYdX-specific: blockchain block height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block: Option<u32>,
    /// ISO 8601 timestamp when order expires (for long-term orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub good_til_block_time: Option<String>,
    /// ISO 8601 timestamp when order was created.
    pub created_at: String,
    /// Block height when order was created. dYdX-specific: blockchain block height.
    pub created_at_height: String,
    /// Client metadata (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<String>,
    /// Trigger price for conditional orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
    /// ISO 8601 timestamp when order was last updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Block height when order was last updated. dYdX-specific: blockchain block height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_height: Option<String>,
}

/// Order side enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderSide {
    /// Buy order.
    Buy,
    /// Sell order.
    Sell,
}

/// Order type enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#orderresponseobject>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderType {
    /// Limit order.
    Limit,
    /// Market order.
    Market,
    /// Stop-limit order.
    StopLimit,
    /// Take-profit limit order.
    TakeProfitLimit,
    /// Stop-market order.
    StopMarket,
    /// Take-profit market order.
    TakeProfitMarket,
}

/// Order status enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#orderresponseobject>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderStatus {
    /// Order is open and active.
    Open,
    /// Order has been fully filled.
    Filled,
    /// Order has been canceled.
    Canceled,
    /// Order canceled with best effort (may have partial fills).
    BestEffortCanceled,
    /// Conditional order not yet triggered.
    Untriggered,
}

/// Time in force enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/types/time_in_force>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTimeInForce {
    /// Good-til-time: order remains active until expiry or filled.
    Gtt,
    /// Fill-or-kill: must be filled immediately and completely or canceled.
    Fok,
    /// Immediate-or-cancel: fill as much as possible immediately, cancel remainder.
    Ioc,
}

/// Response payload returned by `GET /v4/fills`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#list-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxFillsResponse {
    /// List of fills.
    pub fills: Vec<DydxFill>,
}

/// Fill information from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#fillresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxFill {
    /// Unique fill identifier.
    pub id: Ustr,
    /// Order side (BUY or SELL).
    pub side: DydxOrderSide,
    /// Liquidity type (TAKER or MAKER).
    pub liquidity: DydxLiquidity,
    /// Fill type (LIMIT, LIQUIDATED, etc.).
    #[serde(rename = "type")]
    pub fill_type: DydxFillType,
    /// Market ticker.
    pub market: Ustr,
    /// Market type (PERPETUAL or SPOT).
    pub market_type: DydxMarketType,
    /// Fill price.
    pub price: String,
    /// Fill size.
    pub size: String,
    /// Fee paid (negative for maker rebates).
    pub fee: String,
    /// ISO 8601 timestamp when fill occurred.
    pub created_at: String,
    /// Block height when fill occurred. dYdX-specific: blockchain block height.
    pub created_at_height: String,
    /// Associated order ID (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<Ustr>,
    /// Client metadata (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<String>,
    /// Subaccount number.
    pub subaccount_number: u32,
}

/// Liquidity type enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxLiquidity {
    /// Order took liquidity from the book.
    Taker,
    /// Order added liquidity to the book.
    Maker,
}

/// Fill type enumeration.
///
/// dYdX-specific: includes liquidation and deleveraging types due to on-chain risk engine.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#fillresponseobject>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxFillType {
    /// Normal limit order fill.
    Limit,
    /// Fill from being liquidated.
    Liquidated,
    /// Fill from liquidating another position.
    Liquidation,
    /// Fill from deleveraging.
    Deleveraged,
    /// Fill from offset during liquidation.
    OffsetLiquidation,
}

/// Market type enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxMarketType {
    /// Perpetual futures market.
    Perpetual,
    /// Spot market.
    Spot,
}

// ================================================================================================
// Transfer & Funding Models
// ================================================================================================
// dYdX-specific: blockchain-based transfers differ from traditional exchange deposits/withdrawals.

/// Response payload returned by `GET /v4/transfers`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#list-transfers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTransferResponse {
    /// List of transfers.
    pub transfers: Vec<DydxTransfer>,
    /// Page size for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// Total number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_results: Option<u32>,
    /// Current offset for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Response payload returned by `GET /v4/transfers/between`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#list-transfers-between>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTransferBetweenResponse {
    /// Subset of transfers between specified accounts.
    pub transfers_subset: Vec<DydxTransfer>,
    /// Total net transfers amount.
    pub total_net_transfers: String,
    /// Page size for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// Total number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_results: Option<u32>,
    /// Current offset for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Response payload returned by `GET /v4/transfers/parentSubaccount`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#list-parent-subaccount-transfers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxParentSubaccountTransferResponse {
    /// List of parent subaccount transfers.
    pub transfers: Vec<DydxParentSubaccountTransfer>,
}

/// Transfer record from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#transferresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTransfer {
    /// Unique transfer identifier.
    pub id: Ustr,
    /// Sender account information.
    pub sender: DydxAccount,
    /// Recipient account information.
    pub recipient: DydxAccount,
    /// Transfer size/amount.
    pub size: String,
    /// ISO 8601 timestamp when transfer was created.
    pub created_at: String,
    /// Block height when transfer was created. dYdX-specific: blockchain block height.
    pub created_at_height: String,
    /// Token symbol (e.g., USDC).
    pub symbol: Ustr,
    /// Type of transfer.
    #[serde(rename = "type")]
    pub transfer_type: DydxTransferType,
    /// On-chain transaction hash. dYdX-specific: blockchain transaction identifier.
    pub transaction_hash: Ustr,
}

/// Parent subaccount transfer record.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#parentsubaccounttransferresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxParentSubaccountTransfer {
    /// Unique transfer identifier.
    pub id: Ustr,
    /// Sender account with parent subaccount number.
    pub sender: DydxAccountWithParent,
    /// Recipient account with parent subaccount number.
    pub recipient: DydxAccountWithParent,
    /// Transfer size/amount.
    pub size: String,
    /// ISO 8601 timestamp when transfer was created.
    pub created_at: String,
    /// Block height when transfer was created. dYdX-specific: blockchain block height.
    pub created_at_height: String,
    /// Token symbol (e.g., USDC).
    pub symbol: Ustr,
    /// Type of transfer.
    #[serde(rename = "type")]
    pub transfer_type: DydxTransferType,
    /// On-chain transaction hash. dYdX-specific: blockchain transaction identifier.
    pub transaction_hash: Ustr,
}

/// Account information for transfers and orders.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxAccount {
    /// dYdX address.
    pub address: Ustr,
    /// Subaccount number (0-127). dYdX-specific: each address can have up to 128 subaccounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subaccount_number: Option<u32>,
}

/// Account information with parent subaccount number.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxAccountWithParent {
    /// dYdX address.
    pub address: Ustr,
    /// Parent subaccount number. dYdX-specific: for child-to-parent subaccount transfers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_subaccount_number: Option<u32>,
}

/// Transfer type enumeration.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#transfertype>
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTransferType {
    /// Transfer into the protocol.
    TransferIn,
    /// Transfer out of the protocol.
    TransferOut,
    /// Deposit from external source.
    Deposit,
    /// Withdrawal to external destination.
    Withdrawal,
}

/// Response payload returned by `GET /v4/historicalFunding/{address}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-funding-payments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxFundingPaymentResponse {
    /// List of funding payments.
    pub funding_payments: Vec<DydxFundingPayment>,
    /// Page size for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// Total number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_results: Option<u32>,
    /// Current offset for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Funding payment record from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#fundingpaymentresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxFundingPayment {
    /// ISO 8601 timestamp when payment occurred.
    pub created_at: String,
    /// Block height when payment occurred.
    pub created_at_height: String,
    /// Perpetual market ID.
    pub perpetual_id: Ustr,
    /// Market ticker symbol.
    pub ticker: Ustr,
    /// Oracle price at time of payment.
    pub oracle_price: String,
    /// Position size at time of payment.
    pub size: String,
    /// Position side (LONG or SHORT).
    pub side: DydxFundingOrderSide,
    /// Funding rate applied.
    pub rate: String,
    /// Funding payment amount (negative means paid, positive means received).
    pub payment: String,
    /// Subaccount number.
    pub subaccount_number: u32,
    /// Funding index value.
    pub funding_index: Ustr,
}

/// Funding order side enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxFundingOrderSide {
    /// Long position.
    Long,
    /// Short position.
    Short,
}

// ================================================================================================
// PnL & Rewards Models
// ================================================================================================
// dYdX-specific: trading rewards are protocol-level incentives, not present in traditional CEXs.

/// Response payload returned by `GET /v4/historical-pnl/{address}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-historical-pnl>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHistoricalPnlResponse {
    /// List of historical PnL data points.
    pub historical_pnl: Vec<DydxPnlTick>,
}

/// PnL tick record from dYdX v4.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#pnlticksresponseobject>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxPnlTick {
    /// Block height.
    pub block_height: String,
    /// ISO 8601 timestamp of block.
    pub block_time: String,
    /// ISO 8601 timestamp when record was created.
    pub created_at: String,
    /// Account equity at this point.
    pub equity: String,
    /// Total realized and unrealized PnL.
    pub total_pnl: String,
    /// Net transfers (deposits minus withdrawals).
    pub net_transfers: String,
}

/// Response payload returned by `GET /v4/historical-block-trading-rewards/{address}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-historical-trading-rewards>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHistoricalBlockTradingRewardsResponse {
    /// List of block-level trading rewards.
    pub rewards: Vec<DydxHistoricalBlockTradingReward>,
}

/// Block trading reward record from dYdX v4.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHistoricalBlockTradingReward {
    /// Trading reward amount.
    pub trading_reward: String,
    /// Block height when reward was earned.
    pub created_at_height: String,
    /// ISO 8601 timestamp when reward was earned.
    pub created_at: String,
}

/// Response payload returned by `GET /v4/historical-trading-reward-aggregations/{address}`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-aggregated-trading-rewards>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHistoricalTradingRewardAggregationsResponse {
    /// List of aggregated trading rewards.
    pub rewards: Vec<DydxHistoricalTradingRewardAggregation>,
}

/// Aggregated trading reward record from dYdX v4.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHistoricalTradingRewardAggregation {
    /// Total trading reward amount for this period.
    pub trading_reward: String,
    /// Block height at start of period.
    pub started_at_height: String,
    /// ISO 8601 timestamp at start of period.
    pub started_at: String,
    /// Block height at end of period (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at_height: Option<String>,
    /// ISO 8601 timestamp at end of period (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    /// Aggregation period type.
    pub period: DydxTradingRewardAggregationPeriod,
}

/// Trading reward aggregation period enumeration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTradingRewardAggregationPeriod {
    /// Daily aggregation.
    Daily,
    /// Weekly aggregation.
    Weekly,
    /// Monthly aggregation.
    Monthly,
}

// ================================================================================================
// Utility Models
// ================================================================================================

/// Response payload returned by `GET /v4/time`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-time>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTimeResponse {
    /// ISO 8601 formatted timestamp.
    pub iso: String,
    /// Unix epoch time in milliseconds.
    pub epoch: f64,
}

/// Response payload returned by `GET /v4/height`.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#get-height>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxHeightResponse {
    /// Current block height.
    pub height: String,
    /// ISO 8601 timestamp.
    pub time: String,
}

// ================================================================================================
// Error Response
// ================================================================================================

/// Error response from dYdX Indexer API.
///
/// # References
///
/// - <https://docs.dydx.exchange/api_integration-indexer/indexer_api#errors>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxErrorResponse {
    /// List of error details.
    pub errors: Vec<DydxError>,
}

/// Individual error detail.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxError {
    /// Error message.
    pub msg: String,
    /// Parameter that caused the error (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Location of the error (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

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

//! Type definitions for Coinbase API responses.

use serde::{Deserialize, Serialize};

/// Product (trading pair) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub product_id: String,
    pub price: Option<String>,
    pub price_percentage_change_24h: Option<String>,
    pub volume_24h: Option<String>,
    pub volume_percentage_change_24h: Option<String>,
    pub base_increment: String,
    pub quote_increment: String,
    pub quote_min_size: String,
    pub quote_max_size: String,
    pub base_min_size: String,
    pub base_max_size: String,
    pub base_name: String,
    pub quote_name: String,
    pub watched: Option<bool>,
    pub is_disabled: bool,
    pub new: Option<bool>,
    pub status: String,
    pub cancel_only: bool,
    pub limit_only: bool,
    pub post_only: bool,
    pub trading_disabled: bool,
    pub auction_mode: bool,
    pub product_type: String,
    pub quote_currency_id: String,
    pub base_currency_id: String,
    pub mid_market_price: Option<String>,
    pub alias: Option<String>,
    pub alias_to: Option<Vec<String>>,
    pub base_display_symbol: String,
    pub quote_display_symbol: String,
    pub view_only: bool,
    pub price_increment: String,
    pub display_name: String,
    pub product_venue: String,
}

/// Response wrapper for list products
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProductsResponse {
    pub products: Vec<Product>,
    pub num_products: Option<u32>,
}

/// Account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub currency: String,
    pub available_balance: AvailableBalance,
    pub default: bool,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub ready: bool,
    pub hold: Option<AvailableBalance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableBalance {
    pub value: String,
    pub currency: String,
}

/// Response wrapper for list accounts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAccountsResponse {
    pub accounts: Vec<Account>,
    pub has_next: bool,
    pub cursor: Option<String>,
    pub size: u32,
}

/// Order side
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

/// Order status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    Open,
    Filled,
    Cancelled,
    Expired,
    Failed,
    Unknown,
}

/// Order configuration for market orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketOrderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_size: Option<String>,
}

/// Order configuration for limit orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitOrderConfig {
    pub base_size: String,
    pub limit_price: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
}

/// Order configuration for stop-limit orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLimitOrderConfig {
    pub base_size: String,
    pub limit_price: String,
    pub stop_price: String,
    pub stop_direction: StopDirection,
}

/// Stop direction for stop orders
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StopDirection {
    StopDirectionStopUp,
    StopDirectionStopDown,
}

/// Order configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OrderConfiguration {
    Market {
        market_market_ioc: MarketOrderConfig,
    },
    Limit {
        limit_limit_gtc: LimitOrderConfig,
    },
    StopLimit {
        stop_limit_stop_limit_gtc: StopLimitOrderConfig,
    },
}

/// Create order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub client_order_id: String,
    pub product_id: String,
    pub side: OrderSide,
    pub order_configuration: OrderConfiguration,
}

/// Order information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub product_id: String,
    pub user_id: String,
    pub order_configuration: serde_json::Value,
    pub side: OrderSide,
    pub client_order_id: String,
    pub status: OrderStatus,
    pub time_in_force: Option<String>,
    pub created_time: String,
    pub completion_percentage: String,
    pub filled_size: String,
    pub average_filled_price: String,
    pub fee: Option<String>,
    pub number_of_fills: String,
    pub filled_value: String,
    pub pending_cancel: bool,
    pub size_in_quote: bool,
    pub total_fees: String,
    pub size_inclusive_of_fees: bool,
    pub total_value_after_fees: String,
    pub trigger_status: Option<String>,
    pub order_type: String,
    pub reject_reason: Option<String>,
    pub settled: Option<bool>,
    pub product_type: String,
    pub reject_message: Option<String>,
    pub cancel_message: Option<String>,
}

/// Response for create order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderResponse {
    pub success: bool,
    pub failure_reason: Option<String>,
    pub order_id: Option<String>,
    pub success_response: Option<OrderSuccessResponse>,
    pub error_response: Option<OrderErrorResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderSuccessResponse {
    pub order_id: String,
    pub product_id: String,
    pub side: OrderSide,
    pub client_order_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderErrorResponse {
    pub error: String,
    pub message: String,
    pub error_details: Option<String>,
}

/// Cancel orders response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrdersResponse {
    pub results: Vec<CancelOrderResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderResult {
    pub success: bool,
    pub failure_reason: Option<String>,
    pub order_id: String,
}

/// Edit order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditOrderRequest {
    pub order_id: String,
    pub price: String,
    pub size: String,
}

/// Edit order response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditOrderResponse {
    pub success: bool,
    pub errors: Vec<EditOrderError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditOrderError {
    pub edit_failure_reason: Option<String>,
    pub preview_failure_reason: Option<String>,
}

/// Preview order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewOrderRequest {
    pub product_id: String,
    pub side: OrderSide,
    pub order_configuration: OrderConfiguration,
}

/// Preview order response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewOrderResponse {
    pub order_total: String,
    pub commission_total: String,
    pub errs: Vec<String>,
    pub warning: Vec<String>,
    pub quote_size: Option<String>,
    pub base_size: Option<String>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub is_max: Option<bool>,
    pub order_margin_total: Option<String>,
    pub leverage: Option<String>,
    pub long_leverage: Option<String>,
    pub short_leverage: Option<String>,
    pub slippage: Option<String>,
}

/// Candle (OHLCV) data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub start: String,
    pub low: String,
    pub high: String,
    pub open: String,
    pub close: String,
    pub volume: String,
}

/// Response wrapper for candles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCandlesResponse {
    pub candles: Vec<Candle>,
}

/// Market trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTrade {
    pub trade_id: String,
    pub product_id: String,
    pub price: String,
    pub size: String,
    pub time: String,
    pub side: String,
    pub bid: Option<String>,
    pub ask: Option<String>,
}

/// Response wrapper for market trades
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMarketTradesResponse {
    pub trades: Vec<MarketTrade>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
}

/// Order book entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: String,
    pub size: String,
}

/// Product book (order book)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductBook {
    pub product_id: String,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub time: String,
}

/// Response wrapper for product book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetProductBookResponse {
    pub pricebook: ProductBook,
}

/// Best bid/ask
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestBidAsk {
    pub product_id: String,
    pub price_books: Vec<PriceBook>,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}

/// Response wrapper for best bid/ask
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBestBidAskResponse {
    pub pricebooks: Vec<BestBidAsk>,
}


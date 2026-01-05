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

//! Binance Spot HTTP response models.
//!
//! These models represent Binance venue-specific response types decoded from SBE.

use crate::common::sbe::spot::{
    order_side::OrderSide, order_status::OrderStatus, order_type::OrderType,
    self_trade_prevention_mode::SelfTradePreventionMode, time_in_force::TimeInForce,
};

/// Price/quantity level in an order book.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinancePriceLevel {
    /// Price mantissa (multiply by 10^exponent to get actual price).
    pub price_mantissa: i64,
    /// Quantity mantissa (multiply by 10^exponent to get actual quantity).
    pub qty_mantissa: i64,
}

/// Binance order book depth response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceDepth {
    /// Last update ID for this depth snapshot.
    pub last_update_id: i64,
    /// Price exponent for all price levels.
    pub price_exponent: i8,
    /// Quantity exponent for all quantity values.
    pub qty_exponent: i8,
    /// Bid price levels (best bid first).
    pub bids: Vec<BinancePriceLevel>,
    /// Ask price levels (best ask first).
    pub asks: Vec<BinancePriceLevel>,
}

/// A single trade from Binance.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrade {
    /// Trade ID.
    pub id: i64,
    /// Price mantissa.
    pub price_mantissa: i64,
    /// Quantity mantissa.
    pub qty_mantissa: i64,
    /// Quote quantity mantissa (price * qty).
    pub quote_qty_mantissa: i64,
    /// Trade timestamp in microseconds (SBE precision).
    pub time: i64,
    /// Whether the buyer is the maker.
    pub is_buyer_maker: bool,
    /// Whether this trade is the best price match.
    pub is_best_match: bool,
}

/// Binance trades response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrades {
    /// Price exponent for all trades.
    pub price_exponent: i8,
    /// Quantity exponent for all trades.
    pub qty_exponent: i8,
    /// List of trades.
    pub trades: Vec<BinanceTrade>,
}

/// A fill from an order execution.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceOrderFill {
    /// Fill price mantissa.
    pub price_mantissa: i64,
    /// Fill quantity mantissa.
    pub qty_mantissa: i64,
    /// Commission mantissa.
    pub commission_mantissa: i64,
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Commission asset.
    pub commission_asset: String,
    /// Trade ID (if available).
    pub trade_id: Option<i64>,
}

/// New order response (FULL response type).
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceNewOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Transaction time in microseconds.
    pub transact_time: i64,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Stop price mantissa (for stop orders).
    pub stop_price_mantissa: Option<i64>,
    /// Working time in microseconds.
    pub working_time: Option<i64>,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Symbol.
    pub symbol: String,
    /// Order fills.
    pub fills: Vec<BinanceOrderFill>,
}

/// Cancel order response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceCancelOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Transaction time in microseconds.
    pub transact_time: i64,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Original client order ID.
    pub orig_client_order_id: String,
    /// Symbol.
    pub symbol: String,
}

/// Query order response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Stop price mantissa (for stop orders).
    pub stop_price_mantissa: Option<i64>,
    /// Iceberg quantity mantissa.
    pub iceberg_qty_mantissa: Option<i64>,
    /// Order creation time in microseconds.
    pub time: i64,
    /// Last update time in microseconds.
    pub update_time: i64,
    /// Whether the order is working.
    pub is_working: bool,
    /// Working time in microseconds.
    pub working_time: Option<i64>,
    /// Original quote order quantity mantissa.
    pub orig_quote_order_qty_mantissa: i64,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Symbol.
    pub symbol: String,
}

/// Account balance for a single asset.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceBalance {
    /// Asset symbol.
    pub asset: String,
    /// Free (available) balance mantissa.
    pub free_mantissa: i64,
    /// Locked balance mantissa.
    pub locked_mantissa: i64,
    /// Balance exponent.
    pub exponent: i8,
}

/// Account information response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceAccountInfo {
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Maker commission rate mantissa.
    pub maker_commission_mantissa: i64,
    /// Taker commission rate mantissa.
    pub taker_commission_mantissa: i64,
    /// Buyer commission rate mantissa.
    pub buyer_commission_mantissa: i64,
    /// Seller commission rate mantissa.
    pub seller_commission_mantissa: i64,
    /// Whether trading is enabled.
    pub can_trade: bool,
    /// Whether withdrawals are enabled.
    pub can_withdraw: bool,
    /// Whether deposits are enabled.
    pub can_deposit: bool,
    /// Whether the account requires self-trade prevention.
    pub require_self_trade_prevention: bool,
    /// Whether to prevent self-trade by quote order ID.
    pub prevent_sor: bool,
    /// Account update time in microseconds.
    pub update_time: i64,
    /// Account type.
    pub account_type: String,
    /// Account balances.
    pub balances: Vec<BinanceBalance>,
}

/// Price filter from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinancePriceFilterSbe {
    /// Price exponent for mantissa conversion.
    pub price_exponent: i8,
    /// Minimum price mantissa.
    pub min_price: i64,
    /// Maximum price mantissa.
    pub max_price: i64,
    /// Tick size mantissa.
    pub tick_size: i64,
}

/// Lot size filter from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceLotSizeFilterSbe {
    /// Quantity exponent for mantissa conversion.
    pub qty_exponent: i8,
    /// Minimum quantity mantissa.
    pub min_qty: i64,
    /// Maximum quantity mantissa.
    pub max_qty: i64,
    /// Step size mantissa.
    pub step_size: i64,
}

/// Symbol filters from SBE response.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BinanceSymbolFiltersSbe {
    /// Price filter (required for trading).
    pub price_filter: Option<BinancePriceFilterSbe>,
    /// Lot size filter (required for trading).
    pub lot_size_filter: Option<BinanceLotSizeFilterSbe>,
}

/// Symbol information from SBE exchange info response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceSymbolSbe {
    /// Symbol name (e.g., "BTCUSDT").
    pub symbol: String,
    /// Base asset (e.g., "BTC").
    pub base_asset: String,
    /// Quote asset (e.g., "USDT").
    pub quote_asset: String,
    /// Base asset precision.
    pub base_asset_precision: u8,
    /// Quote asset precision.
    pub quote_asset_precision: u8,
    /// Symbol status.
    pub status: u8,
    /// Order types bitset.
    pub order_types: u16,
    /// Whether iceberg orders are allowed.
    pub iceberg_allowed: bool,
    /// Whether OCO orders are allowed.
    pub oco_allowed: bool,
    /// Whether OTO orders are allowed.
    pub oto_allowed: bool,
    /// Whether quote order quantity market orders are allowed.
    pub quote_order_qty_market_allowed: bool,
    /// Whether trailing stop is allowed.
    pub allow_trailing_stop: bool,
    /// Whether cancel-replace is allowed.
    pub cancel_replace_allowed: bool,
    /// Whether amend is allowed.
    pub amend_allowed: bool,
    /// Whether spot trading is allowed.
    pub is_spot_trading_allowed: bool,
    /// Whether margin trading is allowed.
    pub is_margin_trading_allowed: bool,
    /// Symbol filters decoded from SBE.
    pub filters: BinanceSymbolFiltersSbe,
    /// Permission sets.
    pub permissions: Vec<Vec<String>>,
}

/// Exchange information from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceExchangeInfoSbe {
    /// List of symbols.
    pub symbols: Vec<BinanceSymbolSbe>,
}

/// Account trade history entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceAccountTrade {
    /// Price exponent.
    pub price_exponent: i8,
    /// Quantity exponent.
    pub qty_exponent: i8,
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Trade ID.
    pub id: i64,
    /// Order ID.
    pub order_id: i64,
    /// Order list ID (for OCO).
    pub order_list_id: Option<i64>,
    /// Trade price mantissa.
    pub price_mantissa: i64,
    /// Trade quantity mantissa.
    pub qty_mantissa: i64,
    /// Quote quantity mantissa.
    pub quote_qty_mantissa: i64,
    /// Commission mantissa.
    pub commission_mantissa: i64,
    /// Trade time in microseconds.
    pub time: i64,
    /// Whether the trade was as buyer.
    pub is_buyer: bool,
    /// Whether the trade was as maker.
    pub is_maker: bool,
    /// Whether this is the best price match.
    pub is_best_match: bool,
    /// Symbol.
    pub symbol: String,
    /// Commission asset.
    pub commission_asset: String,
}

/// Kline (candlestick) data response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceKlines {
    /// Price exponent for all klines.
    pub price_exponent: i8,
    /// Quantity exponent for all klines.
    pub qty_exponent: i8,
    /// List of klines.
    pub klines: Vec<BinanceKline>,
}

/// A single kline (candlestick) from Binance.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceKline {
    /// Kline open time in milliseconds.
    pub open_time: i64,
    /// Open price mantissa.
    pub open_price: i64,
    /// High price mantissa.
    pub high_price: i64,
    /// Low price mantissa.
    pub low_price: i64,
    /// Close price mantissa.
    pub close_price: i64,
    /// Volume (base asset) as 128-bit bytes.
    pub volume: [u8; 16],
    /// Kline close time in milliseconds.
    pub close_time: i64,
    /// Quote volume as 128-bit bytes.
    pub quote_volume: [u8; 16],
    /// Number of trades.
    pub num_trades: i64,
    /// Taker buy base volume as 128-bit bytes.
    pub taker_buy_base_volume: [u8; 16],
    /// Taker buy quote volume as 128-bit bytes.
    pub taker_buy_quote_volume: [u8; 16],
}

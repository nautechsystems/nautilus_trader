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

//! Binance Futures HTTP response models.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use crate::common::{
    enums::{
        BinanceContractStatus, BinanceFuturesOrderType, BinanceIncomeType, BinanceMarginType,
        BinanceOrderStatus, BinancePositionSide, BinancePriceMatch, BinanceSelfTradePreventionMode,
        BinanceSide, BinanceTimeInForce, BinanceTradingStatus, BinanceWorkingType,
    },
    models::BinanceRateLimit,
};

/// Server time response from `GET /fapi/v1/time`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceServerTime {
    /// Server timestamp in milliseconds.
    pub server_time: i64,
}

/// USD-M Futures exchange information response from `GET /fapi/v1/exchangeInfo`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesUsdExchangeInfo {
    /// Server timezone.
    pub timezone: String,
    /// Server timestamp in milliseconds.
    pub server_time: i64,
    /// Rate limit definitions.
    pub rate_limits: Vec<BinanceRateLimit>,
    /// Exchange-level filters.
    #[serde(default)]
    pub exchange_filters: Vec<Value>,
    /// Asset definitions.
    #[serde(default)]
    pub assets: Vec<BinanceFuturesAsset>,
    /// Trading symbols.
    pub symbols: Vec<BinanceFuturesUsdSymbol>,
}

/// Futures asset definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesAsset {
    /// Asset name.
    pub asset: Ustr,
    /// Whether margin is available.
    pub margin_available: bool,
    /// Auto asset exchange threshold.
    #[serde(default)]
    pub auto_asset_exchange: Option<String>,
}

/// USD-M Futures symbol definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesUsdSymbol {
    /// Symbol name (e.g., "BTCUSDT").
    pub symbol: Ustr,
    /// Trading pair (e.g., "BTCUSDT").
    pub pair: Ustr,
    /// Contract type (PERPETUAL, CURRENT_QUARTER, NEXT_QUARTER).
    pub contract_type: String,
    /// Delivery date timestamp.
    pub delivery_date: i64,
    /// Onboard date timestamp.
    pub onboard_date: i64,
    /// Trading status.
    pub status: BinanceTradingStatus,
    /// Maintenance margin percent.
    pub maint_margin_percent: String,
    /// Required margin percent.
    pub required_margin_percent: String,
    /// Base asset.
    pub base_asset: Ustr,
    /// Quote asset.
    pub quote_asset: Ustr,
    /// Margin asset.
    pub margin_asset: Ustr,
    /// Price precision.
    pub price_precision: i32,
    /// Quantity precision.
    pub quantity_precision: i32,
    /// Base asset precision.
    pub base_asset_precision: i32,
    /// Quote precision.
    pub quote_precision: i32,
    /// Underlying type.
    #[serde(default)]
    pub underlying_type: Option<String>,
    /// Underlying sub type.
    #[serde(default)]
    pub underlying_sub_type: Vec<String>,
    /// Settle plan.
    #[serde(default)]
    pub settle_plan: Option<i64>,
    /// Trigger protect threshold.
    #[serde(default)]
    pub trigger_protect: Option<String>,
    /// Liquidation fee.
    #[serde(default)]
    pub liquidation_fee: Option<String>,
    /// Market take bound.
    #[serde(default)]
    pub market_take_bound: Option<String>,
    /// Allowed order types.
    pub order_types: Vec<String>,
    /// Time in force options.
    pub time_in_force: Vec<String>,
    /// Symbol filters.
    pub filters: Vec<Value>,
}

/// COIN-M Futures exchange information response from `GET /dapi/v1/exchangeInfo`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesCoinExchangeInfo {
    /// Server timezone.
    pub timezone: String,
    /// Server timestamp in milliseconds.
    pub server_time: i64,
    /// Rate limit definitions.
    pub rate_limits: Vec<BinanceRateLimit>,
    /// Exchange-level filters.
    #[serde(default)]
    pub exchange_filters: Vec<Value>,
    /// Trading symbols.
    pub symbols: Vec<BinanceFuturesCoinSymbol>,
}

/// COIN-M Futures symbol definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesCoinSymbol {
    /// Symbol name (e.g., "BTCUSD_PERP").
    pub symbol: Ustr,
    /// Trading pair (e.g., "BTCUSD").
    pub pair: Ustr,
    /// Contract type (PERPETUAL, CURRENT_QUARTER, NEXT_QUARTER).
    pub contract_type: String,
    /// Delivery date timestamp.
    pub delivery_date: i64,
    /// Onboard date timestamp.
    pub onboard_date: i64,
    /// Trading status.
    #[serde(default)]
    pub contract_status: Option<BinanceContractStatus>,
    /// Contract size.
    pub contract_size: i64,
    /// Maintenance margin percent.
    pub maint_margin_percent: String,
    /// Required margin percent.
    pub required_margin_percent: String,
    /// Base asset.
    pub base_asset: Ustr,
    /// Quote asset.
    pub quote_asset: Ustr,
    /// Margin asset.
    pub margin_asset: Ustr,
    /// Price precision.
    pub price_precision: i32,
    /// Quantity precision.
    pub quantity_precision: i32,
    /// Base asset precision.
    pub base_asset_precision: i32,
    /// Quote precision.
    pub quote_precision: i32,
    /// Equal quantity precision.
    #[serde(default, rename = "equalQtyPrecision")]
    pub equal_qty_precision: Option<i32>,
    /// Trigger protect threshold.
    #[serde(default)]
    pub trigger_protect: Option<String>,
    /// Liquidation fee.
    #[serde(default)]
    pub liquidation_fee: Option<String>,
    /// Market take bound.
    #[serde(default)]
    pub market_take_bound: Option<String>,
    /// Allowed order types.
    pub order_types: Vec<String>,
    /// Time in force options.
    pub time_in_force: Vec<String>,
    /// Symbol filters.
    pub filters: Vec<Value>,
}

/// 24hr ticker price change statistics for futures.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesTicker24hr {
    /// Symbol name.
    pub symbol: Ustr,
    /// Price change in quote asset.
    pub price_change: String,
    /// Price change percentage.
    pub price_change_percent: String,
    /// Weighted average price.
    pub weighted_avg_price: String,
    /// Last traded price.
    pub last_price: String,
    /// Last traded quantity.
    #[serde(default)]
    pub last_qty: Option<String>,
    /// Opening price.
    pub open_price: String,
    /// Highest price.
    pub high_price: String,
    /// Lowest price.
    pub low_price: String,
    /// Total traded base asset volume.
    pub volume: String,
    /// Total traded quote asset volume.
    pub quote_volume: String,
    /// Statistics open time.
    pub open_time: i64,
    /// Statistics close time.
    pub close_time: i64,
    /// First trade ID.
    #[serde(default)]
    pub first_id: Option<i64>,
    /// Last trade ID.
    #[serde(default)]
    pub last_id: Option<i64>,
    /// Total number of trades.
    #[serde(default)]
    pub count: Option<i64>,
}

/// Mark price and funding rate for futures.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesMarkPrice {
    /// Symbol name.
    pub symbol: Ustr,
    /// Mark price.
    pub mark_price: String,
    /// Index price.
    #[serde(default)]
    pub index_price: Option<String>,
    /// Estimated settle price (only for delivery contracts).
    #[serde(default)]
    pub estimated_settle_price: Option<String>,
    /// Last funding rate.
    #[serde(default)]
    pub last_funding_rate: Option<String>,
    /// Next funding time.
    #[serde(default)]
    pub next_funding_time: Option<i64>,
    /// Interest rate.
    #[serde(default)]
    pub interest_rate: Option<String>,
    /// Timestamp.
    pub time: i64,
}

/// Order book depth snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOrderBook {
    /// Last update ID.
    pub last_update_id: i64,
    /// Bid levels as `[price, quantity]` arrays.
    pub bids: Vec<(String, String)>,
    /// Ask levels as `[price, quantity]` arrays.
    pub asks: Vec<(String, String)>,
    /// Message output time.
    #[serde(default, rename = "E")]
    pub event_time: Option<i64>,
    /// Transaction time.
    #[serde(default, rename = "T")]
    pub transaction_time: Option<i64>,
}

/// Best bid/ask from book ticker endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceBookTicker {
    /// Symbol name.
    pub symbol: Ustr,
    /// Best bid price.
    pub bid_price: String,
    /// Best bid quantity.
    pub bid_qty: String,
    /// Best ask price.
    pub ask_price: String,
    /// Best ask quantity.
    pub ask_qty: String,
    /// Event time.
    #[serde(default)]
    pub time: Option<i64>,
}

/// Price ticker.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePriceTicker {
    /// Symbol name.
    pub symbol: Ustr,
    /// Current price.
    pub price: String,
    /// Event time.
    #[serde(default)]
    pub time: Option<i64>,
}

/// Funding rate history record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFundingRate {
    /// Symbol name.
    pub symbol: Ustr,
    /// Funding rate value.
    pub funding_rate: String,
    /// Funding time in milliseconds.
    pub funding_time: i64,
    /// Mark price at the funding time.
    #[serde(default)]
    pub mark_price: Option<String>,
    /// Index price at the funding time.
    #[serde(default)]
    pub index_price: Option<String>,
}

/// Open interest record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOpenInterest {
    /// Symbol name.
    pub symbol: Ustr,
    /// Total open interest.
    pub open_interest: String,
    /// Timestamp in milliseconds.
    pub time: i64,
}

/// Futures account balance entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesBalance {
    /// Account alias (only USD-M).
    #[serde(default)]
    pub account_alias: Option<String>,
    /// Asset code (e.g., "USDT").
    pub asset: Ustr,
    /// Total balance.
    pub balance: String,
    /// Cross wallet balance.
    #[serde(default)]
    pub cross_wallet_balance: Option<String>,
    /// Unrealized PnL for cross positions.
    #[serde(default)]
    pub cross_un_pnl: Option<String>,
    /// Available balance.
    pub available_balance: String,
    /// Maximum withdrawable amount.
    #[serde(default)]
    pub max_withdraw_amount: Option<String>,
    /// Whether margin trading is available.
    #[serde(default)]
    pub margin_available: Option<bool>,
    /// Timestamp of last update in milliseconds.
    pub update_time: i64,
    /// Withdrawable amount (COIN-M specific).
    #[serde(default)]
    pub withdraw_available: Option<String>,
}

/// Position risk record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePositionRisk {
    /// Symbol name.
    pub symbol: Ustr,
    /// Position quantity.
    pub position_amt: String,
    /// Entry price.
    pub entry_price: String,
    /// Mark price.
    pub mark_price: String,
    /// Unrealized profit and loss.
    #[serde(default)]
    pub un_realized_profit: Option<String>,
    /// Liquidation price.
    #[serde(default)]
    pub liquidation_price: Option<String>,
    /// Applied leverage.
    pub leverage: String,
    /// Max notional value.
    #[serde(default)]
    pub max_notional_value: Option<String>,
    /// Margin type (CROSSED or ISOLATED).
    #[serde(default)]
    pub margin_type: Option<BinanceMarginType>,
    /// Isolated margin amount.
    #[serde(default)]
    pub isolated_margin: Option<String>,
    /// Auto add margin flag.
    #[serde(default)]
    pub is_auto_add_margin: Option<bool>,
    /// Position side (BOTH, LONG, SHORT).
    #[serde(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Notional position value.
    #[serde(default)]
    pub notional: Option<String>,
    /// Isolated wallet balance.
    #[serde(default)]
    pub isolated_wallet: Option<String>,
    /// ADL quantile indicator.
    #[serde(default)]
    pub adl_quantile: Option<u8>,
    /// Last update time.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Break-even price.
    #[serde(default)]
    pub break_even_price: Option<String>,
    /// Bankruptcy price.
    #[serde(default)]
    pub bust_price: Option<String>,
}

/// Income history record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceIncomeRecord {
    /// Symbol name (may be empty for transfers).
    #[serde(default)]
    pub symbol: Option<Ustr>,
    /// Income type (e.g., FUNDING_FEE, COMMISSION).
    pub income_type: BinanceIncomeType,
    /// Income amount.
    pub income: String,
    /// Asset code.
    pub asset: Ustr,
    /// Event time in milliseconds.
    pub time: i64,
    /// Additional info field.
    #[serde(default)]
    pub info: Option<String>,
    /// Transaction ID.
    #[serde(default)]
    pub tran_id: Option<i64>,
    /// Related trade ID.
    #[serde(default)]
    pub trade_id: Option<i64>,
}

/// User trade record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceUserTrade {
    /// Symbol name.
    pub symbol: Ustr,
    /// Trade ID.
    pub id: i64,
    /// Order ID.
    pub order_id: i64,
    /// Trade price.
    pub price: String,
    /// Executed quantity.
    pub qty: String,
    /// Quote quantity.
    #[serde(default)]
    pub quote_qty: Option<String>,
    /// Realized PnL for the trade.
    pub realized_pnl: String,
    /// Buy/sell side.
    pub side: BinanceSide,
    /// Position side (BOTH, LONG, SHORT).
    #[serde(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Trade time in milliseconds.
    pub time: i64,
    /// Was the buyer the maker?
    pub buyer: bool,
    /// Was the trade maker liquidity?
    pub maker: bool,
    /// Commission paid.
    #[serde(default)]
    pub commission: Option<String>,
    /// Commission asset.
    #[serde(default)]
    pub commission_asset: Option<Ustr>,
    /// Margin asset (if provided).
    #[serde(default)]
    pub margin_asset: Option<Ustr>,
}

/// Futures order information.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesOrder {
    /// Symbol name.
    pub symbol: Ustr,
    /// Order ID.
    pub order_id: i64,
    /// Client order ID.
    pub client_order_id: String,
    /// Original order quantity.
    pub orig_qty: String,
    /// Executed quantity.
    pub executed_qty: String,
    /// Cumulative quote asset transacted.
    pub cum_quote: String,
    /// Original limit price.
    pub price: String,
    /// Average execution price.
    #[serde(default)]
    pub avg_price: Option<String>,
    /// Stop price.
    #[serde(default)]
    pub stop_price: Option<String>,
    /// Order status.
    pub status: BinanceOrderStatus,
    /// Time in force.
    pub time_in_force: BinanceTimeInForce,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: BinanceFuturesOrderType,
    /// Original order type.
    #[serde(default)]
    pub orig_type: Option<BinanceFuturesOrderType>,
    /// Order side (BUY/SELL).
    pub side: BinanceSide,
    /// Position side (BOTH/LONG/SHORT).
    #[serde(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Reduce-only flag.
    #[serde(default)]
    pub reduce_only: Option<bool>,
    /// Close position flag (for stop orders).
    #[serde(default)]
    pub close_position: Option<bool>,
    /// Trailing delta activation price.
    #[serde(default)]
    pub activate_price: Option<String>,
    /// Trailing callback rate.
    #[serde(default)]
    pub price_rate: Option<String>,
    /// Working type (CONTRACT_PRICE or MARK_PRICE).
    #[serde(default)]
    pub working_type: Option<BinanceWorkingType>,
    /// Whether price protection is enabled.
    #[serde(default)]
    pub price_protect: Option<bool>,
    /// Whether order uses isolated margin.
    #[serde(default)]
    pub is_isolated: Option<bool>,
    /// Good till date (for GTD orders).
    #[serde(default)]
    pub good_till_date: Option<i64>,
    /// Price match mode.
    #[serde(default)]
    pub price_match: Option<BinancePriceMatch>,
    /// Self-trade prevention mode.
    #[serde(default)]
    pub self_trade_prevention_mode: Option<BinanceSelfTradePreventionMode>,
    /// Last update time.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Working order ID for tracking.
    #[serde(default)]
    pub working_type_id: Option<i64>,
}

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

//! Binance HTTP response models.
//!
//! This module contains data transfer objects for deserializing Binance REST API responses.

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

/// Server time response from `GET /api/v3/time`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceServerTime {
    /// Server timestamp in milliseconds.
    pub server_time: i64,
}

/// Spot exchange information response from `GET /api/v3/exchangeInfo`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceSpotExchangeInfo {
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
    pub symbols: Vec<BinanceSpotSymbol>,
}

/// Spot symbol definition.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceSpotSymbol {
    /// Symbol name (e.g., "BTCUSDT").
    pub symbol: Ustr,
    /// Trading status.
    pub status: BinanceTradingStatus,
    /// Base asset (e.g., "BTC").
    pub base_asset: Ustr,
    /// Base asset precision.
    pub base_asset_precision: i32,
    /// Quote asset (e.g., "USDT").
    pub quote_asset: Ustr,
    /// Quote asset precision.
    pub quote_precision: i32,
    /// Quote asset precision (duplicate field in some responses).
    #[serde(default)]
    pub quote_asset_precision: Option<i32>,
    /// Allowed order types.
    pub order_types: Vec<String>,
    /// Whether iceberg orders are allowed.
    pub iceberg_allowed: bool,
    /// Whether OCO orders are allowed.
    #[serde(default)]
    pub oco_allowed: Option<bool>,
    /// Whether quote order quantity market orders are allowed.
    #[serde(default)]
    pub quote_order_qty_market_allowed: Option<bool>,
    /// Whether trailing delta is allowed.
    #[serde(default)]
    pub allow_trailing_stop: Option<bool>,
    /// Whether spot trading is allowed.
    #[serde(default)]
    pub is_spot_trading_allowed: Option<bool>,
    /// Whether margin trading is allowed.
    #[serde(default)]
    pub is_margin_trading_allowed: Option<bool>,
    /// Symbol filters (price, lot size, notional, etc.).
    pub filters: Vec<Value>,
    /// Permissions for the symbol.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Permission sets.
    #[serde(default)]
    pub permission_sets: Vec<Vec<String>>,
    /// Default self trade prevention mode.
    #[serde(default)]
    pub default_self_trade_prevention_mode: Option<String>,
    /// Allowed self trade prevention modes.
    #[serde(default)]
    pub allowed_self_trade_prevention_modes: Vec<String>,
}

/// USD-M Futures exchange information response from `GET /fapi/v1/exchangeInfo`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
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
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
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
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/coin-margined-futures/market-data/Exchange-Information>
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
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/coin-margined-futures/market-data/Exchange-Information>
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

/// 24hr ticker price change statistics for spot.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceSpotTicker24hr {
    /// Symbol name.
    pub symbol: Ustr,
    /// Price change in quote asset.
    pub price_change: String,
    /// Price change percentage.
    pub price_change_percent: String,
    /// Weighted average price.
    pub weighted_avg_price: String,
    /// Previous close price.
    #[serde(default)]
    pub prev_close_price: Option<String>,
    /// Last traded price.
    pub last_price: String,
    /// Last traded quantity.
    #[serde(default)]
    pub last_qty: Option<String>,
    /// Best bid price.
    pub bid_price: String,
    /// Best bid quantity.
    #[serde(default)]
    pub bid_qty: Option<String>,
    /// Best ask price.
    pub ask_price: String,
    /// Best ask quantity.
    #[serde(default)]
    pub ask_qty: Option<String>,
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

/// 24hr ticker price change statistics for futures.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/24hr-Ticker-Price-Change-Statistics>
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
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Mark-Price>
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

/// Recent trade from `GET /api/v3/trades` or `GET /fapi/v1/trades`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceTrade {
    /// Trade ID.
    pub id: i64,
    /// Trade price.
    pub price: String,
    /// Trade quantity.
    pub qty: String,
    /// Quote asset quantity.
    #[serde(default)]
    pub quote_qty: Option<String>,
    /// Trade timestamp in milliseconds.
    pub time: i64,
    /// Was the buyer the maker?
    pub is_buyer_maker: bool,
    /// Was this the best price match?
    #[serde(default)]
    pub is_best_match: Option<bool>,
}

/// Aggregated trade from `GET /api/v3/aggTrades` or `GET /fapi/v1/aggTrades`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceAggTrade {
    /// Aggregate trade ID.
    #[serde(rename = "a")]
    pub agg_trade_id: i64,
    /// Trade price.
    #[serde(rename = "p")]
    pub price: String,
    /// Trade quantity.
    #[serde(rename = "q")]
    pub qty: String,
    /// First trade ID.
    #[serde(rename = "f")]
    pub first_trade_id: i64,
    /// Last trade ID.
    #[serde(rename = "l")]
    pub last_trade_id: i64,
    /// Trade timestamp in milliseconds.
    #[serde(rename = "T")]
    pub time: i64,
    /// Was the buyer the maker?
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
    /// Was this the best price match? (spot only)
    #[serde(default, rename = "M")]
    pub is_best_match: Option<bool>,
}

/// Raw kline data as returned by Binance (array format).
///
/// Binance returns klines as arrays: `[openTime, open, high, low, close, volume, closeTime,
/// quoteVolume, trades, takerBuyBaseVol, takerBuyQuoteVol, ignore]`
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
pub type BinanceKlineRaw = (
    i64,    // 0: Open time
    String, // 1: Open price
    String, // 2: High price
    String, // 3: Low price
    String, // 4: Close price
    String, // 5: Volume
    i64,    // 6: Close time
    String, // 7: Quote asset volume
    i64,    // 8: Number of trades
    String, // 9: Taker buy base asset volume
    String, // 10: Taker buy quote asset volume
    String, // 11: Ignore
);

/// Parsed kline/candlestick data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinanceKline {
    /// Kline open timestamp in milliseconds.
    pub open_time: i64,
    /// Open price.
    pub open: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Close price.
    pub close: String,
    /// Base asset volume.
    pub volume: String,
    /// Kline close timestamp in milliseconds.
    pub close_time: i64,
    /// Quote asset volume.
    pub quote_volume: String,
    /// Number of trades.
    pub trade_count: i64,
    /// Taker buy base asset volume.
    pub taker_buy_base_volume: String,
    /// Taker buy quote asset volume.
    pub taker_buy_quote_volume: String,
}

impl From<BinanceKlineRaw> for BinanceKline {
    fn from(raw: BinanceKlineRaw) -> Self {
        Self {
            open_time: raw.0,
            open: raw.1,
            high: raw.2,
            low: raw.3,
            close: raw.4,
            volume: raw.5,
            close_time: raw.6,
            quote_volume: raw.7,
            trade_count: raw.8,
            taker_buy_base_volume: raw.9,
            taker_buy_quote_volume: raw.10,
        }
    }
}

/// Order book depth snapshot from `GET /api/v3/depth` or `GET /fapi/v1/depth`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOrderBook {
    /// Last update ID.
    pub last_update_id: i64,
    /// Bid levels as `[price, quantity]` arrays.
    pub bids: Vec<(String, String)>,
    /// Ask levels as `[price, quantity]` arrays.
    pub asks: Vec<(String, String)>,
    /// Message output time (futures only).
    #[serde(default, rename = "E")]
    pub event_time: Option<i64>,
    /// Transaction time (futures only).
    #[serde(default, rename = "T")]
    pub transaction_time: Option<i64>,
}

/// Best bid/ask from `GET /api/v3/ticker/bookTicker` or `GET /fapi/v1/ticker/bookTicker`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
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
    /// Event time (futures only).
    #[serde(default)]
    pub time: Option<i64>,
}

/// Price ticker from `GET /api/v3/ticker/price` or `GET /fapi/v1/ticker/price`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePriceTicker {
    /// Symbol name.
    pub symbol: Ustr,
    /// Current price.
    pub price: String,
    /// Event time (futures only).
    #[serde(default)]
    pub time: Option<i64>,
}

/// Funding rate history record from `GET /fapi/v1/fundingRate` or `GET /dapi/v1/fundingRate`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Get-Funding-Rate-History>
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

/// Open interest record from `GET /fapi/v1/openInterest` or `GET /dapi/v1/openInterest`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Open-Interest>
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

/// Futures account balance entry from `GET /fapi/v2/balance` or `GET /dapi/v1/balance`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account>
/// - <https://developers.binance.com/docs/derivatives/coin-margined-futures/user-data/account>
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

/// Position risk record from `GET /fapi/v2/positionRisk` or `GET /dapi/v1/positionRisk`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account#position-information-v2-user_data>
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

/// Income history record from `GET /fapi/v1/income` or `GET /dapi/v1/income`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account#income-history-user_data>
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

/// User trade record from `GET /fapi/v1/userTrades` or `GET /dapi/v1/userTrades`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/trade#account-trade-list-user_data>
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

/// Futures order information returned by `GET /fapi/v1/order` or `GET /fapi/v1/openOrders`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/order#query-order-user_data>
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
    /// Price match mode (futures only).
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

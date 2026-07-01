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

use anyhow::Context;
use nautilus_core::{
    UUID4, UnixNanos,
    serialization::{
        deserialize_decimal_or_zero, deserialize_optional_decimal_from_str,
        serialize_decimal_as_str, serialize_optional_decimal_as_str,
    },
};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use crate::{
    common::{
        consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        encoder::decode_broker_id,
        enums::{
            BinanceAlgoStatus, BinanceAlgoType, BinanceContractStatus, BinanceFuturesOrderType,
            BinanceIncomeType, BinanceMarginType, BinanceOrderStatus, BinancePositionSide,
            BinancePriceMatch, BinanceSelfTradePreventionMode, BinanceSide, BinanceTimeInForce,
            BinanceTradingStatus, BinanceWorkingType,
        },
        models::BinanceRateLimit,
        parse::parse_required_decimal,
    },
    futures::conversions::normalize_futures_asset,
};

/// Server time response from `GET /fapi/v1/time`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceServerTime {
    /// Server timestamp in milliseconds.
    pub server_time: i64,
}

/// Public trade from `GET /fapi/v1/trades`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesTrade {
    /// Trade ID.
    pub id: i64,
    /// Trade price.
    pub price: String,
    /// Trade quantity.
    pub qty: String,
    /// Quote asset quantity.
    pub quote_qty: String,
    /// Trade timestamp in milliseconds.
    pub time: i64,
    /// Whether the buyer is the maker.
    pub is_buyer_maker: bool,
}

/// Kline/candlestick data from `GET /fapi/v1/klines`.
#[derive(Clone, Debug)]
pub struct BinanceFuturesKline {
    /// Open time in milliseconds.
    pub open_time: i64,
    /// Open price.
    pub open: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Close price.
    pub close: String,
    /// Volume.
    pub volume: String,
    /// Close time in milliseconds.
    pub close_time: i64,
    /// Quote asset volume.
    pub quote_volume: String,
    /// Number of trades.
    pub num_trades: i64,
    /// Taker buy base volume.
    pub taker_buy_base_volume: String,
    /// Taker buy quote volume.
    pub taker_buy_quote_volume: String,
}

impl<'de> Deserialize<'de> for BinanceFuturesKline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr: Vec<Value> = Vec::deserialize(deserializer)?;
        if arr.len() < 11 {
            return Err(serde::de::Error::custom("Invalid kline array length"));
        }

        Ok(Self {
            open_time: required_kline_i64::<D::Error>(&arr, 0, "open_time")?,
            open: required_kline_string::<D::Error>(&arr, 1, "open")?,
            high: required_kline_string::<D::Error>(&arr, 2, "high")?,
            low: required_kline_string::<D::Error>(&arr, 3, "low")?,
            close: required_kline_string::<D::Error>(&arr, 4, "close")?,
            volume: required_kline_string::<D::Error>(&arr, 5, "volume")?,
            close_time: required_kline_i64::<D::Error>(&arr, 6, "close_time")?,
            quote_volume: required_kline_string::<D::Error>(&arr, 7, "quote_volume")?,
            num_trades: required_kline_i64::<D::Error>(&arr, 8, "num_trades")?,
            taker_buy_base_volume: required_kline_string::<D::Error>(
                &arr,
                9,
                "taker_buy_base_volume",
            )?,
            taker_buy_quote_volume: required_kline_string::<D::Error>(
                &arr,
                10,
                "taker_buy_quote_volume",
            )?,
        })
    }
}

fn required_kline_i64<E>(arr: &[Value], index: usize, field: &str) -> Result<i64, E>
where
    E: serde::de::Error,
{
    arr[index]
        .as_i64()
        .ok_or_else(|| E::custom(format!("invalid kline {field}")))
}

fn required_kline_string<E>(arr: &[Value], index: usize, field: &str) -> Result<String, E>
where
    E: serde::de::Error,
{
    arr[index]
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| E::custom(format!("invalid kline {field}")))
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

/// Historical open interest record from `GET /futures/data/openInterestHist`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOpenInterestHistRecord {
    /// Symbol name for USD-M responses.
    #[serde(default)]
    pub symbol: Option<Ustr>,
    /// Trading pair for COIN-M responses.
    #[serde(default)]
    pub pair: Option<Ustr>,
    /// Contract type for COIN-M responses.
    #[serde(default)]
    pub contract_type: Option<String>,
    /// Total open interest for the bucket.
    pub sum_open_interest: String,
    /// Total open interest notional value for the bucket.
    pub sum_open_interest_value: String,
    /// Bucket timestamp in milliseconds.
    pub timestamp: i64,
    /// USD-M-specific optional circulating supply field.
    #[serde(default, rename = "CMCCirculatingSupply")]
    pub cmc_circulating_supply: Option<String>,
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
    /// Wallet balance (v2 uses walletBalance, v1 uses balance).
    #[serde(
        alias = "balance",
        deserialize_with = "deserialize_decimal_or_zero",
        serialize_with = "serialize_decimal_as_str"
    )]
    pub wallet_balance: Decimal,
    /// Unrealized profit.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub unrealized_profit: Option<Decimal>,
    /// Margin balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub margin_balance: Option<Decimal>,
    /// Maintenance margin required.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub maint_margin: Option<Decimal>,
    /// Initial margin required.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub initial_margin: Option<Decimal>,
    /// Position initial margin.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub position_initial_margin: Option<Decimal>,
    /// Open order initial margin.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub open_order_initial_margin: Option<Decimal>,
    /// Cross wallet balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub cross_wallet_balance: Option<Decimal>,
    /// Unrealized PnL for cross positions.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub cross_un_pnl: Option<Decimal>,
    /// Available balance.
    #[serde(
        deserialize_with = "deserialize_decimal_or_zero",
        serialize_with = "serialize_decimal_as_str"
    )]
    pub available_balance: Decimal,
    /// Maximum withdrawable amount.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub max_withdraw_amount: Option<Decimal>,
    /// Whether margin trading is available.
    #[serde(default)]
    pub margin_available: Option<bool>,
    /// Timestamp of last update in milliseconds.
    pub update_time: i64,
    /// Withdrawable amount (COIN-M specific).
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub withdraw_available: Option<Decimal>,
}

/// Account position from `GET /fapi/v2/account` positions array.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceAccountPosition {
    /// Symbol name.
    pub symbol: Ustr,
    /// Initial margin.
    #[serde(default)]
    pub initial_margin: Option<String>,
    /// Maintenance margin.
    #[serde(default)]
    pub maint_margin: Option<String>,
    /// Unrealized profit.
    #[serde(default)]
    pub unrealized_profit: Option<String>,
    /// Position initial margin.
    #[serde(default)]
    pub position_initial_margin: Option<String>,
    /// Open order initial margin.
    #[serde(default)]
    pub open_order_initial_margin: Option<String>,
    /// Leverage.
    #[serde(default)]
    pub leverage: Option<String>,
    /// Isolated margin mode.
    #[serde(default)]
    pub isolated: Option<bool>,
    /// Entry price.
    #[serde(default)]
    pub entry_price: Option<String>,
    /// Max notional value.
    #[serde(default)]
    pub max_notional: Option<String>,
    /// Bid notional.
    #[serde(default)]
    pub bid_notional: Option<String>,
    /// Ask notional.
    #[serde(default)]
    pub ask_notional: Option<String>,
    /// Position side (BOTH, LONG, SHORT).
    #[serde(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Position amount.
    #[serde(default)]
    pub position_amt: Option<String>,
    /// Update time.
    #[serde(default)]
    pub update_time: Option<i64>,
}

/// Position risk from `GET /fapi/v2/positionRisk`.
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
    /// Auto add margin flag (as string from API).
    #[serde(default)]
    pub is_auto_add_margin: Option<String>,
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

/// Futures account information from `GET /fapi/v2/account` or `GET /dapi/v1/account`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesAccountInfo {
    /// Total initial margin required.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_initial_margin: Option<Decimal>,
    /// Total maintenance margin required.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_maint_margin: Option<Decimal>,
    /// Total wallet balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_wallet_balance: Option<Decimal>,
    /// Total unrealized profit.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_unrealized_profit: Option<Decimal>,
    /// Total margin balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_margin_balance: Option<Decimal>,
    /// Total position initial margin.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_position_initial_margin: Option<Decimal>,
    /// Total open order initial margin.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_open_order_initial_margin: Option<Decimal>,
    /// Total cross wallet balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_cross_wallet_balance: Option<Decimal>,
    /// Total cross unrealized PnL.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub total_cross_un_pnl: Option<Decimal>,
    /// Available balance.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub available_balance: Option<Decimal>,
    /// Max withdraw amount.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub max_withdraw_amount: Option<Decimal>,
    /// Can deposit.
    #[serde(default)]
    pub can_deposit: Option<bool>,
    /// Can trade.
    #[serde(default)]
    pub can_trade: Option<bool>,
    /// Can withdraw.
    #[serde(default)]
    pub can_withdraw: Option<bool>,
    /// Multi-assets margin mode.
    #[serde(default)]
    pub multi_assets_margin: Option<bool>,
    /// Update time.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Account balances.
    #[serde(default)]
    pub assets: Vec<BinanceFuturesBalance>,
    /// Account positions.
    #[serde(default)]
    pub positions: Vec<BinanceAccountPosition>,
}

impl BinanceFuturesAccountInfo {
    /// Converts this Binance account info to a Nautilus [`AccountState`].
    ///
    /// # Errors
    ///
    /// Returns an error if balance parsing fails.
    pub fn to_account_state(
        &self,
        account_id: AccountId,
        ts_init: UnixNanos,
    ) -> anyhow::Result<AccountState> {
        let mut balances = Vec::with_capacity(self.assets.len());

        for asset in &self.assets {
            let currency = Currency::get_or_create_crypto_with_context(
                asset.asset.as_str(),
                Some("futures balance"),
            );

            let balance = AccountBalance::from_total_and_free(
                asset.wallet_balance,
                asset.available_balance,
                currency,
            )
            .context("failed to build account balance")?;
            balances.push(balance);
        }

        // Ensure at least one balance exists
        if balances.is_empty() {
            let zero_currency = Currency::USDT();
            let zero_money = Money::zero(zero_currency);
            let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
            balances.push(zero_balance);
        }

        // Emit account-wide (cross-margin) margin balances per collateral asset.
        // Binance reports per-asset `initialMargin` / `maintMargin` which covers both
        // USDT-M (single collateral, typically USDT or BNB under multi-assets mode) and
        // COIN-M (one entry per base coin, e.g. BTC / ETH).
        let mut margins = Vec::new();

        for asset in &self.assets {
            let initial_dec = asset.initial_margin.unwrap_or_default();
            let maint_dec = asset.maint_margin.unwrap_or_default();

            if initial_dec.is_zero() && maint_dec.is_zero() {
                continue;
            }

            let currency = Currency::get_or_create_crypto_with_context(
                asset.asset.as_str(),
                Some("futures margin"),
            );
            let initial = Money::from_decimal(initial_dec, currency)
                .unwrap_or_else(|_| Money::zero(currency));
            let maintenance =
                Money::from_decimal(maint_dec, currency).unwrap_or_else(|_| Money::zero(currency));
            margins.push(MarginBalance::new(initial, maintenance, None));
        }

        let ts_event = self
            .update_time
            .map_or(ts_init, |t| UnixNanos::from_millis(t as u64));

        Ok(AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            margins,
            vec![],
            true, // is_reported
            UUID4::new(),
            ts_event,
            ts_init,
            None,
        ))
    }
}

/// Hedge mode (dual side position) response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceHedgeModeResponse {
    /// Whether dual side position mode is enabled.
    pub dual_side_position: bool,
}

/// Leverage change response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceLeverageResponse {
    /// Symbol.
    pub symbol: Ustr,
    /// New leverage value.
    pub leverage: u32,
    /// Max notional value at this leverage.
    #[serde(default)]
    pub max_notional_value: Option<String>,
}

/// Cancel all orders response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceCancelAllOrdersResponse {
    /// Response code (200 = success).
    pub code: i32,
    /// Response message.
    pub msg: String,
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
    #[serde(default = "zero_decimal_string")]
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

fn zero_decimal_string() -> String {
    "0".to_string()
}

impl BinanceFuturesOrder {
    /// Converts this Binance order to a Nautilus [`OrderStatusReport`].
    ///
    /// # Errors
    ///
    /// Returns an error if quantity or price parsing fails.
    pub fn to_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        treat_expired_as_canceled: bool,
        ts_init: UnixNanos,
    ) -> anyhow::Result<OrderStatusReport> {
        let ts_event = self
            .update_time
            .map_or(ts_init, |t| UnixNanos::from_millis(t as u64));

        let client_order_id = ClientOrderId::new(decode_broker_id(
            &self.client_order_id,
            BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        ));
        let venue_order_id = VenueOrderId::new(self.order_id.to_string());

        let order_side = match self.side {
            BinanceSide::Buy => OrderSide::Buy,
            BinanceSide::Sell => OrderSide::Sell,
        };

        let order_type = self.order_type.to_nautilus_order_type();
        let time_in_force = self.time_in_force.to_nautilus_time_in_force();
        let order_status = self
            .status
            .to_nautilus_order_status(treat_expired_as_canceled);

        let quantity: Decimal = self.orig_qty.parse().context("invalid orig_qty")?;
        let filled_qty: Decimal = self.executed_qty.parse().context("invalid executed_qty")?;
        let price = if self.price.is_empty() {
            None
        } else {
            let price: Decimal = self.price.parse().context("invalid price")?;
            if price == Decimal::ZERO {
                None
            } else {
                Some(
                    Price::from_decimal_dp(price, price_precision)
                        .context("invalid price precision")?,
                )
            }
        };

        let mut report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            Quantity::from_decimal_dp(quantity, size_precision)
                .context("invalid orig_qty precision")?,
            Quantity::from_decimal_dp(filled_qty, size_precision)
                .context("invalid executed_qty precision")?,
            ts_event,
            ts_event,
            ts_init,
            Some(UUID4::new()),
        );

        if let Some(price) = price {
            report = report.with_price(price);
        }

        Ok(report)
    }
}

impl BinanceFuturesOrderType {
    /// Returns whether this order type is post-only.
    #[must_use]
    pub fn is_post_only(&self) -> bool {
        false // Binance Futures doesn't have a dedicated post-only type
    }

    /// Converts to Nautilus order type.
    #[must_use]
    pub fn to_nautilus_order_type(&self) -> OrderType {
        match self {
            Self::Market => OrderType::Market,
            Self::Limit => OrderType::Limit,
            Self::Stop => OrderType::StopLimit,
            Self::StopMarket => OrderType::StopMarket,
            Self::TakeProfit => OrderType::LimitIfTouched,
            Self::TakeProfitMarket => OrderType::MarketIfTouched,
            Self::TrailingStopMarket => OrderType::TrailingStopMarket,
            Self::Liquidation | Self::Adl => OrderType::Market, // Forced closes
            Self::Unknown => OrderType::Market,
        }
    }
}

impl BinanceTimeInForce {
    /// Converts to Nautilus time in force.
    #[must_use]
    pub fn to_nautilus_time_in_force(&self) -> TimeInForce {
        match self {
            Self::Gtc => TimeInForce::Gtc,
            Self::Ioc => TimeInForce::Ioc,
            Self::Fok => TimeInForce::Fok,
            Self::Gtx => TimeInForce::Gtc, // GTX is GTC with post-only
            Self::Gtd => TimeInForce::Gtd,
            Self::Rpi => TimeInForce::Ioc, // RPI behaves as immediate
            Self::Unknown => TimeInForce::Gtc, // default
        }
    }
}

impl BinanceOrderStatus {
    /// Converts to Nautilus order status.
    #[must_use]
    pub fn to_nautilus_order_status(&self, treat_expired_as_canceled: bool) -> OrderStatus {
        match self {
            Self::New | Self::PendingNew => OrderStatus::Accepted,
            Self::PartiallyFilled => OrderStatus::PartiallyFilled,
            Self::Filled | Self::NewAdl | Self::NewInsurance => OrderStatus::Filled,
            Self::Canceled => OrderStatus::Canceled,
            Self::PendingCancel => OrderStatus::PendingCancel,
            Self::Rejected => OrderStatus::Rejected,
            Self::Expired | Self::ExpiredInMatch => {
                if treat_expired_as_canceled {
                    OrderStatus::Canceled
                } else {
                    OrderStatus::Expired
                }
            }
            Self::Unknown => OrderStatus::Initialized,
        }
    }
}

impl BinanceUserTrade {
    /// Converts this Binance trade to a Nautilus [`FillReport`].
    ///
    /// # Errors
    ///
    /// Returns an error if quantity or price parsing fails.
    pub fn to_fill_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        bnfcr_currency: Currency,
        ts_init: UnixNanos,
    ) -> anyhow::Result<FillReport> {
        let ts_event = UnixNanos::from_millis(self.time as u64);

        let venue_order_id = VenueOrderId::new(self.order_id.to_string());
        let trade_id = TradeId::new(self.id.to_string());

        let order_side = match self.side {
            BinanceSide::Buy => OrderSide::Buy,
            BinanceSide::Sell => OrderSide::Sell,
        };

        let liquidity_side = if self.maker {
            LiquiditySide::Maker
        } else {
            LiquiditySide::Taker
        };

        let last_qty: Decimal = self.qty.parse().context("invalid qty")?;
        let last_px: Decimal = self.price.parse().context("invalid price")?;

        let commission_currency = self
            .commission_asset
            .as_ref()
            .map_or(bnfcr_currency, |asset| {
                normalize_futures_asset(asset, bnfcr_currency)
            });
        let commission = match self.commission.as_ref() {
            Some(raw) => {
                let decimal = parse_required_decimal(raw, "commission")?;
                Money::from_decimal(decimal, commission_currency)?
            }
            None => Money::zero(commission_currency),
        };

        Ok(FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            trade_id,
            order_side,
            Quantity::from_decimal_dp(last_qty, size_precision).context("invalid qty precision")?,
            Price::from_decimal_dp(last_px, price_precision).context("invalid price precision")?,
            commission,
            liquidity_side,
            None, // client_order_id
            None, // venue_position_id
            ts_event,
            ts_init,
            Some(UUID4::new()),
        ))
    }
}

/// Result of a single order in a batch operation.
///
/// Each item in a batch response can be either a success or an error.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum BatchOrderResult {
    /// Successful order operation.
    Success(Box<BinanceFuturesOrder>),
    /// Failed order operation.
    Error(BatchOrderError),
}

/// Error in a batch order response.
#[derive(Clone, Debug, Deserialize)]
pub struct BatchOrderError {
    /// Error code from Binance.
    pub code: i64,
    /// Error message.
    pub msg: String,
}

/// Listen key response from user data stream endpoints.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for WebSocket user data stream.
    pub listen_key: String,
}

/// Algo order response from Binance Futures Algo Service API.
///
/// Algo orders are conditional orders (STOP_MARKET, STOP_LIMIT, TAKE_PROFIT,
/// TAKE_PROFIT_MARKET, TRAILING_STOP_MARKET) that are managed by Binance's
/// Algo Service rather than the traditional order matching engine.
///
/// # References
///
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/New-Algo-Order>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesAlgoOrder {
    /// Unique algo order ID assigned by Binance.
    pub algo_id: i64,
    /// Client-specified algo order ID for idempotency.
    pub client_algo_id: String,
    /// Algo type (currently only `Conditional` is supported).
    pub algo_type: BinanceAlgoType,
    /// Order type (STOP_MARKET, STOP, TAKE_PROFIT, TAKE_PROFIT_MARKET, TRAILING_STOP_MARKET).
    #[serde(rename = "orderType", alias = "type")]
    pub order_type: BinanceFuturesOrderType,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Order side (BUY/SELL).
    pub side: BinanceSide,
    /// Position side (BOTH, LONG, SHORT).
    #[serde(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Time in force.
    #[serde(default)]
    pub time_in_force: Option<BinanceTimeInForce>,
    /// Order quantity.
    #[serde(default)]
    pub quantity: Option<String>,
    /// Algo order status.
    #[serde(default)]
    pub algo_status: Option<BinanceAlgoStatus>,
    /// Trigger price for the conditional order.
    #[serde(default)]
    pub trigger_price: Option<String>,
    /// Limit price (for STOP/TAKE_PROFIT limit orders).
    #[serde(default)]
    pub price: Option<String>,
    /// Working type for trigger price calculation (CONTRACT_PRICE or MARK_PRICE).
    #[serde(default)]
    pub working_type: Option<BinanceWorkingType>,
    /// Close all position flag.
    #[serde(default)]
    pub close_position: Option<bool>,
    /// Price protection enabled.
    #[serde(default)]
    pub price_protect: Option<bool>,
    /// Reduce-only flag.
    #[serde(default)]
    pub reduce_only: Option<bool>,
    /// Activation price for TRAILING_STOP_MARKET orders.
    #[serde(default)]
    pub activate_price: Option<String>,
    /// Callback rate for TRAILING_STOP_MARKET orders (0.1 to 10, where 1 = 1%).
    #[serde(default)]
    pub callback_rate: Option<String>,
    /// Order creation time in milliseconds.
    #[serde(default)]
    pub create_time: Option<i64>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Trigger time in milliseconds (when the algo order triggered).
    #[serde(default)]
    pub trigger_time: Option<i64>,
    /// Order ID in matching engine (populated when algo order is triggered).
    #[serde(default)]
    pub actual_order_id: Option<String>,
    /// Executed quantity in matching engine (populated when algo order is triggered).
    #[serde(default)]
    pub executed_qty: Option<String>,
    /// Average fill price in matching engine (populated when algo order is triggered).
    #[serde(default)]
    pub avg_price: Option<String>,
}

impl BinanceFuturesAlgoOrder {
    /// Converts this Binance algo order to a Nautilus [`OrderStatusReport`].
    ///
    /// # Errors
    ///
    /// Returns an error if quantity, price, trigger, or trailing fields cannot be parsed.
    pub fn to_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> anyhow::Result<OrderStatusReport> {
        let ts_event = self
            .update_time
            .or(self.create_time)
            .map_or(ts_init, |t| UnixNanos::from_millis(t as u64));

        let client_order_id = ClientOrderId::new(decode_broker_id(
            &self.client_algo_id,
            BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        ));
        let venue_order_id = self
            .actual_order_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .map_or_else(
                || VenueOrderId::new(self.algo_id.to_string()),
                |id| VenueOrderId::new(id.clone()),
            );

        let order_side = match self.side {
            BinanceSide::Buy => OrderSide::Buy,
            BinanceSide::Sell => OrderSide::Sell,
        };

        let order_type = self.parse_order_type();
        let time_in_force = self
            .time_in_force
            .as_ref()
            .map_or(TimeInForce::Gtc, |tif| tif.to_nautilus_time_in_force());
        let order_status = self.parse_order_status();

        let quantity: Decimal = self
            .quantity
            .as_ref()
            .map_or(Ok(Decimal::ZERO), |q| q.parse())
            .context("invalid quantity")?;
        let filled_qty: Decimal = self
            .executed_qty
            .as_ref()
            .map_or(Ok(Decimal::ZERO), |q| q.parse())
            .context("invalid executed_qty")?;
        let price = if let Some(price) = self.price.as_ref().filter(|price| !price.is_empty()) {
            let price: Decimal = price.parse().context("invalid price")?;
            if price == Decimal::ZERO {
                None
            } else {
                Some(
                    Price::from_decimal_dp(price, price_precision)
                        .context("invalid price precision")?,
                )
            }
        } else {
            None
        };
        let trigger_price = self.parse_trigger_price(price_precision)?;
        let trailing_offset = self.parse_trailing_offset()?;

        let mut report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            Quantity::from_decimal_dp(quantity, size_precision)
                .context("invalid quantity precision")?,
            Quantity::from_decimal_dp(filled_qty, size_precision)
                .context("invalid executed_qty precision")?,
            ts_event,
            ts_event,
            ts_init,
            Some(UUID4::new()),
        );

        if let Some(price) = price {
            report = report.with_price(price);
        }

        if let Some(trigger_price) = trigger_price {
            report = report
                .with_trigger_price(trigger_price)
                .with_trigger_type(parse_working_type(self.working_type));
        }

        if let Some(trailing_offset) = trailing_offset {
            report = report
                .with_trailing_offset(trailing_offset)
                .with_trailing_offset_type(TrailingOffsetType::BasisPoints);
        }

        Ok(report)
    }

    fn parse_trigger_price(&self, price_precision: u8) -> anyhow::Result<Option<Price>> {
        let raw_trigger_price = match self.order_type {
            BinanceFuturesOrderType::TrailingStopMarket => self
                .trigger_price
                .as_deref()
                .or(self.activate_price.as_deref()),
            _ => self.trigger_price.as_deref(),
        };
        let trigger_price = raw_trigger_price
            .map(|price| parse_positive_price_at_precision(price, price_precision, "trigger_price"))
            .transpose()?
            .flatten();

        if trigger_price.is_none() && requires_algo_trigger_price(self.order_type) {
            anyhow::bail!(
                "missing positive trigger_price for Binance algo order type {:?}",
                self.order_type
            );
        }

        Ok(trigger_price)
    }

    fn parse_trailing_offset(&self) -> anyhow::Result<Option<Decimal>> {
        if self.order_type != BinanceFuturesOrderType::TrailingStopMarket {
            return Ok(None);
        }

        self.callback_rate
            .as_deref()
            .map(parse_callback_rate_basis_points)
            .transpose()
            .map(Option::flatten)
    }

    fn parse_order_type(&self) -> OrderType {
        self.order_type.into()
    }

    fn parse_order_status(&self) -> OrderStatus {
        match self.algo_status {
            Some(BinanceAlgoStatus::New) => OrderStatus::Accepted,
            Some(BinanceAlgoStatus::Triggering) => OrderStatus::Accepted,
            Some(BinanceAlgoStatus::Triggered) => OrderStatus::Accepted,
            Some(BinanceAlgoStatus::Finished) => {
                // Check executed_qty to determine if filled or canceled
                if let Some(qty) = &self.executed_qty
                    && let Ok(dec) = qty.parse::<Decimal>()
                    && !dec.is_zero()
                {
                    return OrderStatus::Filled;
                }
                OrderStatus::Canceled
            }
            Some(BinanceAlgoStatus::Canceled) => OrderStatus::Canceled,
            Some(BinanceAlgoStatus::Expired) => OrderStatus::Expired,
            Some(BinanceAlgoStatus::Rejected) => OrderStatus::Rejected,
            Some(BinanceAlgoStatus::Unknown) | None => OrderStatus::Initialized,
        }
    }
}

fn parse_positive_price_at_precision(
    raw: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Option<Price>> {
    let decimal = parse_required_decimal(raw, field)?;
    if decimal <= Decimal::ZERO {
        return Ok(None);
    }

    Price::from_decimal_dp(decimal, precision)
        .map(Some)
        .map_err(|e| anyhow::anyhow!("invalid {field} precision: {e}"))
}

fn parse_callback_rate_basis_points(raw: &str) -> anyhow::Result<Option<Decimal>> {
    let rate = parse_required_decimal(raw, "callback_rate")?;
    if rate <= Decimal::ZERO {
        return Ok(None);
    }

    rate.checked_mul(Decimal::from(100))
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("invalid callback_rate='{raw}': multiplication overflow"))
}

fn parse_working_type(working_type: Option<BinanceWorkingType>) -> TriggerType {
    match working_type {
        Some(BinanceWorkingType::ContractPrice) => TriggerType::LastPrice,
        Some(BinanceWorkingType::MarkPrice) => TriggerType::MarkPrice,
        Some(BinanceWorkingType::Unknown) | None => TriggerType::Default,
    }
}

fn requires_algo_trigger_price(order_type: BinanceFuturesOrderType) -> bool {
    matches!(
        order_type,
        BinanceFuturesOrderType::Stop
            | BinanceFuturesOrderType::StopMarket
            | BinanceFuturesOrderType::TakeProfit
            | BinanceFuturesOrderType::TakeProfitMarket
            | BinanceFuturesOrderType::TrailingStopMarket
    )
}

/// Cancel response for algo orders from Binance Futures Algo Service API.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFuturesAlgoOrderCancelResponse {
    /// Algo order ID that was canceled.
    pub algo_id: i64,
    /// Client algo order ID.
    pub client_algo_id: String,
    /// Response code (200 for success).
    pub code: String,
    /// Response message.
    pub msg: String,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_fixture_string;

    #[rstest]
    fn test_parse_account_info_v2() {
        let json = load_fixture_string("futures/http_json/account_info_v2.json");
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(&json).expect("Failed to parse account info");

        assert_eq!(
            account.total_wallet_balance,
            Some(Decimal::from_str_exact("23.72469206").unwrap())
        );
        assert_eq!(account.assets.len(), 1);
        assert_eq!(account.assets[0].asset.as_str(), "USDT");
        assert_eq!(
            account.assets[0].wallet_balance,
            Decimal::from_str_exact("23.72469206").unwrap()
        );
        assert_eq!(account.positions.len(), 1);
        assert_eq!(account.positions[0].symbol.as_str(), "BTCUSDT");
        assert_eq!(account.positions[0].leverage, Some("100".to_string()));
    }

    #[rstest]
    fn test_account_info_to_account_state_zero_margins() {
        let json = load_fixture_string("futures/http_json/account_info_v2.json");
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(&json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.account_id, account_id);
        assert_eq!(state.account_type, AccountType::Margin);
        assert!(!state.balances.is_empty());
        assert_eq!(state.margins.len(), 0);
    }

    #[rstest]
    fn test_account_info_to_account_state_with_margins() {
        let json = r#"{
            "totalInitialMargin": "500.25000000",
            "totalMaintMargin": "250.75000000",
            "totalWalletBalance": "10000.00000000",
            "assets": [{
                "asset": "USDT",
                "walletBalance": "10000.00000000",
                "availableBalance": "9500.00000000",
                "initialMargin": "500.25000000",
                "maintMargin": "250.75000000",
                "updateTime": 1617939110373
            }],
            "positions": []
        }"#;
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.margins.len(), 1);
        let margin = &state.margins[0];
        assert!(margin.instrument_id.is_none());
        assert_eq!(margin.currency.code.as_str(), "USDT");
        assert_eq!(margin.initial.as_f64(), 500.25);
        assert_eq!(margin.maintenance.as_f64(), 250.75);
    }

    #[rstest]
    fn test_account_info_to_account_state_coin_margined_per_base_coin() {
        let json = r#"{
            "totalWalletBalance": "0.00000000",
            "assets": [
                {
                    "asset": "BTC",
                    "walletBalance": "1.50000000",
                    "availableBalance": "1.40000000",
                    "initialMargin": "0.05000000",
                    "maintMargin": "0.02500000",
                    "updateTime": 1617939110373
                },
                {
                    "asset": "ETH",
                    "walletBalance": "10.00000000",
                    "availableBalance": "9.00000000",
                    "initialMargin": "0.80000000",
                    "maintMargin": "0.40000000",
                    "updateTime": 1617939110373
                }
            ],
            "positions": []
        }"#;
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.margins.len(), 2);
        assert!(state.margins.iter().all(|m| m.instrument_id.is_none()));
        let btc = state
            .margins
            .iter()
            .find(|m| m.currency.code.as_str() == "BTC")
            .expect("BTC margin missing");
        assert_eq!(btc.initial.as_f64(), 0.05);
        assert_eq!(btc.maintenance.as_f64(), 0.025);
        let eth = state
            .margins
            .iter()
            .find(|m| m.currency.code.as_str() == "ETH")
            .expect("ETH margin missing");
        assert_eq!(eth.initial.as_f64(), 0.8);
        assert_eq!(eth.maintenance.as_f64(), 0.4);
    }

    // Regression for the #3867 bug class: wire values with more decimal places
    // than the currency precision (USDT=8) previously tripped the
    // `total == locked + free` invariant when Money::new rounded each side
    // independently. The `from_total_and_free` helper must keep the invariant.
    #[rstest]
    fn test_account_info_to_account_state_precision_drift() {
        let json = r#"{
            "assets": [{
                "asset": "USDT",
                "walletBalance": "10.000000034999",
                "availableBalance": "9.999999994999",
                "updateTime": 1617939110373
            }],
            "positions": []
        }"#;
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.balances.len(), 1);
        let balance = &state.balances[0];
        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    fn test_account_info_to_account_state_empty_balance() {
        // Empty strings for balance fields (inactive/zero-balance accounts)
        let json = r#"{
            "assets": [{
                "asset": "USDT",
                "walletBalance": "",
                "availableBalance": "",
                "updateTime": 0
            }],
            "positions": []
        }"#;
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.balances.len(), 1);
        let balance = &state.balances[0];
        assert_eq!(balance.total, Money::new(0.0, Currency::USDT()));
        assert_eq!(balance.free, Money::new(0.0, Currency::USDT()));
        assert_eq!(balance.locked, Money::new(0.0, Currency::USDT()));
    }

    #[rstest]
    fn test_account_info_to_account_state_empty_assets() {
        // No assets at all (completely empty account)
        let json = r#"{
            "assets": [],
            "positions": []
        }"#;
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(json).expect("Failed to parse account info");

        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = account.to_account_state(account_id, ts_init).unwrap();

        assert_eq!(state.balances.len(), 1);
        let balance = &state.balances[0];
        assert_eq!(balance.total, Money::new(0.0, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_position_risk() {
        let json = load_fixture_string("futures/http_json/position_risk.json");
        let positions: Vec<BinancePositionRisk> =
            serde_json::from_str(&json).expect("Failed to parse position risk");

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol.as_str(), "BTCUSDT");
        assert_eq!(positions[0].position_amt, "0.001");
        assert_eq!(positions[0].mark_price, "51000.0");
        assert_eq!(positions[0].leverage, "20");
    }

    #[rstest]
    fn test_parse_balance_with_v1_field() {
        // V1 uses 'balance' field
        let json = load_fixture_string("futures/http_json/balance.json");
        let balances: Vec<BinanceFuturesBalance> =
            serde_json::from_str(&json).expect("Failed to parse balance");

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].asset.as_str(), "USDT");
        // Uses alias to parse 'balance' into wallet_balance
        assert_eq!(
            balances[0].wallet_balance,
            Decimal::from_str_exact("122.12345678").unwrap()
        );
        assert_eq!(
            balances[0].available_balance,
            Decimal::from_str_exact("122.12345678").unwrap()
        );
    }

    #[rstest]
    fn test_parse_balance_with_v2_field() {
        // V2 uses 'walletBalance' field
        let json = r#"{
            "asset": "USDT",
            "walletBalance": "100.00000000",
            "availableBalance": "100.00000000",
            "updateTime": 1617939110373
        }"#;

        let balance: BinanceFuturesBalance =
            serde_json::from_str(json).expect("Failed to parse balance");

        assert_eq!(balance.asset.as_str(), "USDT");
        assert_eq!(
            balance.wallet_balance,
            Decimal::from_str_exact("100.00000000").unwrap()
        );
    }

    #[rstest]
    fn test_parse_order() {
        let json = load_fixture_string("futures/http_json/order_response.json");
        let order: BinanceFuturesOrder =
            serde_json::from_str(&json).expect("Failed to parse order");

        assert_eq!(order.order_id, 12345678);
        assert_eq!(order.symbol.as_str(), "BTCUSDT");
        assert_eq!(order.status, BinanceOrderStatus::New);
        assert_eq!(order.time_in_force, BinanceTimeInForce::Gtc);
        assert_eq!(order.side, BinanceSide::Buy);
        assert_eq!(order.order_type, BinanceFuturesOrderType::Limit);
        assert_eq!(order.price_match, Some(BinancePriceMatch::None));
        assert_eq!(
            order.self_trade_prevention_mode,
            Some(BinanceSelfTradePreventionMode::None)
        );
    }

    #[rstest]
    fn test_parse_order_defaults_missing_cum_quote_to_zero() {
        let json = load_fixture_string("futures/http_json/order_response.json");
        let mut value: Value = serde_json::from_str(&json).expect("Failed to parse order fixture");

        value
            .as_object_mut()
            .expect("Order fixture should be a JSON object")
            .remove("cumQuote");

        let order: BinanceFuturesOrder =
            serde_json::from_value(value).expect("Failed to parse order");

        assert_eq!(order.cum_quote, "0");
    }

    #[rstest]
    fn test_parse_kline_rejects_non_string_price() {
        let value = serde_json::json!([
            1_625_474_304_000_i64,
            50000.00,
            "51000.00",
            "49000.00",
            "50500.00",
            "12.5",
            1_625_474_364_000_i64,
            "631250.00",
            100_i64,
            "6.2",
            "313100.00"
        ]);

        let error = serde_json::from_value::<BinanceFuturesKline>(value)
            .unwrap_err()
            .to_string();

        assert!(error.contains("open"));
    }

    #[rstest]
    fn test_parse_hedge_mode_response() {
        let json = r#"{"dualSidePosition": true}"#;
        let response: BinanceHedgeModeResponse =
            serde_json::from_str(json).expect("Failed to parse hedge mode");
        assert!(response.dual_side_position);
    }

    #[rstest]
    fn test_parse_leverage_response() {
        let json = r#"{"symbol": "BTCUSDT", "leverage": 20, "maxNotionalValue": "250000"}"#;
        let response: BinanceLeverageResponse =
            serde_json::from_str(json).expect("Failed to parse leverage");
        assert_eq!(response.symbol.as_str(), "BTCUSDT");
        assert_eq!(response.leverage, 20);
    }

    #[rstest]
    fn test_parse_listen_key_response() {
        let json =
            r#"{"listenKey": "pqia91ma19a5s61cv6a81va65sdf19v8a65a1a5s61cv6a81va65sdf19v8a65a1"}"#;
        let response: ListenKeyResponse =
            serde_json::from_str(json).expect("Failed to parse listen key");
        assert!(!response.listen_key.is_empty());
    }

    #[rstest]
    fn test_parse_account_position() {
        let json = r#"{
            "symbol": "ETHUSDT",
            "initialMargin": "100.00",
            "maintMargin": "50.00",
            "unrealizedProfit": "10.00",
            "positionInitialMargin": "100.00",
            "openOrderInitialMargin": "0",
            "leverage": "10",
            "isolated": true,
            "entryPrice": "2000.00",
            "maxNotional": "100000",
            "bidNotional": "0",
            "askNotional": "0",
            "positionSide": "LONG",
            "positionAmt": "0.5",
            "updateTime": 1625474304765
        }"#;

        let position: BinanceAccountPosition =
            serde_json::from_str(json).expect("Failed to parse account position");

        assert_eq!(position.symbol.as_str(), "ETHUSDT");
        assert_eq!(position.leverage, Some("10".to_string()));
        assert_eq!(position.isolated, Some(true));
        assert_eq!(position.position_side, Some(BinancePositionSide::Long));
    }

    #[rstest]
    fn test_parse_algo_order() {
        let json = r#"{
            "algoId": 123456789,
            "clientAlgoId": "test-algo-order-1",
            "algoType": "CONDITIONAL",
            "type": "STOP_MARKET",
            "symbol": "BTCUSDT",
            "side": "BUY",
            "positionSide": "BOTH",
            "timeInForce": "GTC",
            "quantity": "0.001",
            "algoStatus": "NEW",
            "triggerPrice": "45000.00",
            "workingType": "MARK_PRICE",
            "reduceOnly": false,
            "createTime": 1625474304765,
            "updateTime": 1625474304765
        }"#;

        let order: BinanceFuturesAlgoOrder =
            serde_json::from_str(json).expect("Failed to parse algo order");

        assert_eq!(order.algo_id, 123456789);
        assert_eq!(order.client_algo_id, "test-algo-order-1");
        assert_eq!(order.algo_type, BinanceAlgoType::Conditional);
        assert_eq!(order.order_type, BinanceFuturesOrderType::StopMarket);
        assert_eq!(order.symbol.as_str(), "BTCUSDT");
        assert_eq!(order.side, BinanceSide::Buy);
        assert_eq!(order.algo_status, Some(BinanceAlgoStatus::New));
        assert_eq!(order.trigger_price, Some("45000.00".to_string()));
    }

    #[rstest]
    fn test_parse_algo_order_triggered() {
        let json = r#"{
            "algoId": 123456789,
            "clientAlgoId": "test-algo-order-2",
            "algoType": "CONDITIONAL",
            "type": "TAKE_PROFIT",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "algoStatus": "TRIGGERED",
            "triggerPrice": "2500.00",
            "price": "2500.00",
            "actualOrderId": "987654321",
            "executedQty": "0.5",
            "avgPrice": "2499.50"
        }"#;

        let order: BinanceFuturesAlgoOrder =
            serde_json::from_str(json).expect("Failed to parse triggered algo order");

        assert_eq!(order.algo_status, Some(BinanceAlgoStatus::Triggered));
        assert_eq!(order.order_type, BinanceFuturesOrderType::TakeProfit);
        assert_eq!(order.actual_order_id, Some("987654321".to_string()));
        assert_eq!(order.executed_qty, Some("0.5".to_string()));
    }

    #[rstest]
    fn test_parse_algo_order_cancel_response() {
        let json = r#"{
            "algoId": 123456789,
            "clientAlgoId": "test-algo-order-1",
            "code": "200",
            "msg": "success"
        }"#;

        let response: BinanceFuturesAlgoOrderCancelResponse =
            serde_json::from_str(json).expect("Failed to parse algo cancel response");

        assert_eq!(response.algo_id, 123456789);
        assert_eq!(response.client_algo_id, "test-algo-order-1");
        assert_eq!(response.code, "200");
        assert_eq!(response.msg, "success");
    }

    #[rstest]
    fn test_order_to_report_decodes_broker_id() {
        let json = r#"{
            "orderId": 12345678,
            "symbol": "BTCUSDT",
            "status": "NEW",
            "clientOrderId": "x-aHRE4BCj-T0000000000000",
            "price": "50000.00",
            "avgPrice": "0.00",
            "origQty": "0.001",
            "executedQty": "0.000",
            "cumQuote": "0.00",
            "timeInForce": "GTC",
            "type": "LIMIT",
            "reduceOnly": false,
            "closePosition": false,
            "side": "BUY",
            "positionSide": "BOTH",
            "stopPrice": "0.00",
            "workingType": "CONTRACT_PRICE",
            "priceProtect": false,
            "origType": "LIMIT",
            "priceMatch": "NONE",
            "selfTradePreventionMode": "NONE",
            "goodTillDate": 0,
            "time": 1625474304765,
            "updateTime": 1625474304765
        }"#;

        let order: BinanceFuturesOrder = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, false, ts_init)
            .unwrap();

        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20200101-000000-000-000-0")),
        );
        assert_eq!(report.price, Some(Price::from("50000.00")));
    }

    #[rstest]
    #[case("0")]
    #[case("")]
    fn test_order_to_report_omits_missing_price(#[case] price: &str) {
        let order = order_with_price(price);
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, false, ts_init)
            .unwrap();

        assert_eq!(report.price, None);
    }

    #[rstest]
    fn test_algo_order_to_report_sets_price() {
        let order = algo_order_with_price(Some("50000.00"));
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(report.price, Some(Price::from("50000.00")));
    }

    #[rstest]
    fn test_algo_order_to_report_sets_trigger_fields() {
        let order = algo_order_with_price(Some("44000.00"));
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(report.trigger_price, Some(Price::from("45000.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::MarkPrice));
    }

    #[rstest]
    fn test_algo_order_to_report_sets_trailing_fields() {
        let mut order = algo_order_with_price(None);
        order.order_type = BinanceFuturesOrderType::TrailingStopMarket;
        order.trigger_price = None;
        order.activate_price = Some("45000.00".to_string());
        order.callback_rate = Some("0.25".to_string());
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(report.trigger_price, Some(Price::from("45000.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::MarkPrice));
        assert_eq!(report.trailing_offset, Some(Decimal::from(25)));
        assert_eq!(report.trailing_offset_type, TrailingOffsetType::BasisPoints);
    }

    #[rstest]
    #[case(None)]
    #[case(Some("0"))]
    #[case(Some(""))]
    fn test_algo_order_to_report_omits_missing_price(#[case] price: Option<&str>) {
        let order = algo_order_with_price(price);
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(report.price, None);
    }

    #[rstest]
    fn test_order_to_report_rejects_invalid_price() {
        let order = order_with_price("not-a-number");
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = order.to_order_status_report(account_id, instrument_id, 2, 3, false, ts_init);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("invalid price"));
    }

    #[rstest]
    fn test_algo_order_to_report_rejects_invalid_trigger_price() {
        let mut order = algo_order_with_price(Some("50000.00"));
        order.trigger_price = Some("not-a-number".to_string());
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = order.to_order_status_report(account_id, instrument_id, 2, 3, ts_init);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("trigger_price"));
    }

    #[rstest]
    fn test_algo_order_to_report_rejects_missing_trigger_price() {
        let mut order = algo_order_with_price(None);
        order.order_type = BinanceFuturesOrderType::StopMarket;
        order.trigger_price = None;
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = order.to_order_status_report(account_id, instrument_id, 2, 3, ts_init);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("missing positive trigger_price"));
    }

    #[rstest]
    fn test_algo_order_to_report_rejects_invalid_callback_rate() {
        let mut order = algo_order_with_price(None);
        order.order_type = BinanceFuturesOrderType::TrailingStopMarket;
        order.trigger_price = None;
        order.activate_price = Some("45000.00".to_string());
        order.callback_rate = Some("not-a-number".to_string());
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = order.to_order_status_report(account_id, instrument_id, 2, 3, ts_init);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("callback_rate"));
    }

    #[rstest]
    fn test_algo_order_to_report_rejects_invalid_price() {
        let order = algo_order_with_price(Some("not-a-number"));
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = order.to_order_status_report(account_id, instrument_id, 2, 3, ts_init);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("invalid price"));
    }

    fn order_with_price(price: &str) -> BinanceFuturesOrder {
        BinanceFuturesOrder {
            symbol: Ustr::from("BTCUSDT"),
            order_id: 12345678,
            client_order_id: "external-order".to_string(),
            orig_qty: "0.001".to_string(),
            executed_qty: "0.000".to_string(),
            cum_quote: "0.00".to_string(),
            price: price.to_string(),
            avg_price: Some("0.00".to_string()),
            stop_price: Some("0.00".to_string()),
            status: BinanceOrderStatus::New,
            time_in_force: BinanceTimeInForce::Gtc,
            order_type: BinanceFuturesOrderType::Market,
            orig_type: Some(BinanceFuturesOrderType::Market),
            side: BinanceSide::Buy,
            position_side: Some(BinancePositionSide::Both),
            reduce_only: Some(false),
            close_position: Some(false),
            activate_price: None,
            price_rate: None,
            working_type: Some(BinanceWorkingType::ContractPrice),
            price_protect: Some(false),
            is_isolated: None,
            good_till_date: Some(0),
            price_match: Some(BinancePriceMatch::None),
            self_trade_prevention_mode: Some(BinanceSelfTradePreventionMode::None),
            update_time: Some(1_625_474_304_765),
            working_type_id: None,
        }
    }

    fn algo_order_with_price(price: Option<&str>) -> BinanceFuturesAlgoOrder {
        BinanceFuturesAlgoOrder {
            algo_id: 123456789,
            client_algo_id: "x-aHRE4BCj-Rmy-algo-order-1".to_string(),
            algo_type: BinanceAlgoType::Conditional,
            order_type: BinanceFuturesOrderType::TakeProfit,
            symbol: Ustr::from("BTCUSDT"),
            side: BinanceSide::Sell,
            position_side: Some(BinancePositionSide::Both),
            time_in_force: Some(BinanceTimeInForce::Gtc),
            quantity: Some("0.001".to_string()),
            algo_status: Some(BinanceAlgoStatus::New),
            trigger_price: Some("45000.00".to_string()),
            price: price.map(str::to_string),
            working_type: Some(BinanceWorkingType::MarkPrice),
            close_position: Some(false),
            price_protect: None,
            reduce_only: Some(false),
            activate_price: None,
            callback_rate: None,
            create_time: Some(1_625_474_304_765),
            update_time: Some(1_625_474_304_765),
            trigger_time: None,
            actual_order_id: None,
            executed_qty: None,
            avg_price: None,
        }
    }

    #[rstest]
    fn test_user_trade_to_fill_report_rejects_invalid_commission() {
        let trade = BinanceUserTrade {
            symbol: Ustr::from("BTCUSDT"),
            id: 100,
            order_id: 200,
            price: "50000.00".to_string(),
            qty: "0.001".to_string(),
            quote_qty: None,
            realized_pnl: "0".to_string(),
            side: BinanceSide::Buy,
            position_side: None,
            time: 1_625_474_304_000,
            buyer: true,
            maker: false,
            commission: Some("not-a-number".to_string()),
            commission_asset: Some(Ustr::from("USDT")),
            margin_asset: None,
        };

        let result = trade.to_fill_report(
            AccountId::from("BINANCE-FUTURES-001"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            2,
            3,
            Currency::USDT(),
            UnixNanos::from(1_000_000_000u64),
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("commission"));
    }

    #[rstest]
    fn test_algo_order_to_report_decodes_broker_id() {
        let json = r#"{
            "algoId": 123456789,
            "clientAlgoId": "x-aHRE4BCj-Rmy-algo-order-1",
            "algoType": "CONDITIONAL",
            "type": "STOP_MARKET",
            "symbol": "BTCUSDT",
            "side": "BUY",
            "positionSide": "BOTH",
            "timeInForce": "GTC",
            "quantity": "0.001",
            "algoStatus": "NEW",
            "triggerPrice": "45000.00",
            "workingType": "MARK_PRICE",
            "reduceOnly": false,
            "createTime": 1625474304765,
            "updateTime": 1625474304765
        }"#;

        let order: BinanceFuturesAlgoOrder = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("my-algo-order-1")),
        );
    }

    #[rstest]
    #[case(None, "123456789")]
    #[case(Some(""), "123456789")]
    #[case(Some("987654321"), "987654321")]
    fn test_algo_order_to_report_selects_valid_venue_order_id(
        #[case] actual_order_id: Option<&str>,
        #[case] expected_venue_order_id: &str,
    ) {
        let order = BinanceFuturesAlgoOrder {
            algo_id: 123456789,
            client_algo_id: "x-aHRE4BCj-Rmy-algo-order-1".to_string(),
            algo_type: BinanceAlgoType::Conditional,
            order_type: BinanceFuturesOrderType::StopMarket,
            symbol: Ustr::from("BTCUSDT"),
            side: BinanceSide::Buy,
            position_side: Some(BinancePositionSide::Both),
            time_in_force: Some(BinanceTimeInForce::Gtc),
            quantity: Some("0.001".to_string()),
            algo_status: Some(BinanceAlgoStatus::New),
            trigger_price: Some("45000.00".to_string()),
            price: None,
            working_type: Some(BinanceWorkingType::MarkPrice),
            close_position: Some(false),
            price_protect: None,
            reduce_only: Some(false),
            activate_price: None,
            callback_rate: None,
            create_time: Some(1_625_474_304_765),
            update_time: Some(1_625_474_304_765),
            trigger_time: None,
            actual_order_id: actual_order_id.map(str::to_string),
            executed_qty: None,
            avg_price: None,
        };
        let account_id = AccountId::from("BINANCE-FUTURES-001");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = order
            .to_order_status_report(account_id, instrument_id, 2, 3, ts_init)
            .unwrap();

        assert_eq!(
            report.venue_order_id,
            VenueOrderId::new(expected_venue_order_id)
        );
    }

    #[rstest]
    #[case(BinanceOrderStatus::Expired, false, OrderStatus::Expired)]
    #[case(BinanceOrderStatus::Expired, true, OrderStatus::Canceled)]
    #[case(BinanceOrderStatus::ExpiredInMatch, false, OrderStatus::Expired)]
    #[case(BinanceOrderStatus::ExpiredInMatch, true, OrderStatus::Canceled)]
    fn test_to_nautilus_order_status_expired_respects_treat_as_canceled(
        #[case] status: BinanceOrderStatus,
        #[case] treat_expired_as_canceled: bool,
        #[case] expected: OrderStatus,
    ) {
        let result = status.to_nautilus_order_status(treat_expired_as_canceled);
        assert_eq!(result, expected);
    }
}

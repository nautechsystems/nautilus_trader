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
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, Venue, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
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
            open_time: arr[0].as_i64().unwrap_or(0),
            open: arr[1].as_str().unwrap_or("0").to_string(),
            high: arr[2].as_str().unwrap_or("0").to_string(),
            low: arr[3].as_str().unwrap_or("0").to_string(),
            close: arr[4].as_str().unwrap_or("0").to_string(),
            volume: arr[5].as_str().unwrap_or("0").to_string(),
            close_time: arr[6].as_i64().unwrap_or(0),
            quote_volume: arr[7].as_str().unwrap_or("0").to_string(),
            num_trades: arr[8].as_i64().unwrap_or(0),
            taker_buy_base_volume: arr[9].as_str().unwrap_or("0").to_string(),
            taker_buy_quote_volume: arr[10].as_str().unwrap_or("0").to_string(),
        })
    }
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
    /// Wallet balance (v2 uses walletBalance, v1 uses balance).
    #[serde(alias = "balance")]
    pub wallet_balance: String,
    /// Unrealized profit.
    #[serde(default)]
    pub unrealized_profit: Option<String>,
    /// Margin balance.
    #[serde(default)]
    pub margin_balance: Option<String>,
    /// Maintenance margin required.
    #[serde(default)]
    pub maint_margin: Option<String>,
    /// Initial margin required.
    #[serde(default)]
    pub initial_margin: Option<String>,
    /// Position initial margin.
    #[serde(default)]
    pub position_initial_margin: Option<String>,
    /// Open order initial margin.
    #[serde(default)]
    pub open_order_initial_margin: Option<String>,
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
    #[serde(default)]
    pub total_initial_margin: Option<String>,
    /// Total maintenance margin required.
    #[serde(default)]
    pub total_maint_margin: Option<String>,
    /// Total wallet balance.
    #[serde(default)]
    pub total_wallet_balance: Option<String>,
    /// Total unrealized profit.
    #[serde(default)]
    pub total_unrealized_profit: Option<String>,
    /// Total margin balance.
    #[serde(default)]
    pub total_margin_balance: Option<String>,
    /// Total position initial margin.
    #[serde(default)]
    pub total_position_initial_margin: Option<String>,
    /// Total open order initial margin.
    #[serde(default)]
    pub total_open_order_initial_margin: Option<String>,
    /// Total cross wallet balance.
    #[serde(default)]
    pub total_cross_wallet_balance: Option<String>,
    /// Total cross unrealized PnL.
    #[serde(default)]
    pub total_cross_un_pnl: Option<String>,
    /// Available balance.
    #[serde(default)]
    pub available_balance: Option<String>,
    /// Max withdraw amount.
    #[serde(default)]
    pub max_withdraw_amount: Option<String>,
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

            let total: Decimal = asset.wallet_balance.parse().context("invalid balance")?;
            let available: Decimal = asset
                .available_balance
                .parse()
                .context("invalid available_balance")?;
            let locked = total - available;

            let total_money = Money::from_decimal(total, currency)
                .unwrap_or_else(|_| Money::new(total.to_string().parse().unwrap_or(0.0), currency));
            let locked_money = Money::from_decimal(locked, currency).unwrap_or_else(|_| {
                Money::new(locked.to_string().parse().unwrap_or(0.0), currency)
            });
            let free_money = Money::from_decimal(available, currency).unwrap_or_else(|_| {
                Money::new(available.to_string().parse().unwrap_or(0.0), currency)
            });

            let balance = AccountBalance::new(total_money, locked_money, free_money);
            balances.push(balance);
        }

        // Ensure at least one balance exists
        if balances.is_empty() {
            let zero_currency = Currency::USDT();
            let zero_money = Money::new(0.0, zero_currency);
            let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
            balances.push(zero_balance);
        }

        // Parse margin requirements
        let mut margins = Vec::new();

        let initial_margin_dec = self
            .total_initial_margin
            .as_ref()
            .and_then(|s| Decimal::from_str_exact(s).ok());
        let maint_margin_dec = self
            .total_maint_margin
            .as_ref()
            .and_then(|s| Decimal::from_str_exact(s).ok());

        if let (Some(initial_margin_dec), Some(maint_margin_dec)) =
            (initial_margin_dec, maint_margin_dec)
        {
            let has_margin = !initial_margin_dec.is_zero() || !maint_margin_dec.is_zero();
            if has_margin {
                let margin_currency = Currency::USDT();
                let margin_instrument_id =
                    InstrumentId::new(Symbol::new("ACCOUNT"), Venue::new("BINANCE"));

                let initial_margin = Money::from_decimal(initial_margin_dec, margin_currency)
                    .unwrap_or_else(|_| Money::zero(margin_currency));
                let maintenance_margin = Money::from_decimal(maint_margin_dec, margin_currency)
                    .unwrap_or_else(|_| Money::zero(margin_currency));

                let margin_balance =
                    MarginBalance::new(initial_margin, maintenance_margin, margin_instrument_id);
                margins.push(margin_balance);
            }
        }

        let ts_event = self
            .update_time
            .map_or(ts_init, |t| UnixNanos::from((t * 1_000_000) as u64));

        Ok(AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            margins,
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

impl BinanceFuturesOrder {
    /// Converts this Binance order to a Nautilus [`OrderStatusReport`].
    ///
    /// # Errors
    ///
    /// Returns an error if quantity parsing fails.
    pub fn to_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        size_precision: u8,
    ) -> anyhow::Result<OrderStatusReport> {
        let ts_now = get_atomic_clock_realtime().get_time_ns();
        let ts_event = self
            .update_time
            .map_or(ts_now, |t| UnixNanos::from((t * 1_000_000) as u64));

        let client_order_id = ClientOrderId::new(&self.client_order_id);
        let venue_order_id = VenueOrderId::new(self.order_id.to_string());

        let order_side = match self.side {
            BinanceSide::Buy => OrderSide::Buy,
            BinanceSide::Sell => OrderSide::Sell,
        };

        let order_type = self.order_type.to_nautilus_order_type();
        let time_in_force = self.time_in_force.to_nautilus_time_in_force();
        let order_status = self.status.to_nautilus_order_status();

        let quantity: Decimal = self.orig_qty.parse().context("invalid orig_qty")?;
        let filled_qty: Decimal = self.executed_qty.parse().context("invalid executed_qty")?;

        Ok(OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            Quantity::new(quantity.to_string().parse()?, size_precision),
            Quantity::new(filled_qty.to_string().parse()?, size_precision),
            ts_event,
            ts_event,
            ts_now,
            Some(UUID4::new()),
        ))
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
            Self::Unknown => TimeInForce::Gtc, // default
        }
    }
}

impl BinanceOrderStatus {
    /// Converts to Nautilus order status.
    #[must_use]
    pub fn to_nautilus_order_status(&self) -> OrderStatus {
        match self {
            Self::New => OrderStatus::Accepted,
            Self::PartiallyFilled => OrderStatus::PartiallyFilled,
            Self::Filled => OrderStatus::Filled,
            Self::Canceled => OrderStatus::Canceled,
            Self::PendingCancel => OrderStatus::PendingCancel,
            Self::Rejected => OrderStatus::Rejected,
            Self::Expired => OrderStatus::Expired,
            Self::ExpiredInMatch => OrderStatus::Expired,
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
    ) -> anyhow::Result<FillReport> {
        let ts_now = get_atomic_clock_realtime().get_time_ns();
        let ts_event = UnixNanos::from((self.time * 1_000_000) as u64);

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

        let commission = {
            let comm_val: f64 = self
                .commission
                .as_ref()
                .and_then(|c| c.parse().ok())
                .unwrap_or(0.0);
            let comm_asset = self
                .commission_asset
                .as_ref()
                .map_or_else(Currency::USDT, Currency::from);
            Money::new(comm_val, comm_asset)
        };

        Ok(FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            trade_id,
            order_side,
            Quantity::new(last_qty.to_string().parse()?, size_precision),
            Price::new(last_px.to_string().parse()?, price_precision),
            commission,
            liquidity_side,
            None, // client_order_id
            None, // venue_position_id
            ts_event,
            ts_now,
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    /// Test fixture from Binance API docs for GET /fapi/v2/account
    const ACCOUNT_INFO_V2_JSON: &str = r#"{
        "feeTier": 0,
        "canTrade": true,
        "canDeposit": true,
        "canWithdraw": true,
        "updateTime": 0,
        "multiAssetsMargin": false,
        "tradeGroupId": -1,
        "totalInitialMargin": "0.00000000",
        "totalMaintMargin": "0.00000000",
        "totalWalletBalance": "23.72469206",
        "totalUnrealizedProfit": "0.00000000",
        "totalMarginBalance": "23.72469206",
        "totalPositionInitialMargin": "0.00000000",
        "totalOpenOrderInitialMargin": "0.00000000",
        "totalCrossWalletBalance": "23.72469206",
        "totalCrossUnPnl": "0.00000000",
        "availableBalance": "23.72469206",
        "maxWithdrawAmount": "23.72469206",
        "assets": [
            {
                "asset": "USDT",
                "walletBalance": "23.72469206",
                "unrealizedProfit": "0.00000000",
                "marginBalance": "23.72469206",
                "maintMargin": "0.00000000",
                "initialMargin": "0.00000000",
                "positionInitialMargin": "0.00000000",
                "openOrderInitialMargin": "0.00000000",
                "crossWalletBalance": "23.72469206",
                "crossUnPnl": "0.00000000",
                "availableBalance": "23.72469206",
                "maxWithdrawAmount": "23.72469206",
                "marginAvailable": true,
                "updateTime": 1625474304765
            }
        ],
        "positions": [
            {
                "symbol": "BTCUSDT",
                "initialMargin": "0",
                "maintMargin": "0",
                "unrealizedProfit": "0.00000000",
                "positionInitialMargin": "0",
                "openOrderInitialMargin": "0",
                "leverage": "100",
                "isolated": false,
                "entryPrice": "0.00000",
                "maxNotional": "250000",
                "bidNotional": "0",
                "askNotional": "0",
                "positionSide": "BOTH",
                "positionAmt": "0",
                "updateTime": 0
            }
        ]
    }"#;

    /// Test fixture for GET /fapi/v2/positionRisk
    const POSITION_RISK_JSON: &str = r#"[
        {
            "symbol": "BTCUSDT",
            "positionAmt": "0.001",
            "entryPrice": "50000.0",
            "markPrice": "51000.0",
            "unRealizedProfit": "1.00000000",
            "liquidationPrice": "45000.0",
            "leverage": "20",
            "maxNotionalValue": "250000",
            "marginType": "cross",
            "isolatedMargin": "0.00000000",
            "isAutoAddMargin": "false",
            "positionSide": "BOTH",
            "notional": "51.0",
            "isolatedWallet": "0",
            "updateTime": 1625474304765,
            "breakEvenPrice": "50100.0"
        }
    ]"#;

    /// Test fixture for balance endpoint
    const BALANCE_JSON: &str = r#"[
        {
            "accountAlias": "SgsR",
            "asset": "USDT",
            "balance": "122.12345678",
            "crossWalletBalance": "122.12345678",
            "crossUnPnl": "0.00000000",
            "availableBalance": "122.12345678",
            "maxWithdrawAmount": "122.12345678",
            "marginAvailable": true,
            "updateTime": 1617939110373
        }
    ]"#;

    /// Test fixture for order response
    const ORDER_JSON: &str = r#"{
        "orderId": 12345678,
        "symbol": "BTCUSDT",
        "status": "NEW",
        "clientOrderId": "testOrder123",
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

    #[rstest]
    fn test_parse_account_info_v2() {
        let account: BinanceFuturesAccountInfo =
            serde_json::from_str(ACCOUNT_INFO_V2_JSON).expect("Failed to parse account info");

        assert_eq!(
            account.total_wallet_balance,
            Some("23.72469206".to_string())
        );
        assert_eq!(account.assets.len(), 1);
        assert_eq!(account.assets[0].asset.as_str(), "USDT");
        assert_eq!(account.assets[0].wallet_balance, "23.72469206");
        assert_eq!(account.positions.len(), 1);
        assert_eq!(account.positions[0].symbol.as_str(), "BTCUSDT");
        assert_eq!(account.positions[0].leverage, Some("100".to_string()));
    }

    #[rstest]
    fn test_parse_position_risk() {
        let positions: Vec<BinancePositionRisk> =
            serde_json::from_str(POSITION_RISK_JSON).expect("Failed to parse position risk");

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol.as_str(), "BTCUSDT");
        assert_eq!(positions[0].position_amt, "0.001");
        assert_eq!(positions[0].mark_price, "51000.0");
        assert_eq!(positions[0].leverage, "20");
    }

    #[rstest]
    fn test_parse_balance_with_v1_field() {
        // V1 uses 'balance' field
        let balances: Vec<BinanceFuturesBalance> =
            serde_json::from_str(BALANCE_JSON).expect("Failed to parse balance");

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].asset.as_str(), "USDT");
        // Uses alias to parse 'balance' into wallet_balance
        assert_eq!(balances[0].wallet_balance, "122.12345678");
        assert_eq!(balances[0].available_balance, "122.12345678");
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
        assert_eq!(balance.wallet_balance, "100.00000000");
    }

    #[rstest]
    fn test_parse_order() {
        let order: BinanceFuturesOrder =
            serde_json::from_str(ORDER_JSON).expect("Failed to parse order");

        assert_eq!(order.order_id, 12345678);
        assert_eq!(order.symbol.as_str(), "BTCUSDT");
        assert_eq!(order.status, BinanceOrderStatus::New);
        assert_eq!(order.side, BinanceSide::Buy);
        assert_eq!(order.order_type, BinanceFuturesOrderType::Limit);
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
}

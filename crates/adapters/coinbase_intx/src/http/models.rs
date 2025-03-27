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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use ustr::Ustr;
use uuid::Uuid;

use crate::common::enums::{
    CoinbaseIntxAlgoStrategy, CoinbaseIntxAssetStatus, CoinbaseIntxFeeTierType,
    CoinbaseIntxInstrumentType, CoinbaseIntxOrderEventType, CoinbaseIntxOrderStatus,
    CoinbaseIntxOrderType, CoinbaseIntxSTPMode, CoinbaseIntxSide, CoinbaseIntxTimeInForce,
    CoinbaseIntxTradingState,
};

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
}

fn deserialize_optional_datetime<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(ref s) if s.is_empty() => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(&s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Represents a Coinbase International asset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxAsset {
    /// Asset ID.
    pub asset_id: String,
    /// Asset UUID.
    pub asset_uuid: String,
    /// Asset name/symbol (e.g., "BTC").
    pub asset_name: String,
    /// Asset status (e.g., "ACTIVE").
    pub status: CoinbaseIntxAssetStatus,
    /// Weight used for collateral calculations.
    pub collateral_weight: f64,
    /// Whether supported networks are enabled.
    pub supported_networks_enabled: bool,
    /// Minimum borrow quantity allowed.
    pub min_borrow_qty: Option<String>,
    /// Maximum borrow quantity allowed.
    pub max_borrow_qty: Option<String>,
    /// Collateral requirement multiplier for loans.
    pub loan_collateral_requirement_multiplier: f64,
    /// Collateral limit per account.
    pub account_collateral_limit: Option<String>,
    /// Whether ecosystem collateral limit is breached.
    pub ecosystem_collateral_limit_breached: bool,
}

/// Represents a Coinbase International instrument.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxInstrument {
    /// Instrument ID.
    pub instrument_id: String,
    /// Instrument UUID.
    pub instrument_uuid: String,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Instrument type (e.g., "PERP"). Renamed from `type` because it is reserved in Rust.
    #[serde(rename = "type")]
    pub instrument_type: CoinbaseIntxInstrumentType,
    /// Mode (e.g., "STANDARD").
    pub mode: String,
    /// Base asset ID.
    pub base_asset_id: String,
    /// Base asset UUID.
    pub base_asset_uuid: String,
    /// Base asset name (e.g., "ETH", "BTC").
    pub base_asset_name: String,
    /// Quote asset ID.
    pub quote_asset_id: String,
    /// Quote asset UUID.
    pub quote_asset_uuid: String,
    /// Quote asset name (e.g., "USDC").
    pub quote_asset_name: String,
    /// Minimum increment for the base asset.
    pub base_increment: String,
    /// Minimum increment for the quote asset.
    pub quote_increment: String,
    /// Price band percent.
    pub price_band_percent: f64,
    /// Market order percent.
    pub market_order_percent: f64,
    /// 24-hour traded quantity.
    pub qty_24hr: String,
    /// 24-hour notional value.
    pub notional_24hr: String,
    /// Average daily quantity.
    pub avg_daily_qty: String,
    /// Average daily notional value.
    pub avg_daily_notional: String,
    /// Average 30‑day notional value.
    pub avg_30day_notional: String,
    /// Average 30‑day quantity.
    pub avg_30day_qty: String,
    /// Previous day's traded quantity.
    pub previous_day_qty: String,
    /// Open interest.
    pub open_interest: String,
    /// Position limit quantity.
    pub position_limit_qty: String,
    /// Position limit acquisition percent.
    pub position_limit_adq_pct: f64,
    /// Position notional limit.
    pub position_notional_limit: Option<String>,
    /// Open interest notional limit.
    pub open_interest_notional_limit: Option<String>,
    /// Replacement cost.
    pub replacement_cost: String,
    /// Base initial margin factor.
    pub base_imf: f64,
    /// Minimum notional value.
    pub min_notional_value: String,
    /// Funding interval.
    pub funding_interval: String,
    /// Trading state.
    pub trading_state: CoinbaseIntxTradingState,
    /// Quote details.
    pub quote: CoinbaseIntxInstrumentQuote,
    /// Default initial margin factor.
    pub default_imf: Option<f64>,
    /// Base asset multiplier.
    pub base_asset_multiplier: String,
    /// Underlying type (e.g., "SPOT", "PERP").
    pub underlying_type: CoinbaseIntxInstrumentType,
}

/// Represents a Coinbase International instrument quote.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxInstrumentQuote {
    /// Best bid price.
    #[serde(default)]
    pub best_bid_price: Option<String>,
    /// Best bid size.
    #[serde(default)]
    pub best_bid_size: Option<String>,
    /// Best ask price.
    #[serde(default)]
    pub best_ask_price: Option<String>,
    /// Best ask size.
    #[serde(default)]
    pub best_ask_size: Option<String>,
    /// Last traded price.
    #[serde(default)]
    pub trade_price: Option<String>,
    /// Last traded quantity.
    #[serde(default)]
    pub trade_qty: Option<String>,
    /// Index price.
    pub index_price: Option<String>,
    /// Mark price.
    pub mark_price: String,
    /// Settlement price.
    pub settlement_price: String,
    /// Upper price limit.
    pub limit_up: Option<String>,
    /// Lower price limit.
    pub limit_down: Option<String>,
    /// Predicted funding rate (optional; only provided for PERP instruments).
    #[serde(default)]
    pub predicted_funding: Option<String>,
    /// Timestamp of the quote.
    pub timestamp: DateTime<Utc>,
}

/// Represents a Coinbase International fee tier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxFeeTier {
    /// Type of fee tier (e.g., "REGULAR", "LIQUIDITY_PROGRAM")
    pub fee_tier_type: CoinbaseIntxFeeTierType,
    /// Type of instrument this fee tier applies to.
    pub instrument_type: String, // Not the same as CoinbaseInstrumentType
    /// Unique identifier for the fee tier.
    pub fee_tier_id: String,
    /// Human readable name for the fee tier.
    pub fee_tier_name: String,
    /// Maker fee rate as a decimal string.
    pub maker_fee_rate: String,
    /// Taker fee rate as a decimal string.
    pub taker_fee_rate: String,
    /// Minimum balance required for this tier.
    pub min_balance: String,
    /// Minimum volume required for this tier.
    pub min_volume: String,
    /// Whether both balance and volume requirements must be met.
    pub require_balance_and_volume: bool,
}

/// Represents Coinbase International portfolio fee rates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxPortfolioFeeRates {
    /// Type of instrument this fee rate applies to (e.g., "SPOT", "PERPETUAL_FUTURE")
    pub instrument_type: String, // Not the same as CoinbaseInstrumentType.
    /// Unique identifier for the fee tier.
    pub fee_tier_id: String,
    /// Human readable name for the fee tier.
    pub fee_tier_name: String,
    /// Fee rate applied when making liquidity, as a decimal string.
    pub maker_fee_rate: String,
    /// Fee rate applied when taking liquidity, as a decimal string.
    pub taker_fee_rate: String,
    /// Whether this is a VIP fee tier.
    pub is_vip_tier: bool,
    /// Whether these rates are overridden from the standard tier rates.
    pub is_override: bool,
    /// Trading volume over the last 30 days as a decimal string.
    #[serde(default)]
    pub trailing_30day_volume: Option<String>,
    /// USDC balance over the last 24 hours as a decimal string.
    pub trailing_24hr_usdc_balance: String,
}

/// A portfolio summary on Coinbase International.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxPortfolio {
    /// Unique identifier for the portfolio.
    pub portfolio_id: String,
    /// UUID for the portfolio.
    pub portfolio_uuid: Uuid,
    /// Human readable name for the portfolio.
    pub name: String,
    /// User UUID for brokers that attribute a single user per portfolio.
    pub user_uuid: Uuid,
    /// Fee rate charged for order making liquidity.
    pub maker_fee_rate: String,
    /// Fee rate charged for orders taking liquidity.
    pub taker_fee_rate: String,
    /// Whether the portfolio has been locked from trading.
    pub trading_lock: bool,
    /// Whether or not the portfolio can borrow.
    pub borrow_disabled: bool,
    /// Whether the portfolio is setup to take liquidation assignments.
    pub is_lsp: bool,
    /// Whether the portfolio is the account default portfolio.
    pub is_default: bool,
    /// Whether cross collateral is enabled for the portfolio.
    pub cross_collateral_enabled: bool,
    /// Whether pre-launch trading is enabled for the portfolio.
    pub pre_launch_trading_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxPortfolioDetails {
    pub summary: CoinbaseIntxPortfolioSummary,
    pub balances: Vec<CoinbaseIntxBalance>,
    pub positions: Vec<CoinbaseIntxPosition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxPortfolioSummary {
    pub collateral: String,
    pub unrealized_pnl: String,
    pub unrealized_pnl_percent: String,
    pub position_notional: String,
    pub balance: String,
    pub buying_power: String,
    pub portfolio_initial_margin: f64,
    pub portfolio_maintenance_margin: f64,
    pub in_liquidation: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxBalance {
    pub asset_id: String,
    pub asset_name: String,
    pub quantity: String,
    pub hold: String,
    pub collateral_value: String,
    pub max_withdraw_amount: String,
}

/// Response for listing orders.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxOrderList {
    /// Pagination information.
    pub pagination: OrderListPagination,
    /// List of orders matching the query.
    pub results: Vec<CoinbaseIntxOrder>,
}

/// Pagination information for list orders response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderListPagination {
    /// The datetime from which results were searched.
    pub ref_datetime: Option<DateTime<Utc>>,
    /// Number of results returned.
    pub result_limit: u32,
    /// Number of results skipped.
    pub result_offset: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxOrder {
    /// Unique identifier assigned by the exchange.
    pub order_id: Ustr,
    /// Unique identifier assigned by the client.
    pub client_order_id: Ustr,
    /// Side of the transaction (BUY/SELL).
    pub side: CoinbaseIntxSide,
    /// Unique identifier of the instrument.
    pub instrument_id: Ustr,
    /// UUID of the instrument.
    pub instrument_uuid: Uuid,
    /// Trading symbol (e.g., "BTC-PERP").
    pub symbol: Ustr,
    /// Portfolio identifier.
    pub portfolio_id: Ustr,
    /// Portfolio UUID.
    pub portfolio_uuid: Uuid,
    /// Order type (LIMIT, MARKET, etc.).
    #[serde(rename = "type")]
    pub order_type: CoinbaseIntxOrderType,
    /// Price limit in quote asset units (for limit and stop limit orders).
    pub price: Option<String>,
    /// Market price that activates a stop order.
    pub stop_price: Option<String>,
    /// Limit price for TP/SL stop leg orders.
    pub stop_limit_price: Option<String>,
    /// Amount in base asset units.
    pub size: String,
    /// Time in force for the order.
    pub tif: CoinbaseIntxTimeInForce,
    /// Expiration time for GTT orders.
    pub expire_time: Option<DateTime<Utc>>,
    /// Self-trade prevention mode.
    pub stp_mode: CoinbaseIntxSTPMode,
    /// Most recent event type for the order.
    pub event_type: CoinbaseIntxOrderEventType,
    /// Time of the most recent event.
    pub event_time: Option<DateTime<Utc>>,
    /// Time the order was submitted.
    pub submit_time: Option<DateTime<Utc>>,
    /// Current order status.
    pub order_status: CoinbaseIntxOrderStatus,
    /// Remaining open quantity.
    pub leaves_qty: String,
    /// Executed quantity.
    pub exec_qty: String,
    /// Average execution price.
    pub avg_price: Option<String>,
    /// Exchange fee for trades.
    pub fee: Option<String>,
    /// Whether order was post-only.
    pub post_only: bool,
    /// Whether order was close-only.
    pub close_only: bool,
    /// Algorithmic trading strategy.
    pub algo_strategy: Option<CoinbaseIntxAlgoStrategy>,
    /// Cancellation reason or other message.
    pub text: Option<String>,
}

/// Response for listing fills.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxFillList {
    /// Pagination information.
    pub pagination: OrderListPagination,
    /// List of fills matching the query.
    pub results: Vec<CoinbaseIntxFill>,
}

/// A fill in a Coinbase International portfolio.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxFill {
    /// Unique identifier for the portfolio.
    pub portfolio_id: Ustr,
    /// UUID for the portfolio.
    pub portfolio_uuid: Uuid,
    /// Human readable name for the portfolio.
    pub portfolio_name: String,
    /// Unique identifier for the fill.
    pub fill_id: String,
    /// UUID for the fill.
    pub fill_uuid: Uuid,
    /// Execution identifier.
    pub exec_id: String,
    /// Unique identifier for the order.
    pub order_id: Ustr,
    /// UUID for the order.
    pub order_uuid: Uuid,
    /// Unique identifier for the instrument.
    pub instrument_id: Ustr,
    /// UUID for the instrument.
    pub instrument_uuid: Uuid,
    /// Trading symbol (e.g., "BTC-PERP").
    pub symbol: Ustr,
    /// Unique identifier for the match.
    pub match_id: String,
    /// UUID for the match.
    pub match_uuid: Uuid,
    /// Price at which the fill executed.
    pub fill_price: String,
    /// Quantity filled in this execution.
    pub fill_qty: String,
    /// Client-assigned identifier.
    pub client_id: String,
    /// Client-assigned order identifier.
    pub client_order_id: Ustr,
    /// Original order quantity.
    pub order_qty: String,
    /// Original limit price of the order.
    pub limit_price: String,
    /// Total quantity filled for the order.
    pub total_filled: String,
    /// Volume-weighted average price of all fills for the order.
    pub filled_vwap: String,
    /// Expiration time for GTT orders.
    #[serde(deserialize_with = "deserialize_optional_datetime")]
    pub expire_time: Option<DateTime<Utc>>,
    /// Market price that activates a stop order.
    #[serde(default)]
    #[serde(deserialize_with = "empty_string_as_none")]
    pub stop_price: Option<String>,
    /// Side of the transaction (BUY/SELL).
    pub side: CoinbaseIntxSide,
    /// Time in force for the order.
    pub tif: CoinbaseIntxTimeInForce,
    /// Self-trade prevention mode.
    pub stp_mode: CoinbaseIntxSTPMode,
    /// Order flags as a string.
    pub flags: String,
    /// Fee charged for the trade.
    pub fee: String,
    /// Asset in which the fee was charged.
    pub fee_asset: String,
    /// Current order status.
    pub order_status: CoinbaseIntxOrderStatus,
    /// Time of the fill event.
    pub event_time: DateTime<Utc>,
    /// Source of the fill.
    pub source: String,
}

/// A position in a Coinbase portfolio.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxPosition {
    /// Unique identifier for the position.
    pub id: String,
    /// UUID for the position.
    pub uuid: Uuid,
    /// Trading symbol (e.g., "ETH-PERP").
    pub symbol: Ustr,
    /// Instrument ID.
    pub instrument_id: Ustr,
    /// Instrument UUID.
    pub instrument_uuid: Uuid,
    /// Volume Weighted Average Price.
    pub vwap: String,
    /// Net size of the position.
    pub net_size: String,
    /// Size of buy orders.
    pub buy_order_size: String,
    /// Size of sell orders.
    pub sell_order_size: String,
    /// Initial Margin contribution.
    pub im_contribution: String,
    /// Unrealized Profit and Loss.
    pub unrealized_pnl: String,
    /// Mark price.
    pub mark_price: String,
    /// Entry VWAP.
    pub entry_vwap: String,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::{enums::CoinbaseIntxTradingState, testing::load_test_json};

    #[rstest]
    fn test_parse_asset_model() {
        let json_data = load_test_json("http_get_assets_BTC.json");
        let parsed: CoinbaseIntxAsset = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.asset_id, "118059611751202816");
        assert_eq!(parsed.asset_uuid, "5b71fc48-3dd3-540c-809b-f8c94d0e68b5");
        assert_eq!(parsed.asset_name, "BTC");
        assert_eq!(parsed.status, CoinbaseIntxAssetStatus::Active);
        assert_eq!(parsed.collateral_weight, 0.9);
        assert_eq!(parsed.supported_networks_enabled, true);
        assert_eq!(parsed.min_borrow_qty, Some("0".to_string()));
        assert_eq!(parsed.max_borrow_qty, Some("0".to_string()));
        assert_eq!(parsed.loan_collateral_requirement_multiplier, 0.0);
        assert_eq!(parsed.account_collateral_limit, Some("0".to_string()));
        assert_eq!(parsed.ecosystem_collateral_limit_breached, false);
    }

    #[rstest]
    fn test_parse_spot_model() {
        let json_data = load_test_json("http_get_instruments_BTC-USDC.json");
        let parsed: CoinbaseIntxInstrument = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.instrument_id, "252572044003115008");
        assert_eq!(
            parsed.instrument_uuid,
            "cf8dee38-6d4e-4658-a5ff-70c19201c485"
        );
        assert_eq!(parsed.symbol, "BTC-USDC");
        assert_eq!(parsed.instrument_type, CoinbaseIntxInstrumentType::Spot);
        assert_eq!(parsed.mode, "STANDARD");
        assert_eq!(parsed.base_asset_id, "118059611751202816");
        assert_eq!(
            parsed.base_asset_uuid,
            "5b71fc48-3dd3-540c-809b-f8c94d0e68b5"
        );
        assert_eq!(parsed.base_asset_name, "BTC");
        assert_eq!(parsed.quote_asset_id, "1");
        assert_eq!(
            parsed.quote_asset_uuid,
            "2b92315d-eab7-5bef-84fa-089a131333f5"
        );
        assert_eq!(parsed.quote_asset_name, "USDC");
        assert_eq!(parsed.base_increment, "0.00001");
        assert_eq!(parsed.quote_increment, "0.01");
        assert_eq!(parsed.price_band_percent, 0.02);
        assert_eq!(parsed.market_order_percent, 0.0075);
        assert_eq!(parsed.qty_24hr, "0");
        assert_eq!(parsed.notional_24hr, "0");
        assert_eq!(parsed.avg_daily_qty, "1241.5042833333332");
        assert_eq!(parsed.avg_daily_notional, "125201028.9956107");
        assert_eq!(parsed.avg_30day_notional, "3756030869.868321");
        assert_eq!(parsed.avg_30day_qty, "37245.1285");
        assert_eq!(parsed.previous_day_qty, "0");
        assert_eq!(parsed.open_interest, "0");
        assert_eq!(parsed.position_limit_qty, "0");
        assert_eq!(parsed.position_limit_adq_pct, 0.0);
        assert_eq!(parsed.position_notional_limit.as_ref().unwrap(), "5000000");
        assert_eq!(
            parsed.open_interest_notional_limit.as_ref().unwrap(),
            "26000000"
        );
        assert_eq!(parsed.replacement_cost, "0");
        assert_eq!(parsed.base_imf, 1.0);
        assert_eq!(parsed.min_notional_value, "10");
        assert_eq!(parsed.funding_interval, "0");
        assert_eq!(parsed.trading_state, CoinbaseIntxTradingState::Trading);
        assert_eq!(parsed.default_imf.unwrap(), 1.0);
        assert_eq!(parsed.base_asset_multiplier, "1.0");
        assert_eq!(parsed.underlying_type, CoinbaseIntxInstrumentType::Spot);

        // Quote assertions
        assert_eq!(parsed.quote.best_bid_size.as_ref().unwrap(), "0");
        assert_eq!(parsed.quote.best_ask_size.as_ref().unwrap(), "0");
        assert_eq!(parsed.quote.trade_price, Some("101761.64".to_string()));
        assert_eq!(parsed.quote.trade_qty, Some("3".to_string()));
        assert_eq!(parsed.quote.index_price.as_ref().unwrap(), "97728.02");
        assert_eq!(parsed.quote.mark_price, "101761.64");
        assert_eq!(parsed.quote.settlement_price, "101761.64");
        assert_eq!(parsed.quote.limit_up.as_ref().unwrap(), "102614.41");
        assert_eq!(parsed.quote.limit_down.as_ref().unwrap(), "92841.61");
        assert_eq!(
            parsed.quote.timestamp.to_rfc3339(),
            "2025-02-05T06:40:23.040+00:00"
        );
    }

    #[rstest]
    fn test_parse_perp_model() {
        let json_data = load_test_json("http_get_instruments_BTC-PERP.json");
        let parsed: CoinbaseIntxInstrument = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.instrument_id, "149264167780483072");
        assert_eq!(
            parsed.instrument_uuid,
            "b3469e0b-222c-4f8a-9f68-1f9e44d7e5e0"
        );
        assert_eq!(parsed.symbol, "BTC-PERP");
        assert_eq!(parsed.instrument_type, CoinbaseIntxInstrumentType::Perp);
        assert_eq!(parsed.mode, "STANDARD");
        assert_eq!(parsed.base_asset_id, "118059611751202816");
        assert_eq!(
            parsed.base_asset_uuid,
            "5b71fc48-3dd3-540c-809b-f8c94d0e68b5"
        );
        assert_eq!(parsed.base_asset_name, "BTC");
        assert_eq!(parsed.quote_asset_id, "1");
        assert_eq!(
            parsed.quote_asset_uuid,
            "2b92315d-eab7-5bef-84fa-089a131333f5"
        );
        assert_eq!(parsed.quote_asset_name, "USDC");
        assert_eq!(parsed.base_increment, "0.0001");
        assert_eq!(parsed.quote_increment, "0.1");
        assert_eq!(parsed.price_band_percent, 0.05);
        assert_eq!(parsed.market_order_percent, 0.01);
        assert_eq!(parsed.qty_24hr, "0.0051");
        assert_eq!(parsed.notional_24hr, "499.3577");
        assert_eq!(parsed.avg_daily_qty, "2362.797683333333");
        assert_eq!(parsed.avg_daily_notional, "237951057.95349997");
        assert_eq!(parsed.avg_30day_notional, "7138531738.605");
        assert_eq!(parsed.avg_30day_qty, "70883.9305");
        assert_eq!(parsed.previous_day_qty, "0.0116");
        assert_eq!(parsed.open_interest, "899.6503");
        assert_eq!(parsed.position_limit_qty, "2362.7977");
        assert_eq!(parsed.position_limit_adq_pct, 1.0);
        assert_eq!(
            parsed.position_notional_limit.as_ref().unwrap(),
            "120000000"
        );
        assert_eq!(
            parsed.open_interest_notional_limit.as_ref().unwrap(),
            "300000000"
        );
        assert_eq!(parsed.replacement_cost, "0.19");
        assert_eq!(parsed.base_imf, 0.1);
        assert_eq!(parsed.min_notional_value, "10");
        assert_eq!(parsed.funding_interval, "3600000000000");
        assert_eq!(parsed.trading_state, CoinbaseIntxTradingState::Trading);
        assert_eq!(parsed.default_imf.unwrap(), 0.2);
        assert_eq!(parsed.base_asset_multiplier, "1.0");
        assert_eq!(parsed.underlying_type, CoinbaseIntxInstrumentType::Spot);

        assert_eq!(parsed.quote.best_bid_price.as_ref().unwrap(), "96785.5");
        assert_eq!(parsed.quote.best_bid_size.as_ref().unwrap(), "0.0005");
        assert_eq!(parsed.quote.best_ask_size.as_ref().unwrap(), "0");
        assert_eq!(parsed.quote.trade_price, Some("97908.8".to_string()));
        assert_eq!(parsed.quote.trade_qty, Some("0.0005".to_string()));
        assert_eq!(parsed.quote.index_price.as_ref().unwrap(), "97743.1");
        assert_eq!(parsed.quote.mark_price, "97908.8");
        assert_eq!(parsed.quote.settlement_price, "97908.8");
        assert_eq!(parsed.quote.limit_up.as_ref().unwrap(), "107517.3");
        assert_eq!(parsed.quote.limit_down.as_ref().unwrap(), "87968.7");
        assert_eq!(
            parsed.quote.predicted_funding.as_ref().unwrap(),
            "-0.000044"
        );
        assert_eq!(
            parsed.quote.timestamp.to_rfc3339(),
            "2025-02-05T06:40:42.399+00:00"
        );
    }

    #[rstest]
    fn test_parse_fee_rate_tiers() {
        let json_data = load_test_json("http_get_fee-rate-tiers.json");
        let parsed: Vec<CoinbaseIntxFeeTier> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.len(), 2);

        let first = &parsed[0];
        assert_eq!(first.fee_tier_type, CoinbaseIntxFeeTierType::Regular);
        assert_eq!(first.instrument_type, "PERPETUAL_FUTURE");
        assert_eq!(first.fee_tier_id, "1");
        assert_eq!(first.fee_tier_name, "Public Tier 6");
        assert_eq!(
            first.maker_fee_rate,
            "0.00020000000000000000958434720477185919662588275969028472900390625"
        );
        assert_eq!(
            first.taker_fee_rate,
            "0.0004000000000000000191686944095437183932517655193805694580078125"
        );
        assert_eq!(first.min_balance, "0");
        assert_eq!(first.min_volume, "0");
        assert!(!first.require_balance_and_volume);

        let second = &parsed[1];
        assert_eq!(second.fee_tier_type, CoinbaseIntxFeeTierType::Regular);
        assert_eq!(second.instrument_type, "PERPETUAL_FUTURE");
        assert_eq!(second.fee_tier_id, "2");
        assert_eq!(second.fee_tier_name, "Public Tier 5");
        assert_eq!(
            second.maker_fee_rate,
            "0.00016000000000000001308848862624500952733797021210193634033203125"
        );
        assert_eq!(
            second.taker_fee_rate,
            "0.0004000000000000000191686944095437183932517655193805694580078125"
        );
        assert_eq!(second.min_balance, "50000");
        assert_eq!(second.min_volume, "1000000");
        assert!(second.require_balance_and_volume);
    }

    #[rstest]
    fn test_parse_order() {
        let json_data = load_test_json("http_post_orders.json");
        let parsed: CoinbaseIntxOrder = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.order_id, "2v2ckc1g-1-0");
        assert_eq!(
            parsed.client_order_id,
            "f346ca69-11b4-4e1b-ae47-85971290c771"
        );
        assert_eq!(parsed.side, CoinbaseIntxSide::Sell);
        assert_eq!(parsed.instrument_id, "114jqqhr-0-0");
        assert_eq!(
            parsed.instrument_uuid,
            Uuid::parse_str("e9360798-6a10-45d6-af05-67c30eb91e2d").unwrap()
        );
        assert_eq!(parsed.symbol, "ETH-PERP");
        assert_eq!(parsed.portfolio_id, "3mnk39ap-1-21");
        assert_eq!(
            parsed.portfolio_uuid,
            Uuid::parse_str("cc0958ad-0c7d-4445-a812-1370fe46d0d4").unwrap()
        );
        assert_eq!(parsed.order_type, CoinbaseIntxOrderType::Limit);
        assert_eq!(parsed.price, Some("3000".to_string()));
        assert_eq!(parsed.stop_price, None);
        assert_eq!(parsed.stop_limit_price, None);
        assert_eq!(parsed.size, "0.01");
        assert_eq!(parsed.tif, CoinbaseIntxTimeInForce::Gtc);
        assert_eq!(parsed.expire_time, None);
        assert_eq!(parsed.stp_mode, CoinbaseIntxSTPMode::Both);
        assert_eq!(parsed.event_type, CoinbaseIntxOrderEventType::New);
        assert_eq!(parsed.event_time, None);
        assert_eq!(parsed.submit_time, None);
        assert_eq!(parsed.order_status, CoinbaseIntxOrderStatus::Working);
        assert_eq!(parsed.leaves_qty, "0.01");
        assert_eq!(parsed.exec_qty, "0");
        assert_eq!(parsed.avg_price, Some("0".to_string()));
        assert_eq!(parsed.fee, Some("0".to_string()));
        assert_eq!(parsed.post_only, false);
        assert_eq!(parsed.close_only, false);
        assert_eq!(parsed.algo_strategy, None);
        assert_eq!(parsed.text, None);
    }

    #[rstest]
    fn test_parse_position() {
        let json_data = load_test_json("http_get_portfolios_positions_ETH-PERP.json");
        let parsed: CoinbaseIntxPosition = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.id, "2vev82mx-1-57");
        assert_eq!(
            parsed.uuid,
            Uuid::parse_str("cb1df22f-05c7-8000-8000-7102a7804039").unwrap()
        );
        assert_eq!(parsed.symbol, "ETH-PERP");
        assert_eq!(parsed.instrument_id, "114jqqhr-0-0");
        assert_eq!(
            parsed.instrument_uuid,
            Uuid::parse_str("e9360798-6a10-45d6-af05-67c30eb91e2d").unwrap()
        );
        assert_eq!(parsed.vwap, "2747.71");
        assert_eq!(parsed.net_size, "0.01");
        assert_eq!(parsed.buy_order_size, "0");
        assert_eq!(parsed.sell_order_size, "0");
        assert_eq!(parsed.im_contribution, "0.2");
        assert_eq!(parsed.unrealized_pnl, "0.0341");
        assert_eq!(parsed.mark_price, "2751.12");
        assert_eq!(parsed.entry_vwap, "2749.61");
    }
}

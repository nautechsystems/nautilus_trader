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

//! Deribit HTTP API models and types.

use std::fmt::Display;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

pub use crate::common::rpc::{DeribitJsonRpcError, DeribitJsonRpcRequest, DeribitJsonRpcResponse};

/// JSON-RPC 2.0 response payload (either success or error).
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeribitResponsePayload<T> {
    /// Successful response with result data
    Success { result: T },
    /// Error response with error details
    Error { error: DeribitJsonRpcError },
}

/// Deribit instrument definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitInstrument {
    /// The underlying currency being traded
    pub base_currency: Ustr,
    /// Block trade commission for instrument
    #[serde(default)]
    pub block_trade_commission: Option<f64>,
    /// Minimum amount for block trading
    pub block_trade_min_trade_amount: Option<f64>,
    /// Specifies minimal price change for block trading
    #[serde(default)]
    pub block_trade_tick_size: Option<f64>,
    /// Contract size for instrument
    pub contract_size: f64,
    /// Counter currency for the instrument
    pub counter_currency: Option<Ustr>,
    /// The time when the instrument was first created (milliseconds since UNIX epoch)
    pub creation_timestamp: i64,
    /// The time when the instrument will expire (milliseconds since UNIX epoch)
    pub expiration_timestamp: Option<i64>,
    /// Future type (deprecated, use instrument_type instead)
    pub future_type: Option<String>,
    /// Instrument ID
    pub instrument_id: i64,
    /// Unique instrument identifier (e.g., "BTC-PERPETUAL")
    pub instrument_name: Ustr,
    /// Type of the instrument: "linear" or "reversed"
    pub instrument_type: Option<String>,
    /// Indicates if the instrument can currently be traded
    pub is_active: bool,
    /// Instrument kind: "future", "option", "spot", "future_combo", "option_combo"
    pub kind: DeribitInstrumentKind,
    /// Maker commission for instrument
    pub maker_commission: f64,
    /// Maximal leverage for instrument (only for futures)
    pub max_leverage: Option<i64>,
    /// Maximal liquidation trade commission for instrument (only for futures)
    pub max_liquidation_commission: Option<f64>,
    /// Minimum amount for trading
    pub min_trade_amount: f64,
    /// The option type (only for options)
    pub option_type: Option<DeribitOptionType>,
    /// Name of price index that is used for this instrument
    pub price_index: Option<String>,
    /// The currency in which the instrument prices are quoted
    pub quote_currency: Ustr,
    /// Settlement currency for the instrument (not present for spot)
    pub settlement_currency: Option<Ustr>,
    /// The settlement period (not present for spot)
    pub settlement_period: Option<String>,
    /// The strike value (only for options)
    pub strike: Option<f64>,
    /// Taker commission for instrument
    pub taker_commission: f64,
    /// Specifies minimal price change and number of decimal places for instrument prices
    pub tick_size: f64,
    /// Tick size steps for different price ranges
    pub tick_size_steps: Option<Vec<DeribitTickSizeStep>>,
}

/// Tick size step definition for price-dependent tick sizes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTickSizeStep {
    /// The price from which the increased tick size applies
    pub above_price: f64,
    /// Tick size to be used above the price
    pub tick_size: f64,
}

/// Deribit instrument kind.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitInstrumentKind {
    /// Future contract
    Future,
    /// Option contract
    Option,
    /// Spot market
    Spot,
    /// Future combo
    #[serde(rename = "future_combo")]
    FutureCombo,
    /// Option combo
    #[serde(rename = "option_combo")]
    OptionCombo,
}

/// Deribit currency.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    strum::AsRefStr,
    strum::EnumIter,
    strum::EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitCurrency {
    /// Bitcoin
    BTC,
    /// Ethereum
    ETH,
    /// USD Coin
    USDC,
    /// Tether
    USDT,
    /// Euro stablecoin
    EURR,
    /// All currencies
    #[serde(rename = "any")]
    ANY,
}

/// Deribit option type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitOptionType {
    /// Call option
    Call,
    /// Put option
    Put,
}

impl DeribitCurrency {
    /// Returns the currency as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BTC => "BTC",
            Self::ETH => "ETH",
            Self::USDC => "USDC",
            Self::USDT => "USDT",
            Self::EURR => "EURR",
            Self::ANY => "any",
        }
    }
}

impl Display for DeribitCurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Wrapper for the account summaries response.
///
/// The API returns an object with a `summaries` field containing the array of account summaries,
/// plus account-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitAccountSummariesResponse {
    /// Array of per-currency account summaries
    pub summaries: Vec<DeribitAccountSummary>,
    /// Account ID
    #[serde(default)]
    pub id: Option<i64>,
    /// Account email
    #[serde(default)]
    pub email: Option<String>,
    /// System name
    #[serde(default)]
    pub system_name: Option<String>,
    /// Account username
    #[serde(default)]
    pub username: Option<String>,
    /// Account type (e.g., "main", "subaccount")
    #[serde(rename = "type", default)]
    pub account_type: Option<String>,
    /// Account creation timestamp (milliseconds since UNIX epoch)
    #[serde(default)]
    pub creation_timestamp: Option<i64>,
    /// Referrer ID (affiliation program)
    #[serde(default)]
    pub referrer_id: Option<String>,
    /// Whether login is enabled for this account
    #[serde(default)]
    pub login_enabled: Option<bool>,
    /// Whether security keys are enabled
    #[serde(default)]
    pub security_keys_enabled: Option<bool>,
    /// Whether MMP (Market Maker Protection) is enabled
    #[serde(default)]
    pub mmp_enabled: Option<bool>,
    /// Whether inter-user transfers are enabled
    #[serde(default)]
    pub interuser_transfers_enabled: Option<bool>,
    /// Self-trading reject mode
    #[serde(default)]
    pub self_trading_reject_mode: Option<String>,
    /// Whether self-trading is extended to subaccounts
    #[serde(default)]
    pub self_trading_extended_to_subaccounts: Option<bool>,
    /// Block RFQ self match prevention
    #[serde(default)]
    pub block_rfq_self_match_prevention: Option<bool>,
}

/// Account summary for a single currency.
///
/// Contains balance, equity, margin information, and profit/loss data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitAccountSummary {
    /// Currency code (e.g., "BTC", "ETH")
    pub currency: Ustr,
    /// Account equity (balance + unrealized PnL)
    pub equity: f64,
    /// Account balance
    pub balance: f64,
    /// Available funds for trading
    pub available_funds: f64,
    /// Margin balance (for derivatives)
    pub margin_balance: f64,
    /// Initial margin (required for current positions)
    #[serde(default)]
    pub initial_margin: Option<f64>,
    /// Maintenance margin
    #[serde(default)]
    pub maintenance_margin: Option<f64>,
    /// Total profit/loss
    #[serde(default)]
    pub total_pl: Option<f64>,
    /// Session unrealized profit/loss
    #[serde(default)]
    pub session_upl: Option<f64>,
    /// Session realized profit/loss
    #[serde(default)]
    pub session_rpl: Option<f64>,
    /// Portfolio margining enabled
    #[serde(default)]
    pub portfolio_margining_enabled: Option<bool>,
}

/// Extended account summary with additional account details.
///
/// Returned by `private/get_account_summary` with `extended=true`.
/// Contains all fields from [`DeribitAccountSummary`] plus account metadata,
/// position Greeks, detailed margins, and fee structures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitAccountSummaryExtended {
    /// Currency code (e.g., "BTC", "ETH")
    pub currency: Ustr,
    /// Account equity (balance + unrealized PnL)
    pub equity: f64,
    /// Account balance
    pub balance: f64,
    /// Available funds for trading
    pub available_funds: f64,
    /// Margin balance (for derivatives)
    pub margin_balance: f64,
    /// Initial margin (required for current positions)
    #[serde(default)]
    pub initial_margin: Option<f64>,
    /// Maintenance margin
    #[serde(default)]
    pub maintenance_margin: Option<f64>,
    /// Total profit/loss
    #[serde(default)]
    pub total_pl: Option<f64>,
    /// Session unrealized profit/loss
    #[serde(default)]
    pub session_upl: Option<f64>,
    /// Session realized profit/loss
    #[serde(default)]
    pub session_rpl: Option<f64>,
    /// Portfolio margining enabled
    #[serde(default)]
    pub portfolio_margining_enabled: Option<bool>,
    // Extended fields below
    /// Account ID
    #[serde(default)]
    pub id: Option<i64>,
    /// Account email
    #[serde(default)]
    pub email: Option<String>,
    /// Account username
    #[serde(default)]
    pub username: Option<String>,
    /// System name
    #[serde(default)]
    pub system_name: Option<String>,
    /// Account type (e.g., "main", "subaccount")
    #[serde(rename = "type", default)]
    pub account_type: Option<String>,
    /// Futures session unrealized P&L
    #[serde(default)]
    pub futures_session_upl: Option<f64>,
    /// Futures session realized P&L
    #[serde(default)]
    pub futures_session_rpl: Option<f64>,
    /// Options session unrealized P&L
    #[serde(default)]
    pub options_session_upl: Option<f64>,
    /// Options session realized P&L
    #[serde(default)]
    pub options_session_rpl: Option<f64>,
    /// Futures profit/loss
    #[serde(default)]
    pub futures_pl: Option<f64>,
    /// Options profit/loss
    #[serde(default)]
    pub options_pl: Option<f64>,
    /// Options delta
    #[serde(default)]
    pub options_delta: Option<f64>,
    /// Options gamma
    #[serde(default)]
    pub options_gamma: Option<f64>,
    /// Options vega
    #[serde(default)]
    pub options_vega: Option<f64>,
    /// Options theta
    #[serde(default)]
    pub options_theta: Option<f64>,
    /// Options value
    #[serde(default)]
    pub options_value: Option<f64>,
    /// Total delta across all positions
    #[serde(default)]
    pub delta_total: Option<f64>,
    /// Projected delta total
    #[serde(default)]
    pub projected_delta_total: Option<f64>,
    /// Projected initial margin
    #[serde(default)]
    pub projected_initial_margin: Option<f64>,
    /// Projected maintenance margin
    #[serde(default)]
    pub projected_maintenance_margin: Option<f64>,
    /// Estimated liquidation ratio
    #[serde(default)]
    pub estimated_liquidation_ratio: Option<f64>,
    /// Available withdrawal funds
    #[serde(default)]
    pub available_withdrawal_funds: Option<f64>,
    /// Spot reserve
    #[serde(default)]
    pub spot_reserve: Option<f64>,
    /// Fee balance
    #[serde(default)]
    pub fee_balance: Option<f64>,
    /// Margin model (e.g., "segregated_sm", "cross_pm")
    #[serde(default)]
    pub margin_model: Option<String>,
    /// Cross collateral enabled
    #[serde(default)]
    pub cross_collateral_enabled: Option<bool>,
    /// Account creation timestamp (milliseconds since UNIX epoch)
    #[serde(default)]
    pub creation_timestamp: Option<i64>,
    /// Whether login is enabled for this account
    #[serde(default)]
    pub login_enabled: Option<bool>,
    /// Whether security keys are enabled
    #[serde(default)]
    pub security_keys_enabled: Option<bool>,
    /// Whether MMP (Market Maker Protection) is enabled
    #[serde(default)]
    pub mmp_enabled: Option<bool>,
    /// Whether inter-user transfers are enabled
    #[serde(default)]
    pub interuser_transfers_enabled: Option<bool>,
    /// Self-trading reject mode
    #[serde(default)]
    pub self_trading_reject_mode: Option<String>,
    /// Whether self-trading is extended to subaccounts
    #[serde(default)]
    pub self_trading_extended_to_subaccounts: Option<bool>,
    /// Referrer ID (affiliation program)
    #[serde(default)]
    pub referrer_id: Option<String>,
    /// Block RFQ self match prevention
    #[serde(default)]
    pub block_rfq_self_match_prevention: Option<bool>,
}

/// Deribit public trade data from the market data API.
///
/// Represents a single trade returned by `/public/get_last_trades_by_instrument_and_time`
/// and other trade-related endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitPublicTrade {
    /// Trade amount. For perpetual and inverse futures the amount is in USD units.
    /// For options and linear futures it is the underlying base currency coin.
    pub amount: f64,
    /// Trade size in contract units (optional, may be absent in historical trades).
    #[serde(default)]
    pub contracts: Option<f64>,
    /// Direction of the trade: "buy" or "sell"
    pub direction: String,
    /// Index Price at the moment of trade.
    pub index_price: f64,
    /// Unique instrument identifier.
    pub instrument_name: String,
    /// Option implied volatility for the price (Option only).
    #[serde(default)]
    pub iv: Option<f64>,
    /// Optional field (only for trades caused by liquidation).
    #[serde(default)]
    pub liquidation: Option<String>,
    /// Mark Price at the moment of trade.
    pub mark_price: f64,
    /// Price in base currency.
    pub price: f64,
    /// Direction of the "tick" (0 = Plus Tick, 1 = Zero-Plus Tick, 2 = Minus Tick, 3 = Zero-Minus Tick).
    pub tick_direction: i32,
    /// The timestamp of the trade (milliseconds since the UNIX epoch).
    pub timestamp: i64,
    /// Unique (per currency) trade identifier.
    pub trade_id: String,
    /// The sequence number of the trade within instrument.
    pub trade_seq: i64,
    /// Block trade id - when trade was part of a block trade.
    #[serde(default)]
    pub block_trade_id: Option<String>,
    /// Block trade leg count - when trade was part of a block trade.
    #[serde(default)]
    pub block_trade_leg_count: Option<i32>,
    /// ID of the Block RFQ - when trade was part of the Block RFQ.
    #[serde(default)]
    pub block_rfq_id: Option<i64>,
    /// Optional field containing combo instrument name if the trade is a combo trade.
    #[serde(default)]
    pub combo_id: Option<String>,
    /// Optional field containing combo trade identifier if the trade is a combo trade.
    #[serde(default)]
    pub combo_trade_id: Option<f64>,
}

/// Response wrapper for trades endpoints.
///
/// Contains the trades array and pagination information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTradesResponse {
    /// Whether there are more trades available.
    pub has_more: bool,
    /// Array of trade objects.
    pub trades: Vec<DeribitPublicTrade>,
}

/// Response from `public/get_tradingview_chart_data` endpoint.
///
/// Contains OHLCV data in array format where each array element corresponds
/// to a single candle at the index in the `ticks` array.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTradingViewChartData {
    /// List of prices at close (one per candle)
    pub close: Vec<f64>,
    /// List of cost bars (volume in quote currency, one per candle)
    #[serde(default)]
    pub cost: Vec<f64>,
    /// List of highest price levels (one per candle)
    pub high: Vec<f64>,
    /// List of lowest price levels (one per candle)
    pub low: Vec<f64>,
    /// List of prices at open (one per candle)
    pub open: Vec<f64>,
    /// Status of the query: "ok" or "no_data"
    pub status: String,
    /// Values of the time axis given in milliseconds since UNIX epoch
    pub ticks: Vec<i64>,
    /// List of volume bars (in base currency, one per candle)
    pub volume: Vec<f64>,
}

/// Response from `public/get_order_book` endpoint.
///
/// Contains the current order book state with bids, asks, and market data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitOrderBook {
    /// The timestamp of the order book (milliseconds since UNIX epoch)
    pub timestamp: i64,
    /// Unique instrument identifier
    pub instrument_name: String,
    /// List of bids as [price, amount] pairs
    pub bids: Vec<[f64; 2]>,
    /// List of asks as [price, amount] pairs
    pub asks: Vec<[f64; 2]>,
    /// The state of the order book: "open" or "closed"
    pub state: String,
    /// The current best bid price (null if there aren't any bids)
    #[serde(default)]
    pub best_bid_price: Option<f64>,
    /// The current best ask price (null if there aren't any asks)
    #[serde(default)]
    pub best_ask_price: Option<f64>,
    /// The order size of all best bids
    #[serde(default)]
    pub best_bid_amount: Option<f64>,
    /// The order size of all best asks
    #[serde(default)]
    pub best_ask_amount: Option<f64>,
    /// The mark price for the instrument
    #[serde(default)]
    pub mark_price: Option<f64>,
    /// The price for the last trade
    #[serde(default)]
    pub last_price: Option<f64>,
    /// Current index price
    #[serde(default)]
    pub index_price: Option<f64>,
    /// The total amount of outstanding contracts
    #[serde(default)]
    pub open_interest: Option<f64>,
    /// The maximum price for the future
    #[serde(default)]
    pub max_price: Option<f64>,
    /// The minimum price for the future
    #[serde(default)]
    pub min_price: Option<f64>,
    /// Current funding (perpetual only)
    #[serde(default)]
    pub current_funding: Option<f64>,
    /// Funding 8h (perpetual only)
    #[serde(default)]
    pub funding_8h: Option<f64>,
    /// The settlement price for the instrument (when state = open)
    #[serde(default)]
    pub settlement_price: Option<f64>,
    /// The settlement/delivery price for the instrument (when state = closed)
    #[serde(default)]
    pub delivery_price: Option<f64>,
    /// (Only for option) implied volatility for best bid
    #[serde(default)]
    pub bid_iv: Option<f64>,
    /// (Only for option) implied volatility for best ask
    #[serde(default)]
    pub ask_iv: Option<f64>,
    /// (Only for option) implied volatility for mark price
    #[serde(default)]
    pub mark_iv: Option<f64>,
    /// Underlying price for implied volatility calculations (options only)
    #[serde(default)]
    pub underlying_price: Option<f64>,
    /// Name of the underlying future, or index_price (options only)
    #[serde(default)]
    pub underlying_index: Option<serde_json::Value>,
    /// Interest rate used in implied volatility calculations (options only)
    #[serde(default)]
    pub interest_rate: Option<f64>,
}

/// Position data from `/private/get_positions` endpoint.
///
/// Contains information about a single position in a specific instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitPosition {
    /// Unique instrument identifier
    pub instrument_name: Ustr,
    /// Position direction: "buy" (long), "sell" (short), or "zero" (flat)
    pub direction: String,
    /// Position size in contracts (positive = long, negative = short)
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub size: Decimal,
    /// Average entry price
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub average_price: Decimal,
    /// Current mark price
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub mark_price: Decimal,
    /// Current index price
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub index_price: Option<Decimal>,
    /// Maintenance margin
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub maintenance_margin: Decimal,
    /// Initial margin
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub initial_margin: Decimal,
    /// Leverage used for the position
    #[serde(default)]
    pub leverage: Option<i64>,
    /// Current unrealized profit/loss
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub floating_profit_loss: Decimal,
    /// Realized profit/loss for this position
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub realized_profit_loss: Decimal,
    /// Total profit/loss (floating + realized)
    #[serde(
        serialize_with = "nautilus_core::serialization::serialize_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_decimal"
    )]
    pub total_profit_loss: Decimal,
    /// Instrument kind: future, option, spot, etc.
    pub kind: DeribitInstrumentKind,
    /// Position size in currency units (for currency-quoted positions)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub size_currency: Option<Decimal>,
    /// Estimated liquidation price
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub estimated_liquidation_price: Option<Decimal>,
    /// Position delta (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub delta: Option<Decimal>,
    /// Position gamma (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub gamma: Option<Decimal>,
    /// Position vega (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub vega: Option<Decimal>,
    /// Position theta (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub theta: Option<Decimal>,
    /// Settlement price (if settled)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub settlement_price: Option<Decimal>,
    /// Open orders margin for this position
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub open_orders_margin: Option<Decimal>,
    /// Average price in USD (for currency-margined contracts)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub average_price_usd: Option<Decimal>,
    /// Realized profit loss (session)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub realized_profit_loss_session: Option<Decimal>,
    /// Floating profit loss in USD
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal_flexible"
    )]
    pub floating_profit_loss_usd: Option<Decimal>,
}

/// Response wrapper for user trades endpoints.
///
/// Contains the trades array and pagination information.
/// Used by `/private/get_user_trades_by_*` endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitUserTradesResponse {
    /// Whether there are more trades available.
    pub has_more: bool,
    /// Array of user trade objects.
    pub trades: Vec<crate::websocket::messages::DeribitUserTradeMsg>,
}

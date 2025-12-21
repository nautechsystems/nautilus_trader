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

//! Deribit HTTP API models and types.

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

impl std::fmt::Display for DeribitCurrency {
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

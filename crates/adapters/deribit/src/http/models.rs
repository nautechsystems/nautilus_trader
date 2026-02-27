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

use nautilus_core::serialization::{deserialize_decimal, deserialize_optional_decimal};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

pub use crate::common::{
    enums::{DeribitCurrency, DeribitOptionType, DeribitProductType},
    rpc::{DeribitJsonRpcError, DeribitJsonRpcRequest, DeribitJsonRpcResponse},
};

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
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub block_trade_commission: Option<Decimal>,
    /// Minimum amount for block trading
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub block_trade_min_trade_amount: Option<Decimal>,
    /// Specifies minimal price change for block trading
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub block_trade_tick_size: Option<Decimal>,
    /// Contract size for instrument
    #[serde(deserialize_with = "deserialize_decimal")]
    pub contract_size: Decimal,
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
    /// Product type: "future", "option", "spot", "future_combo", "option_combo"
    pub kind: DeribitProductType,
    /// Maker commission for instrument
    #[serde(deserialize_with = "deserialize_decimal")]
    pub maker_commission: Decimal,
    /// Maximal leverage for instrument (only for futures)
    pub max_leverage: Option<i64>,
    /// Maximal liquidation trade commission for instrument (only for futures)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub max_liquidation_commission: Option<Decimal>,
    /// Minimum amount for trading
    #[serde(deserialize_with = "deserialize_decimal")]
    pub min_trade_amount: Decimal,
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
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub strike: Option<Decimal>,
    /// Taker commission for instrument
    #[serde(deserialize_with = "deserialize_decimal")]
    pub taker_commission: Decimal,
    /// Specifies minimal price change and number of decimal places for instrument prices
    #[serde(deserialize_with = "deserialize_decimal")]
    pub tick_size: Decimal,
    /// Tick size steps for different price ranges
    pub tick_size_steps: Option<Vec<DeribitTickSizeStep>>,
}

/// Tick size step definition for price-dependent tick sizes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTickSizeStep {
    /// The price from which the increased tick size applies
    #[serde(deserialize_with = "deserialize_decimal")]
    pub above_price: Decimal,
    /// Tick size to be used above the price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub tick_size: Decimal,
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
    #[serde(deserialize_with = "deserialize_decimal")]
    pub equity: Decimal,
    /// Account balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub balance: Decimal,
    /// Available funds for trading
    #[serde(deserialize_with = "deserialize_decimal")]
    pub available_funds: Decimal,
    /// Margin balance (for derivatives)
    #[serde(deserialize_with = "deserialize_decimal")]
    pub margin_balance: Decimal,
    /// Initial margin (required for current positions)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub maintenance_margin: Option<Decimal>,
    /// Total profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_pl: Option<Decimal>,
    /// Session unrealized profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub session_upl: Option<Decimal>,
    /// Session realized profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub session_rpl: Option<Decimal>,
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
    #[serde(deserialize_with = "deserialize_decimal")]
    pub equity: Decimal,
    /// Account balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub balance: Decimal,
    /// Available funds for trading
    #[serde(deserialize_with = "deserialize_decimal")]
    pub available_funds: Decimal,
    /// Margin balance (for derivatives)
    #[serde(deserialize_with = "deserialize_decimal")]
    pub margin_balance: Decimal,
    /// Initial margin (required for current positions)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub maintenance_margin: Option<Decimal>,
    /// Total profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_pl: Option<Decimal>,
    /// Session unrealized profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub session_upl: Option<Decimal>,
    /// Session realized profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub session_rpl: Option<Decimal>,
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
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub futures_session_upl: Option<Decimal>,
    /// Futures session realized P&L
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub futures_session_rpl: Option<Decimal>,
    /// Options session unrealized P&L
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_session_upl: Option<Decimal>,
    /// Options session realized P&L
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_session_rpl: Option<Decimal>,
    /// Futures profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub futures_pl: Option<Decimal>,
    /// Options profit/loss
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_pl: Option<Decimal>,
    /// Options delta
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_delta: Option<Decimal>,
    /// Options gamma
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_gamma: Option<Decimal>,
    /// Options vega
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_vega: Option<Decimal>,
    /// Options theta
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_theta: Option<Decimal>,
    /// Options value
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub options_value: Option<Decimal>,
    /// Total delta across all positions
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub delta_total: Option<Decimal>,
    /// Projected delta total
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub projected_delta_total: Option<Decimal>,
    /// Projected initial margin
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub projected_initial_margin: Option<Decimal>,
    /// Projected maintenance margin
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub projected_maintenance_margin: Option<Decimal>,
    /// Estimated liquidation ratio
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub estimated_liquidation_ratio: Option<Decimal>,
    /// Available withdrawal funds
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub available_withdrawal_funds: Option<Decimal>,
    /// Spot reserve
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub spot_reserve: Option<Decimal>,
    /// Fee balance
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub fee_balance: Option<Decimal>,
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
    #[serde(deserialize_with = "deserialize_decimal")]
    pub amount: Decimal,
    /// Trade size in contract units (optional, may be absent in historical trades).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub contracts: Option<Decimal>,
    /// Direction of the trade: "buy" or "sell"
    pub direction: String,
    /// Index Price at the moment of trade (can be empty for some trade types).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub index_price: Option<Decimal>,
    /// Unique instrument identifier.
    pub instrument_name: String,
    /// Option implied volatility for the price (Option only).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub iv: Option<Decimal>,
    /// Optional field (only for trades caused by liquidation).
    #[serde(default)]
    pub liquidation: Option<String>,
    /// Mark Price at the moment of trade (can be empty for some trade types).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// Price in base currency.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
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
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub combo_trade_id: Option<Decimal>,
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
    /// List of bids as [price, amount] pairs (kept as f64 for performance)
    pub bids: Vec<[f64; 2]>,
    /// List of asks as [price, amount] pairs (kept as f64 for performance)
    pub asks: Vec<[f64; 2]>,
    /// The state of the order book: "open" or "closed"
    pub state: String,
    /// The current best bid price (null if there aren't any bids)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub best_bid_price: Option<Decimal>,
    /// The current best ask price (null if there aren't any asks)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub best_ask_price: Option<Decimal>,
    /// The order size of all best bids
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub best_bid_amount: Option<Decimal>,
    /// The order size of all best asks
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub best_ask_amount: Option<Decimal>,
    /// The mark price for the instrument
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// The price for the last trade
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub last_price: Option<Decimal>,
    /// Current index price
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub index_price: Option<Decimal>,
    /// The total amount of outstanding contracts
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub open_interest: Option<Decimal>,
    /// The maximum price for the future
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub max_price: Option<Decimal>,
    /// The minimum price for the future
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub min_price: Option<Decimal>,
    /// Current funding (perpetual only)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub current_funding: Option<Decimal>,
    /// Funding 8h (perpetual only)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub funding_8h: Option<Decimal>,
    /// The settlement price for the instrument (when state = open)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub settlement_price: Option<Decimal>,
    /// The settlement/delivery price for the instrument (when state = closed)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub delivery_price: Option<Decimal>,
    /// (Only for option) implied volatility for best bid
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub bid_iv: Option<Decimal>,
    /// (Only for option) implied volatility for best ask
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub ask_iv: Option<Decimal>,
    /// (Only for option) implied volatility for mark price
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub mark_iv: Option<Decimal>,
    /// Underlying price for implied volatility calculations (options only)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub underlying_price: Option<Decimal>,
    /// Name of the underlying future, or index_price (options only)
    #[serde(default)]
    pub underlying_index: Option<serde_json::Value>,
    /// Interest rate used in implied volatility calculations (options only)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub interest_rate: Option<Decimal>,
}

/// Book summary data from `/public/get_book_summary_by_currency` endpoint.
///
/// Each entry represents a single instrument's book summary including the
/// forward/underlying price used for ATM determination.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitBookSummary {
    /// Unique instrument identifier (e.g. "BTC-28MAR25-90000-C")
    pub instrument_name: String,
    /// The forward/underlying price for implied volatility calculations
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub underlying_price: Option<Decimal>,
    /// Name of the underlying future or index (e.g. "BTC-28MAR25" or "SYN.BTC-28MAR25")
    #[serde(default)]
    pub underlying_index: Option<String>,
    /// Mark price for the instrument
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// The time when the instrument was created (milliseconds since UNIX epoch)
    pub creation_timestamp: i64,
}

/// Ticker data from `/public/ticker` endpoint.
///
/// Only the fields needed for forward price extraction are included;
/// serde will ignore the many additional fields returned by the API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTicker {
    /// Unique instrument identifier (e.g., "BTC-28FEB26-65000-C")
    pub instrument_name: String,
    /// Underlying price for implied volatility calculations (options only)
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub underlying_price: Option<Decimal>,
    /// Name of the underlying future or index (e.g., "BTC-28MAR25" or "SYN.BTC-28MAR25")
    #[serde(default)]
    pub underlying_index: Option<String>,
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
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
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
    /// Product type: future, option, spot, etc.
    pub kind: DeribitProductType,
    /// Position size in currency units (for currency-quoted positions)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub size_currency: Option<Decimal>,
    /// Estimated liquidation price
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub estimated_liquidation_price: Option<Decimal>,
    /// Position delta (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub delta: Option<Decimal>,
    /// Position gamma (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub gamma: Option<Decimal>,
    /// Position vega (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub vega: Option<Decimal>,
    /// Position theta (for options)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub theta: Option<Decimal>,
    /// Settlement price (if settled)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub settlement_price: Option<Decimal>,
    /// Open orders margin for this position
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub open_orders_margin: Option<Decimal>,
    /// Average price in USD (for currency-margined contracts)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub average_price_usd: Option<Decimal>,
    /// Realized profit loss (session)
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub realized_profit_loss_session: Option<Decimal>,
    /// Floating profit loss in USD
    #[serde(
        default,
        serialize_with = "nautilus_core::serialization::serialize_optional_decimal",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_deserialize_public_trade_with_empty_mark_and_index_price() {
        let json = r#"{
            "amount": 1.0,
            "direction": "sell",
            "index_price": "",
            "instrument_name": "ETH-PERPETUAL",
            "mark_price": "",
            "price": 2968.3,
            "tick_direction": 0,
            "timestamp": 1766332040636,
            "trade_id": "ETH-123",
            "trade_seq": 1
        }"#;

        let trade: DeribitPublicTrade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.index_price, None);
        assert_eq!(trade.mark_price, None);
        assert_eq!(trade.price, dec!(2968.3));
    }

    #[rstest]
    fn test_deserialize_public_trade_with_missing_mark_and_index_price() {
        let json = r#"{
            "amount": 1.0,
            "direction": "sell",
            "instrument_name": "ETH-PERPETUAL",
            "price": 2968.3,
            "tick_direction": 0,
            "timestamp": 1766332040636,
            "trade_id": "ETH-123",
            "trade_seq": 1
        }"#;

        let trade: DeribitPublicTrade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.index_price, None);
        assert_eq!(trade.mark_price, None);
    }

    #[rstest]
    fn test_deserialize_public_trade_with_present_mark_and_index_price() {
        let json = r#"{
            "amount": 1.0,
            "direction": "sell",
            "index_price": 2967.73,
            "instrument_name": "ETH-PERPETUAL",
            "mark_price": 2968.01,
            "price": 2968.3,
            "tick_direction": 0,
            "timestamp": 1766332040636,
            "trade_id": "ETH-123",
            "trade_seq": 1
        }"#;

        let trade: DeribitPublicTrade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.index_price, Some(dec!(2967.73)));
        assert_eq!(trade.mark_price, Some(dec!(2968.01)));
    }
}

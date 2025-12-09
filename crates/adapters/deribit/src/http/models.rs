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

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeribitJsonRpcRequest<T> {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID for matching responses
    pub id: u64,
    /// API method name (e.g., "public/get_instruments")
    pub method: String,
    /// Method-specific parameters
    pub params: T,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeribitJsonRpcResponse<T> {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID matching the request (may be absent in error responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Success result (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    /// Error details (if error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DeribitError>,
    /// Whether this is from testnet
    #[serde(default)]
    pub testnet: bool,
    /// Server timestamp when request was received (microseconds)
    #[serde(rename = "usIn")]
    pub us_in: Option<u64>,
    /// Server timestamp when response was sent (microseconds)
    #[serde(rename = "usOut")]
    pub us_out: Option<u64>,
    /// Server processing time (microseconds)
    #[serde(rename = "usDiff")]
    pub us_diff: Option<u64>,
}

/// JSON-RPC 2.0 response payload (either success or error).
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeribitResponsePayload<T> {
    /// Successful response with result data
    Success { result: T },
    /// Error response with error details
    Error { error: DeribitError },
}

/// Deribit error details.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeribitError {
    /// Error code (e.g., 10050, 11029)
    pub code: i64,
    /// Human-readable error message
    pub message: String,
    /// Optional additional error data
    #[serde(default)]
    pub data: Option<serde_json::Value>,
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
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
}

/// Deribit option type.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
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
        }
    }
}

impl std::fmt::Display for DeribitCurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

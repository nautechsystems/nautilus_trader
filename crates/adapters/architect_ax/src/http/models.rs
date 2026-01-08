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

//! Data transfer objects for deserializing Ax HTTP API payloads.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{AxCandleWidth, AxInstrumentState, AxOrderSide, AxOrderStatus, AxTimeInForce},
    parse::{deserialize_decimal_or_zero, deserialize_optional_decimal_from_str},
};

/// Default instrument state when not provided by API.
fn default_instrument_state() -> AxInstrumentState {
    AxInstrumentState::Open
}

/// Response payload returned by `GET /whoami`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/whoami>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxWhoAmI {
    /// User account UUID.
    pub id: String,
    /// Username for the account.
    pub username: String,
    /// Account creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Whether two-factor authentication is enabled.
    pub enabled_2fa: bool,
    /// Whether the user has completed onboarding.
    pub is_onboarded: bool,
    /// Whether the account is frozen.
    pub is_frozen: bool,
    /// Whether the user has admin privileges.
    pub is_admin: bool,
    /// Whether the account is in close-only mode.
    pub is_close_only: bool,
    /// Maker fee rate.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maker_fee: Decimal,
    /// Taker fee rate.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub taker_fee: Decimal,
}

/// Individual instrument definition.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxInstrument {
    /// Trading symbol for the instrument.
    pub symbol: Ustr,
    /// Current trading state of the instrument (defaults to Open if not provided).
    #[serde(default = "default_instrument_state")]
    pub state: AxInstrumentState,
    /// Contract multiplier.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub multiplier: Decimal,
    /// Minimum order size.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub minimum_order_size: Decimal,
    /// Price tick size.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub tick_size: Decimal,
    /// Quote currency symbol.
    pub quote_currency: Ustr,
    // TODO: Rename to `funding_settlement_currency` once fixed
    /// Funding settlement currency.
    #[serde(alias = "funding_settlement_currency")]
    pub finding_settlement_currency: Ustr,
    /// Maintenance margin percentage.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maintenance_margin_pct: Decimal,
    /// Initial margin percentage.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_pct: Decimal,
    /// Current mark price for the contract (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub contract_mark_price: Option<Decimal>,
    /// Contract size (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub contract_size: Option<Decimal>,
    /// Instrument description (optional).
    #[serde(default)]
    pub description: Option<String>,
    /// Funding calendar schedule (optional).
    #[serde(default)]
    pub funding_calendar_schedule: Option<String>,
    /// Funding frequency (optional).
    #[serde(default)]
    pub funding_frequency: Option<String>,
    /// Lower cap for funding rate percentage (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub funding_rate_cap_lower_pct: Option<Decimal>,
    /// Upper cap for funding rate percentage (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub funding_rate_cap_upper_pct: Option<Decimal>,
    /// Lower deviation percentage for price bands (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub price_band_lower_deviation_pct: Option<Decimal>,
    /// Upper deviation percentage for price bands (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub price_band_upper_deviation_pct: Option<Decimal>,
    /// Price bands configuration (optional).
    #[serde(default)]
    pub price_bands: Option<String>,
    /// Price quotation format (optional).
    #[serde(default)]
    pub price_quotation: Option<String>,
    /// Underlying benchmark price (optional).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub underlying_benchmark_price: Option<Decimal>,
}

/// Response payload returned by `GET /instruments`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxInstrumentsResponse {
    /// List of instruments.
    pub instruments: Vec<AxInstrument>,
}

/// Individual balance entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-balances>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxBalance {
    /// Asset symbol.
    pub symbol: Ustr,
    /// Available balance amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub amount: Decimal,
}

/// Response payload returned by `GET /balances`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-balances>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxBalancesResponse {
    /// List of balances.
    pub balances: Vec<AxBalance>,
}

/// Individual position entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPosition {
    /// User account UUID.
    pub user_id: String,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Open quantity (positive for long, negative for short).
    pub open_quantity: i64,
    /// Open notional value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub open_notional: Decimal,
    /// Position timestamp.
    pub timestamp: DateTime<Utc>,
    /// Realized profit and loss.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub realized_pnl: Decimal,
}

/// Response payload returned by `GET /positions`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPositionsResponse {
    /// List of positions.
    pub positions: Vec<AxPosition>,
}

/// Individual ticker entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-ticker>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTicker {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Best bid price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub bid: Option<Decimal>,
    /// Best ask price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub ask: Option<Decimal>,
    /// Last trade price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub last: Option<Decimal>,
    /// Mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub mark: Option<Decimal>,
    /// Index price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub index: Option<Decimal>,
    /// 24-hour volume.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub volume_24h: Option<Decimal>,
    /// 24-hour high price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub high_24h: Option<Decimal>,
    /// 24-hour low price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub low_24h: Option<Decimal>,
    /// Ticker timestamp.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
}

/// Response payload returned by `GET /tickers`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTickersResponse {
    /// List of tickers.
    pub tickers: Vec<AxTicker>,
}

/// Response payload returned by `POST /authenticate`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxAuthenticateResponse {
    /// Session token for authenticated requests.
    pub token: String,
}

/// Response payload returned by `POST /place_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxPlaceOrderResponse {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Response payload returned by `POST /cancel_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxCancelOrderResponse {
    /// Whether the cancel request has been accepted.
    pub cxl_rx: bool,
}

/// Individual open order entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOpenOrder {
    /// Trade number.
    pub tn: i64,
    /// Timestamp (Unix epoch).
    pub ts: i64,
    /// Order side: "B" (buy) or "S" (sell).
    pub d: AxOrderSide,
    /// Order status.
    pub o: AxOrderStatus,
    /// Order ID.
    pub oid: String,
    /// Price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Quantity.
    pub q: i64,
    /// Remaining quantity.
    pub rq: i64,
    /// Symbol.
    pub s: Ustr,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// User ID.
    pub u: String,
    /// Executed quantity.
    pub xq: i64,
    /// Optional order tag.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Response payload returned by `GET /open_orders`.
///
/// Note: The response is a direct array, not wrapped in an object.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
pub type AxOpenOrdersResponse = Vec<AxOpenOrder>;

/// Individual fill/trade entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFill {
    /// Execution ID.
    pub execution_id: String,
    /// Fee amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub fee: Decimal,
    /// Whether this was a taker order.
    pub is_taker: bool,
    /// Execution price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub price: Decimal,
    /// Executed quantity.
    pub quantity: i64,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Execution timestamp.
    pub timestamp: DateTime<Utc>,
    /// User ID.
    pub user_id: String,
}

/// Response payload returned by `GET /fills`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFillsResponse {
    /// List of fills.
    pub fills: Vec<AxFill>,
}

/// Individual candle/OHLCV entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandle {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Candle timestamp.
    pub tn: DateTime<Utc>,
    /// Open price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub open: Decimal,
    /// High price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub high: Decimal,
    /// Low price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub low: Decimal,
    /// Close price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub close: Decimal,
    /// Buy volume.
    pub buy_volume: i64,
    /// Sell volume.
    pub sell_volume: i64,
    /// Total volume.
    pub volume: i64,
    /// Candle width/interval.
    pub width: AxCandleWidth,
}

/// Response payload returned by `GET /candles`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandlesResponse {
    /// List of candles.
    pub candles: Vec<AxCandle>,
}

/// Response payload returned by `GET /candles/current` and `GET /candles/last`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-current-candle>
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-last-candle>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandleResponse {
    /// The candle data.
    pub candle: AxCandle,
}

/// Individual funding rate entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFundingRate {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Timestamp in nanoseconds.
    pub timestamp_ns: i64,
    /// Funding rate.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub funding_rate: Decimal,
    /// Funding amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub funding_amount: Decimal,
    /// Benchmark price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub benchmark_price: Decimal,
    /// Settlement price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub settlement_price: Decimal,
}

/// Response payload returned by `GET /funding-rates`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFundingRatesResponse {
    /// List of funding rates.
    pub funding_rates: Vec<AxFundingRate>,
}

/// Per-symbol risk metrics.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPerSymbolRisk {
    /// Open quantity.
    pub open_quantity: i64,
    /// Open notional value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub open_notional: Decimal,
    /// Average entry price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub average_price: Decimal,
    /// Liquidation price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub liquidation_price: Option<Decimal>,
    /// Initial margin required.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub initial_margin_required: Option<Decimal>,
    /// Maintenance margin required.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maintenance_margin_required: Option<Decimal>,
    /// Unrealized P&L.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub unrealized_pnl: Option<Decimal>,
}

/// Risk snapshot data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxRiskSnapshot {
    /// USD account balance.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub balance_usd: Decimal,
    /// Total equity value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub equity: Decimal,
    /// Available initial margin.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_available: Decimal,
    /// Margin required for open orders.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_for_open_orders: Decimal,
    /// Margin required for positions.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_for_positions: Decimal,
    /// Total initial margin requirement.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_total: Decimal,
    /// Available maintenance margin.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maintenance_margin_available: Decimal,
    /// Required maintenance margin.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maintenance_margin_required: Decimal,
    /// Unrealized profit/loss.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub unrealized_pnl: Decimal,
    /// Snapshot timestamp.
    pub timestamp_ns: DateTime<Utc>,
    /// User identifier.
    pub user_id: String,
    /// Per-symbol risk data.
    #[serde(default)]
    pub per_symbol: HashMap<String, AxPerSymbolRisk>,
}

/// Response payload returned by `GET /risk-snapshot`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxRiskSnapshotResponse {
    /// The risk snapshot data.
    pub risk_snapshot: AxRiskSnapshot,
}

/// Individual transaction entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTransaction {
    /// Transaction amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub amount: Decimal,
    /// Unique event identifier.
    pub event_id: String,
    /// Asset symbol.
    pub symbol: Ustr,
    /// Transaction timestamp.
    pub timestamp: DateTime<Utc>,
    /// Type of transaction.
    pub transaction_type: Ustr,
    /// User identifier.
    pub user_id: String,
    /// Optional reference identifier.
    #[serde(default)]
    pub reference_id: Option<String>,
}

/// Response payload returned by `GET /transactions`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTransactionsResponse {
    /// List of transactions.
    pub transactions: Vec<AxTransaction>,
}

/// Request body for `POST /authenticate` using API key and secret.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateApiKeyRequest {
    /// API key.
    pub api_key: String,
    /// API secret.
    pub api_secret: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
    /// Optional 2FA code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp: Option<String>,
}

impl AuthenticateApiKeyRequest {
    /// Creates a new [`AuthenticateApiKeyRequest`].
    #[must_use]
    pub fn new(
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        expiration_seconds: i32,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            expiration_seconds,
            totp: None,
        }
    }

    /// Sets the optional 2FA code.
    #[must_use]
    pub fn with_totp(mut self, totp: impl Into<String>) -> Self {
        self.totp = Some(totp.into());
        self
    }
}

/// Request body for `POST /authenticate` using username and password.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateUserRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
    /// Optional 2FA code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp: Option<String>,
}

impl AuthenticateUserRequest {
    /// Creates a new [`AuthenticateUserRequest`].
    #[must_use]
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        expiration_seconds: i32,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            expiration_seconds,
            totp: None,
        }
    }

    /// Sets the optional 2FA code.
    #[must_use]
    pub fn with_totp(mut self, totp: impl Into<String>) -> Self {
        self.totp = Some(totp.into());
        self
    }
}

/// Request body for `POST /place_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
    /// Order side: "B" (buy) or "S" (sell).
    pub d: AxOrderSide,
    /// Order price as decimal string.
    pub p: String,
    /// Post-only flag (maker-or-cancel).
    pub po: bool,
    /// Order quantity in contracts.
    pub q: i64,
    /// Order symbol.
    pub s: String,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// Optional order tag (max 10 alphanumeric characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl PlaceOrderRequest {
    /// Creates a new [`PlaceOrderRequest`].
    #[must_use]
    pub fn new(
        side: AxOrderSide,
        price: impl Into<String>,
        quantity: i64,
        symbol: impl Into<String>,
        time_in_force: AxTimeInForce,
        post_only: bool,
    ) -> Self {
        Self {
            d: side,
            p: price.into(),
            po: post_only,
            q: quantity,
            s: symbol.into(),
            tif: time_in_force,
            tag: None,
        }
    }

    /// Sets the optional order tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}

/// Request body for `POST /cancel_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    /// Order ID to cancel.
    pub oid: String,
}

impl CancelOrderRequest {
    /// Creates a new [`CancelOrderRequest`].
    #[must_use]
    pub fn new(order_id: impl Into<String>) -> Self {
        Self {
            oid: order_id.into(),
        }
    }
}

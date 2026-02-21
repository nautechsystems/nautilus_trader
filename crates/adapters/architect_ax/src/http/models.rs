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

use ahash::AHashMap;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display};
use ustr::Ustr;

use crate::common::{
    enums::{
        AxCandleWidth, AxCategory, AxInstrumentState, AxOrderSide, AxOrderStatus, AxOrderType,
        AxTimeInForce,
    },
    parse::{
        deserialize_decimal_or_zero, deserialize_optional_decimal_from_str,
        serialize_decimal_as_str, serialize_optional_decimal_as_str,
    },
};

/// Default instrument state when not provided by API.
fn default_instrument_state() -> AxInstrumentState {
    AxInstrumentState::Open
}

/// Response payload returned by `GET /whoami`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/whoami>
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
/// - <https://docs.architect.exchange/api-reference/symbols-instruments/get-instruments>
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
    /// Funding settlement currency.
    pub funding_settlement_currency: Ustr,
    /// Instrument category (e.g. fx, equities, metals).
    #[serde(default)]
    pub category: Option<AxCategory>,
    /// Maintenance margin percentage.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maintenance_margin_pct: Decimal,
    /// Initial margin percentage.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_pct: Decimal,
    /// Contract mark price description (optional).
    #[serde(default)]
    pub contract_mark_price: Option<String>,
    /// Contract size description (optional).
    #[serde(default)]
    pub contract_size: Option<String>,
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
    /// Underlying benchmark price description (optional).
    #[serde(default)]
    pub underlying_benchmark_price: Option<String>,
}

/// Response payload returned by `GET /instruments`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/symbols-instruments/get-instruments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxInstrumentsResponse {
    /// List of instruments.
    pub instruments: Vec<AxInstrument>,
}

/// Individual balance entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-balances>
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
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-balances>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxBalancesResponse {
    /// List of balances.
    pub balances: Vec<AxBalance>,
}

/// Individual position entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPosition {
    /// User account UUID.
    pub user_id: String,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Signed quantity (positive for long, negative for short).
    pub signed_quantity: i64,
    /// Signed notional value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub signed_notional: Decimal,
    /// Position timestamp.
    pub timestamp: DateTime<Utc>,
    /// Realized profit and loss.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub realized_pnl: Decimal,
}

/// Response payload returned by `GET /positions`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPositionsResponse {
    /// List of positions.
    pub positions: Vec<AxPosition>,
}

/// Individual ticker entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-ticker>
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
/// - <https://docs.architect.exchange/api-reference/marketdata/get-tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTickersResponse {
    /// List of tickers.
    pub tickers: Vec<AxTicker>,
}

/// Response payload returned by `POST /authenticate`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxAuthenticateResponse {
    /// Session token for authenticated requests.
    pub token: String,
}

/// Response payload returned by `POST /place_order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxPlaceOrderResponse {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Response payload returned by `POST /cancel_order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxCancelOrderResponse {
    /// Whether the cancel request has been accepted.
    pub cxl_rx: bool,
}

/// Individual trade entry from the REST API.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-trades>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxRestTrade {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Nanosecond component of the timestamp.
    pub tn: i64,
    /// Trade price (decimal string).
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Trade quantity.
    pub q: i64,
    /// Symbol.
    pub s: Ustr,
    /// Trade direction (aggressor side).
    pub d: AxOrderSide,
}

/// Response payload returned by `GET /trades`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-trades>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxTradesResponse {
    /// List of trades.
    pub trades: Vec<AxRestTrade>,
}

/// Individual price level in the order book.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-book>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBookLevel {
    /// Price (decimal string).
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Quantity at this price level.
    pub q: i64,
    /// Individual order IDs (Level 3 only).
    #[serde(default)]
    pub o: Option<Vec<i64>>,
}

/// Order book snapshot.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-book>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBook {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Nanosecond component of the timestamp.
    pub tn: i64,
    /// Symbol.
    pub s: String,
    /// Bid levels (best to worst).
    pub b: Vec<AxBookLevel>,
    /// Ask levels (best to worst).
    pub a: Vec<AxBookLevel>,
}

/// Response payload returned by `GET /book`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-book>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBookResponse {
    /// The order book snapshot.
    pub book: AxBook,
}

/// Detailed order status from single-order lookup.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-order-status>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOrderStatusDetail {
    /// Trading symbol.
    pub symbol: Ustr,
    /// Order ID.
    pub order_id: String,
    /// Current order state.
    pub state: AxOrderStatus,
    /// Client order ID.
    #[serde(default)]
    pub clord_id: Option<u64>,
    /// Filled quantity.
    #[serde(default)]
    pub filled_quantity: Option<i64>,
    /// Remaining quantity.
    #[serde(default)]
    pub remaining_quantity: Option<i64>,
}

/// Response payload returned by `GET /order-status`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-order-status>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOrderStatusQueryResponse {
    /// The order status detail.
    pub status: AxOrderStatusDetail,
}

/// Reason for order rejection from the exchange.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-orders>
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq, Hash, AsRefStr, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum AxOrderRejectReason {
    CloseOnly,
    InsufficientMargin,
    MaxOpenOrdersExceeded,
    UnknownSymbol,
    ExchangeClosed,
    IncorrectQuantity,
    InvalidPriceIncrement,
    IncorrectOrderType,
    PriceOutOfBounds,
    NoLiquidity,
    InsufficientCreditLimit,
    #[serde(other)]
    Unknown,
}

/// Detailed order entry from historical orders query.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOrderDetail {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Nanosecond component.
    #[serde(default)]
    pub tn: i64,
    /// Order ID.
    pub oid: String,
    /// User ID.
    pub u: String,
    /// Symbol.
    pub s: Ustr,
    /// Price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Order quantity.
    pub q: u64,
    /// Executed quantity.
    pub xq: u64,
    /// Remaining quantity.
    pub rq: u64,
    /// Order state.
    pub o: AxOrderStatus,
    /// Order side.
    pub d: AxOrderSide,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// Client order ID.
    #[serde(default)]
    pub cid: Option<u64>,
    /// Reject reason.
    #[serde(default)]
    pub r: Option<AxOrderRejectReason>,
    /// Order tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Text note.
    #[serde(default)]
    pub txt: Option<String>,
}

/// Response payload returned by `GET /orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOrdersResponse {
    /// List of order details.
    pub orders: Vec<AxOrderDetail>,
    /// Total matching records (for pagination).
    pub total_count: i64,
    /// Applied limit.
    pub limit: i32,
    /// Applied offset.
    pub offset: i32,
}

/// Response payload returned by `POST /initial-margin-requirement`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/post-initial-margin-requirement>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxInitialMarginRequirementResponse {
    /// Initial margin requirement.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub im: Decimal,
}

/// Individual open order entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-open-orders>
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
    pub q: u64,
    /// Remaining quantity.
    pub rq: u64,
    /// Symbol.
    pub s: Ustr,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// User ID.
    pub u: String,
    /// Executed quantity.
    pub xq: u64,
    /// Optional client ID for order correlation.
    #[serde(default)]
    pub cid: Option<u64>,
    /// Optional order tag.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Response payload returned by `GET /open_orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-open-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOpenOrdersResponse {
    /// List of open orders.
    pub orders: Vec<AxOpenOrder>,
}

/// Individual fill/trade entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFill {
    /// Trade ID (execution identifier).
    pub trade_id: String,
    /// Order ID.
    pub order_id: String,
    /// Fee amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub fee: Decimal,
    /// Whether this was a taker order.
    pub is_taker: bool,
    /// Execution price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub price: Decimal,
    /// Executed quantity (always non-negative).
    pub quantity: u64,
    /// Order side.
    pub side: AxOrderSide,
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
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFillsResponse {
    /// List of fills.
    pub fills: Vec<AxFill>,
}

/// Individual candle/OHLCV entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandle {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Candle timestamp (Unix epoch seconds).
    pub ts: i64,
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
    pub buy_volume: u64,
    /// Sell volume.
    pub sell_volume: u64,
    /// Total volume.
    pub volume: u64,
    /// Candle width/interval.
    pub width: AxCandleWidth,
}

/// Response payload returned by `GET /candles`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandlesResponse {
    /// List of candles.
    pub candles: Vec<AxCandle>,
}

/// Response payload returned by `GET /candles/current` and `GET /candles/last`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-current-candle>
/// - <https://docs.architect.exchange/api-reference/marketdata/get-last-candle>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxCandleResponse {
    /// The candle data.
    pub candle: AxCandle,
}

/// Individual funding rate entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-funding-rates>
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
/// - <https://docs.architect.exchange/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFundingRatesResponse {
    /// List of funding rates.
    pub funding_rates: Vec<AxFundingRate>,
}

/// Per-symbol risk metrics.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxPerSymbolRisk {
    /// Signed quantity (positive for long, negative for short).
    pub signed_quantity: i64,
    /// Signed notional value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub signed_notional: Decimal,
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
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-risk-snapshot>
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
    pub per_symbol: AHashMap<String, AxPerSymbolRisk>,
}

/// Response payload returned by `GET /risk-snapshot`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxRiskSnapshotResponse {
    /// The risk snapshot data.
    pub risk_snapshot: AxRiskSnapshot,
}

/// Individual transaction entry.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-transactions>
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
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTransactionsResponse {
    /// List of transactions.
    pub transactions: Vec<AxTransaction>,
}

/// Request body for `POST /authenticate` using API key and secret.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateApiKeyRequest {
    /// API key.
    pub api_key: String,
    /// API secret.
    pub api_secret: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
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
        }
    }
}

/// Request body for `POST /authenticate` using username and password.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateUserRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
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
        }
    }
}

/// Request body for `POST /place_order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
    /// Order side: "B" (buy) or "S" (sell).
    pub d: AxOrderSide,
    /// Order price (limit price).
    #[serde(serialize_with = "serialize_decimal_as_str")]
    pub p: Decimal,
    /// Post-only flag (maker-or-cancel).
    pub po: bool,
    /// Order quantity in contracts.
    pub q: u64,
    /// Order symbol.
    pub s: Ustr,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// Optional order tag (max 10 alphanumeric characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Order type (defaults to LIMIT if not specified).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<AxOrderType>,
    /// Trigger price for stop-loss orders (required for STOP_LOSS_LIMIT).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub trigger_price: Option<Decimal>,
}

impl PlaceOrderRequest {
    /// Creates a new [`PlaceOrderRequest`] for a limit order.
    #[must_use]
    pub fn new(
        side: AxOrderSide,
        price: Decimal,
        quantity: u64,
        symbol: Ustr,
        time_in_force: AxTimeInForce,
        post_only: bool,
    ) -> Self {
        Self {
            d: side,
            p: price,
            po: post_only,
            q: quantity,
            s: symbol,
            tif: time_in_force,
            tag: None,
            order_type: None,
            trigger_price: None,
        }
    }

    /// Creates a new [`PlaceOrderRequest`] for a stop-loss limit order.
    #[must_use]
    pub fn new_stop_loss(
        side: AxOrderSide,
        limit_price: Decimal,
        trigger_price: Decimal,
        quantity: u64,
        symbol: Ustr,
        time_in_force: AxTimeInForce,
    ) -> Self {
        Self {
            d: side,
            p: limit_price,
            po: false,
            q: quantity,
            s: symbol,
            tif: time_in_force,
            tag: None,
            order_type: Some(AxOrderType::StopLossLimit),
            trigger_price: Some(trigger_price),
        }
    }

    /// Sets the optional order tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Sets the order type.
    #[must_use]
    pub fn with_order_type(mut self, order_type: AxOrderType) -> Self {
        self.order_type = Some(order_type);
        self
    }

    /// Sets the trigger price for stop orders.
    #[must_use]
    pub fn with_trigger_price(mut self, trigger_price: Decimal) -> Self {
        self.trigger_price = Some(trigger_price);
        self
    }
}

/// Request body for `POST /preview-aggressive-limit-order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/preview-aggressive-limit-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewAggressiveLimitOrderRequest {
    /// Trading symbol.
    pub symbol: Ustr,
    /// Order quantity in contracts.
    pub quantity: u64,
    /// Order side: "B" (buy) or "S" (sell).
    pub side: AxOrderSide,
}

impl PreviewAggressiveLimitOrderRequest {
    /// Creates a new [`PreviewAggressiveLimitOrderRequest`].
    #[must_use]
    pub fn new(symbol: Ustr, quantity: u64, side: AxOrderSide) -> Self {
        Self {
            symbol,
            quantity,
            side,
        }
    }
}

/// Response payload returned by `POST /preview-aggressive-limit-order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/preview-aggressive-limit-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxPreviewAggressiveLimitOrderResponse {
    /// Quantity that would be filled at the aggressive price.
    pub filled_quantity: u64,
    /// Quantity that cannot be filled (insufficient book depth).
    pub remaining_quantity: u64,
    /// The aggressive limit price ("take through" price), or None if no liquidity.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub limit_price: Option<Decimal>,
    /// Volume-weighted average price of expected fills.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub vwap: Option<Decimal>,
}

/// Request body for `POST /cancel_order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/cancel-order>
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

/// Request body for `POST /cancel_all_orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CancelAllOrdersRequest {
    /// Optional symbol filter - only cancel orders for this symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
    /// Optional execution venue filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_venue: Option<Ustr>,
}

impl CancelAllOrdersRequest {
    /// Creates a new [`CancelAllOrdersRequest`] to cancel all orders.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the symbol filter.
    #[must_use]
    pub fn with_symbol(mut self, symbol: Ustr) -> Self {
        self.symbol = Some(symbol);
        self
    }

    /// Sets the execution venue filter.
    #[must_use]
    pub fn with_venue(mut self, venue: Ustr) -> Self {
        self.execution_venue = Some(venue);
        self
    }
}

/// Response payload returned by `POST /cancel_all_orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxCancelAllOrdersResponse {
    /// Number of orders canceled.
    #[serde(default)]
    pub canceled_count: i64,
}

/// Request body for batch cancel orders.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchCancelOrdersRequest {
    /// List of order IDs to cancel.
    pub order_ids: Vec<String>,
}

impl BatchCancelOrdersRequest {
    /// Creates a new [`BatchCancelOrdersRequest`].
    #[must_use]
    pub fn new(order_ids: Vec<String>) -> Self {
        Self { order_ids }
    }
}

/// Response payload returned by batch cancel orders.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBatchCancelOrdersResponse {
    /// Number of orders successfully canceled.
    #[serde(default)]
    pub canceled_count: i64,
    /// Order IDs that failed to cancel.
    #[serde(default)]
    pub failed_order_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_deserialize_authenticate_response() {
        let json = include_str!("../../test_data/http_authenticate.json");
        let response: AxAuthenticateResponse = serde_json::from_str(json).unwrap();
        assert!(response.token.starts_with("test-token"));
    }

    #[rstest]
    fn test_deserialize_whoami_response() {
        let json = include_str!("../../test_data/http_get_whoami.json");
        let response: AxWhoAmI = serde_json::from_str(json).unwrap();
        assert_eq!(response.username, "test_user");
        assert!(response.enabled_2fa);
    }

    #[rstest]
    fn test_deserialize_instruments_response() {
        let json = include_str!("../../test_data/http_get_instruments.json");
        let response: AxInstrumentsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.instruments.len(), 3);
        assert_eq!(response.instruments[0].symbol, "EURUSD-PERP");
    }

    #[rstest]
    fn test_deserialize_balances_response() {
        let json = include_str!("../../test_data/http_get_balances.json");
        let response: AxBalancesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.balances.len(), 3);
        assert_eq!(response.balances[0].symbol, "USD");
    }

    #[rstest]
    fn test_deserialize_positions_response() {
        let json = include_str!("../../test_data/http_get_positions.json");
        let response: AxPositionsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.positions.len(), 2);
        assert_eq!(response.positions[0].symbol, "BTC-PERP");
        assert_eq!(response.positions[1].signed_quantity, -5);
    }

    #[rstest]
    fn test_deserialize_tickers_response() {
        let json = include_str!("../../test_data/http_get_tickers.json");
        let response: AxTickersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.tickers.len(), 3);
        assert_eq!(response.tickers[0].symbol, "EURUSD-PERP");
        assert!(response.tickers[0].bid.is_some());
        assert!(response.tickers[2].bid.is_none());
    }

    #[rstest]
    fn test_deserialize_funding_rates_response() {
        let json = include_str!("../../test_data/http_get_funding_rates.json");
        let response: AxFundingRatesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.funding_rates.len(), 2);
        assert_eq!(response.funding_rates[0].symbol, "JPYUSD-PERP");
    }

    #[rstest]
    fn test_deserialize_open_orders_response() {
        let json = include_str!("../../test_data/http_get_open_orders.json");
        let response: AxOpenOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.orders.len(), 2);
        assert_eq!(response.orders[0].oid, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(response.orders[0].d, AxOrderSide::Buy);
        assert_eq!(response.orders[0].o, AxOrderStatus::Accepted);
        assert_eq!(response.orders[1].xq, 300);
    }

    #[rstest]
    fn test_deserialize_fills_response() {
        let json = include_str!("../../test_data/http_get_fills.json");
        let response: AxFillsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.fills.len(), 2);
        assert_eq!(response.fills[0].side, AxOrderSide::Buy);
        assert!(response.fills[0].is_taker);
        assert!(!response.fills[1].is_taker);
    }

    #[rstest]
    fn test_deserialize_candles_response() {
        let json = include_str!("../../test_data/http_get_candles.json");
        let response: AxCandlesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.candles.len(), 2);
        assert_eq!(response.candles[0].symbol, "EURUSD-PERP");
        assert_eq!(response.candles[0].width, AxCandleWidth::Minutes1);
    }

    #[rstest]
    fn test_deserialize_candle_response() {
        let json = include_str!("../../test_data/http_get_candle.json");
        let response: AxCandleResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.candle.symbol, "EURUSD-PERP");
        assert_eq!(response.candle.width, AxCandleWidth::Minutes1);
    }

    #[rstest]
    fn test_deserialize_risk_snapshot_response() {
        let json = include_str!("../../test_data/http_get_risk_snapshot.json");
        let response: AxRiskSnapshotResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            response.risk_snapshot.user_id,
            "3c90c3cc-0d44-4b50-8888-8dd25736052a"
        );
        assert_eq!(response.risk_snapshot.per_symbol.len(), 2);
        assert!(
            response
                .risk_snapshot
                .per_symbol
                .contains_key("EURUSD-PERP")
        );
    }

    #[rstest]
    fn test_deserialize_transactions_response() {
        let json = include_str!("../../test_data/http_get_transactions.json");
        let response: AxTransactionsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.transactions.len(), 2);
        assert_eq!(response.transactions[0].transaction_type, "deposit");
        assert!(response.transactions[1].reference_id.is_none());
    }

    #[rstest]
    fn test_deserialize_preview_aggressive_limit_order_response() {
        let json = include_str!("../../test_data/http_preview_aggressive_limit_order.json");
        let response: AxPreviewAggressiveLimitOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.filled_quantity, 1000);
        assert_eq!(response.remaining_quantity, 0);
        assert!(response.limit_price.is_some());
        assert!(response.vwap.is_some());
    }

    #[rstest]
    fn test_deserialize_place_order_response() {
        let json = include_str!("../../test_data/http_place_order.json");
        let response: AxPlaceOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.oid, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[rstest]
    fn test_deserialize_cancel_order_response() {
        let json = include_str!("../../test_data/http_cancel_order.json");
        let response: AxCancelOrderResponse = serde_json::from_str(json).unwrap();
        assert!(response.cxl_rx);
    }

    #[rstest]
    fn test_deserialize_cancel_all_orders_response() {
        let json = include_str!("../../test_data/http_cancel_all_orders.json");
        let response: AxCancelAllOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.canceled_count, 3);
    }

    #[rstest]
    fn test_deserialize_batch_cancel_orders_response() {
        let json = include_str!("../../test_data/http_batch_cancel_orders.json");
        let response: AxBatchCancelOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.canceled_count, 2);
        assert_eq!(response.failed_order_ids.len(), 1);
    }

    #[rstest]
    fn test_deserialize_trades_response() {
        let json = include_str!("../../test_data/http_get_trades.json");
        let response: AxTradesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.trades.len(), 2);
        assert_eq!(response.trades[0].s, "EURUSD-PERP");
        assert_eq!(response.trades[0].d, AxOrderSide::Buy);
        assert_eq!(response.trades[0].q, 100);
        assert_eq!(response.trades[1].d, AxOrderSide::Sell);
    }

    #[rstest]
    fn test_deserialize_book_response() {
        let json = include_str!("../../test_data/http_get_book.json");
        let response: AxBookResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.book.s, "EURUSD-PERP");
        assert_eq!(response.book.b.len(), 3);
        assert_eq!(response.book.a.len(), 3);
        assert_eq!(response.book.b[0].q, 500);
        assert_eq!(response.book.a[0].q, 400);
    }

    #[rstest]
    fn test_deserialize_order_status_query_response() {
        let json = include_str!("../../test_data/http_get_order_status.json");
        let response: AxOrderStatusQueryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status.symbol, "EURUSD-PERP");
        assert_eq!(response.status.order_id, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(response.status.state, AxOrderStatus::PartiallyFilled);
        assert_eq!(response.status.clord_id, Some(12345));
        assert_eq!(response.status.filled_quantity, Some(300));
        assert_eq!(response.status.remaining_quantity, Some(700));
    }

    #[rstest]
    fn test_deserialize_orders_response() {
        let json = include_str!("../../test_data/http_get_orders.json");
        let response: AxOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.orders.len(), 2);
        assert_eq!(response.total_count, 2);
        assert_eq!(response.orders[0].o, AxOrderStatus::PartiallyFilled);
        assert_eq!(response.orders[0].xq, 300);
        assert_eq!(response.orders[1].o, AxOrderStatus::Filled);
        assert_eq!(response.orders[1].d, AxOrderSide::Sell);
    }

    #[rstest]
    fn test_deserialize_initial_margin_requirement_response() {
        let json = include_str!("../../test_data/http_initial_margin_requirement.json");
        let response: AxInitialMarginRequirementResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.im, Decimal::new(125050, 2));
    }
}

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
use jiff::{Timestamp, civil::Date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display};
use ustr::Ustr;

use crate::common::{
    enums::{
        AxCandleWidth, AxCategory, AxFundingSlotStatus, AxFundingVariant, AxInstrumentState,
        AxOrderSide, AxOrderStatus, AxTimeInForce,
    },
    parse::{
        deserialize_decimal_or_zero, deserialize_optional_decimal,
        deserialize_optional_decimal_from_str, serialize_decimal_as_str,
        serialize_optional_decimal_as_str,
    },
};

/// Default instrument state when not provided by API.
fn default_instrument_state() -> AxInstrumentState {
    AxInstrumentState::Open
}

/// An account entry within a [`AxWhoAmI`] response.
///
/// Fee rates and close-only state are per account rather than per user.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/whoami>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxWhoAmIAccount {
    /// Account identifier.
    pub id: String,
    /// Account display name.
    pub name: String,
    /// Whether the account is in close-only mode.
    pub is_close_only: bool,
    /// Maker fee rate; absent when the venue supplies no rate, which is distinct from zero.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maker_fee: Option<Decimal>,
    /// Taker fee rate; absent when the venue supplies no rate, which is distinct from zero.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub taker_fee: Option<Decimal>,
    /// Whether the account may list its own state.
    pub can_list: bool,
    /// Whether the account may read venue state.
    pub can_read: bool,
    /// Whether the account may set risk limits.
    pub can_set_limits: bool,
    /// Whether the account may reduce or close existing positions.
    pub can_reduce_or_close: bool,
    /// Whether the account may open new positions.
    pub can_trade: bool,
}

/// Response payload returned by `GET /whoami`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/user-management/whoami>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxWhoAmI {
    /// User identifier.
    pub id: String,
    /// Username for the account.
    pub username: String,
    /// Account creation timestamp.
    pub created_at: Timestamp,
    /// Whether two-factor authentication is required.
    pub require_2fa: bool,
    /// Whether the user has completed onboarding.
    pub is_onboarded: bool,
    /// Whether the account is frozen.
    pub is_frozen: bool,
    /// Whether the user has admin privileges.
    pub is_admin: bool,
    /// Accounts the credentials can act on.
    pub accounts: Vec<AxWhoAmIAccount>,
    /// Human-readable alias for the user (optional).
    #[serde(default)]
    pub pseudonym: Option<String>,
    /// Reference code for fiat deposits (optional).
    #[serde(default)]
    pub fiat_deposit_code: Option<String>,
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
    /// Umbrella product shared by sibling contracts.
    #[serde(default)]
    pub product: Option<Ustr>,
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
    pub category: AxCategory,
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
    /// Contract expiration; absent for perpetual contracts.
    #[serde(default)]
    pub expiration: Option<Timestamp>,
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
    /// Account identifier.
    pub account_id: Ustr,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Signed quantity (positive for long, negative for short).
    pub signed_quantity: i64,
    /// Signed notional value.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub signed_notional: Decimal,
    /// Position timestamp.
    pub timestamp: Timestamp,
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
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Best bid price.
    #[serde(
        default,
        rename = "bp",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub bid: Option<Decimal>,
    /// Best ask price.
    #[serde(
        default,
        rename = "ap",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub ask: Option<Decimal>,
    /// Last trade price.
    #[serde(
        default,
        rename = "p",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub last: Option<Decimal>,
    /// Mark price.
    #[serde(
        default,
        rename = "m",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub mark: Option<Decimal>,
    /// 24-hour volume.
    #[serde(
        default,
        rename = "v",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub volume_24h: Option<Decimal>,
    /// 24-hour high price.
    #[serde(
        default,
        rename = "h",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub high_24h: Option<Decimal>,
    /// 24-hour low price.
    #[serde(
        default,
        rename = "l",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub low_24h: Option<Decimal>,
    /// Timestamp seconds.
    #[serde(default)]
    pub ts: Option<i64>,
    /// Timestamp nanosecond component.
    #[serde(default)]
    pub tn: Option<i64>,
    /// Last trade quantity.
    #[serde(default, rename = "q")]
    pub last_quantity: Option<u64>,
    /// Open interest.
    #[serde(default, rename = "oi")]
    pub open_interest: Option<i64>,
    /// Instrument state.
    #[serde(default, rename = "i")]
    pub instrument_state: Option<AxInstrumentState>,
    /// Price band lower limit.
    #[serde(
        default,
        rename = "pl",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub price_band_lower: Option<Decimal>,
    /// Price band upper limit.
    #[serde(
        default,
        rename = "pu",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub price_band_upper: Option<Decimal>,
    /// Last settlement price.
    #[serde(
        default,
        rename = "lsp",
        deserialize_with = "deserialize_optional_decimal"
    )]
    pub last_settlement_price: Option<Decimal>,
    /// Last settlement time as epoch seconds.
    #[serde(default, rename = "lst")]
    pub last_settlement_time: Option<i64>,
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
    /// Total matching records.
    pub total_count: i64,
    /// Applied limit.
    pub limit: i32,
    /// Applied offset.
    pub offset: i32,
}

/// Response payload returned by `GET /ticker`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-ticker>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxTickerResponse {
    /// The ticker data.
    pub ticker: AxTicker,
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

/// Response payload returned by `POST /place-order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxPlaceOrderResponse {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Response payload returned by `POST /cancel-order`.
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
    pub s: Ustr,
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
    /// Reject reason.
    #[serde(default)]
    pub reject_reason: Option<AxOrderRejectReason>,
    /// Reject message.
    #[serde(default)]
    pub reject_message: Option<String>,
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
    /// Account ID.
    #[serde(default)]
    pub aid: Option<String>,
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
    /// Whether the order is post-only.
    #[serde(default)]
    pub po: bool,
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
    #[serde(default)]
    pub total_count: Option<i64>,
    /// Applied limit.
    #[serde(default)]
    pub limit: Option<i32>,
    /// Applied offset.
    #[serde(default)]
    pub offset: Option<i32>,
    /// Next page cursor.
    #[serde(default)]
    pub next_cursor: Option<String>,
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
    /// Whether the order is post-only.
    #[serde(default)]
    pub po: bool,
}

/// Response payload returned by `GET /open-orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-open-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxOpenOrdersResponse {
    /// List of open orders.
    pub orders: Vec<AxOpenOrder>,
    /// Total matching records.
    pub total_count: i64,
    /// Applied limit.
    pub limit: i32,
    /// Applied offset.
    pub offset: i32,
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
    pub order_id: Option<String>,
    /// Fee amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub fee: Decimal,
    /// Whether this was a taker order.
    pub is_taker: bool,
    /// Whether this fill was generated by an off-book block trade.
    pub is_block_trade: Option<bool>,
    /// Whether this fill was generated by final contract settlement.
    pub is_final_settlement: Option<bool>,
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
    pub timestamp: Timestamp,
    /// Account identifier.
    pub account_id: Ustr,
    /// Realized PnL for this fill.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub realized_pnl: Option<Decimal>,
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
    /// Total matching records, when supplied by AX.
    #[serde(default)]
    pub total_count: Option<i64>,
    /// Applied limit, when supplied by AX.
    #[serde(default)]
    pub limit: Option<i32>,
    /// Cursor for the next page, when one exists.
    #[serde(default)]
    pub next_cursor: Option<String>,
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
    /// Total matching records, when supplied by AX.
    #[serde(default)]
    pub total_count: Option<i64>,
    /// Applied limit, when supplied by AX.
    #[serde(default)]
    pub limit: Option<i32>,
    /// Cursor for the next page, when one exists.
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// One funding slot of a trading day, as returned by `GET /funding-slots`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-funding-slots>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFundingSlot {
    /// 1-based position within the day's schedule.
    pub index: i32,
    /// Scheduled settlement time of the slot.
    pub funding_time: Timestamp,
    /// Slot settlement state.
    pub status: AxFundingSlotStatus,
    /// True when the rate was clamped by the symbol's funding rate cap.
    pub capped: bool,
    /// Mark-price TWAP over the slot, when available.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub mark_twap: Option<Decimal>,
    /// Underlying-price TWAP over the slot, when available.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub underlying_twap: Option<Decimal>,
    /// Premium of the mark TWAP over the underlying TWAP, in basis points.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub premium_bps: Option<Decimal>,
    /// Slot funding rate in basis points; positive means longs pay shorts.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub funding_rate_bps: Option<Decimal>,
    /// Why a skipped slot did not settle; present only on skipped slots.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Response payload returned by `GET /funding-slots`.
///
/// A full trading day of funding slots with running totals. `daily_close`
/// symbols report a single slot.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-funding-slots>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AxFundingSlotsResponse {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Trading day the schedule covers.
    pub date: Date,
    /// IANA name of the funding schedule's timezone.
    pub timezone: String,
    /// How the symbol's funding accrues over the day.
    pub variant: AxFundingVariant,
    /// Number of funding slots scheduled on `date`; 0 on holidays and weekends.
    pub interval_count: i32,
    /// Per-slot cap on the funding rate in basis points, when configured.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cap_bps: Option<Decimal>,
    /// Funding slots for the day.
    pub slots: Vec<AxFundingSlot>,
    /// Sum of realized slot rates so far, in basis points.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub realized_sum_bps: Decimal,
    /// Projected end-of-day total in basis points: realized plus remaining projections.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub projected_eod_bps: Decimal,
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
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub average_price: Option<Decimal>,
    /// Liquidation price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub liquidation_price: Option<Decimal>,
    /// Initial margin required for the position.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_position: Decimal,
    /// Initial margin required for open orders.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_open_orders: Decimal,
    /// Total initial margin required.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub initial_margin_required_total: Decimal,
    /// Maintenance margin required.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub maintenance_margin_required: Decimal,
    /// Unrealized P&L.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub unrealized_pnl: Decimal,
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
    pub timestamp_ns: Timestamp,
    /// Account identifier.
    pub account_id: Ustr,
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
    /// Account identifier.
    pub account_id: Ustr,
    /// Transaction amount.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub amount: Decimal,
    /// Unique event identifier.
    pub event_id: String,
    /// Asset symbol.
    pub symbol: Ustr,
    /// Transaction timestamp.
    pub timestamp: Timestamp,
    /// Type of transaction.
    pub transaction_type: Ustr,
    /// User who initiated the transaction, when available.
    #[serde(default)]
    pub initiated_by_user_id: Option<String>,
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
    /// Total matching records.
    #[serde(default)]
    pub total_count: Option<i64>,
    /// Applied limit.
    #[serde(default)]
    pub limit: Option<i32>,
    /// Next page cursor.
    #[serde(default)]
    pub next_cursor: Option<String>,
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

/// Request body for `POST /place-order`.
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
}

impl PlaceOrderRequest {
    /// Creates a new [`PlaceOrderRequest`] for the AX priced order shape.
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
        }
    }

    /// Sets the optional order tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
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

/// Request body for `POST /cancel-order`.
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

/// Request body for `POST /replace-order`.
///
/// Replaces (amends) an existing order. Unspecified optional fields inherit
/// from the original order. The exchange returns a new order ID.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/replace-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplaceOrderRequest {
    /// Order ID to replace.
    pub oid: String,
    /// New limit price (optional, inherits from original if omitted).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub p: Option<Decimal>,
    /// New quantity in contracts (optional, inherits from original if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<u64>,
    /// New post-only flag (optional, inherits from original if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub po: Option<bool>,
    /// New time-in-force (optional, inherits from original if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tif: Option<AxTimeInForce>,
}

impl ReplaceOrderRequest {
    /// Creates a new [`ReplaceOrderRequest`] with only the order ID.
    ///
    /// Use the builder methods to set the fields to amend.
    #[must_use]
    pub fn new(order_id: impl Into<String>) -> Self {
        Self {
            oid: order_id.into(),
            p: None,
            q: None,
            po: None,
            tif: None,
        }
    }

    /// Sets the new limit price.
    #[must_use]
    pub fn with_price(mut self, price: Decimal) -> Self {
        self.p = Some(price);
        self
    }

    /// Sets the new quantity.
    #[must_use]
    pub fn with_quantity(mut self, quantity: u64) -> Self {
        self.q = Some(quantity);
        self
    }
}

/// Response payload returned by `POST /replace-order`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/replace-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxReplaceOrderResponse {
    /// New order ID assigned to the replacement order.
    pub oid: String,
}

/// Request body for `POST /cancel-all-orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CancelAllOrdersRequest {
    /// Optional account ID. AX infers the session account when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Ustr>,
    /// Optional symbol filter - only cancel orders for this symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
}

impl CancelAllOrdersRequest {
    /// Creates a new [`CancelAllOrdersRequest`] to cancel all orders.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the account filter.
    #[must_use]
    pub fn with_account_id(mut self, account_id: Ustr) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Sets the symbol filter.
    #[must_use]
    pub fn with_symbol(mut self, symbol: Ustr) -> Self {
        self.symbol = Some(symbol);
        self
    }
}

/// Response payload returned by `POST /cancel-all-orders`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxCancelAllOrdersResponse {}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_deserialize_authenticate_response() {
        let json = include_str!("../../test_data/http_authenticate.json");
        let response: AxAuthenticateResponse = serde_json::from_str(json).unwrap();
        assert!(response.token.starts_with("test-token"));
    }

    #[rstest]
    fn test_serialize_cancel_all_orders_request() {
        let request = CancelAllOrdersRequest::new()
            .with_account_id(Ustr::from("account-1"))
            .with_symbol(Ustr::from("XAU-PERP"));

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["account_id"], "account-1");
        assert_eq!(value["symbol"], "XAU-PERP");
        assert!(value.get("execution_venue").is_none());
    }

    #[rstest]
    fn test_deserialize_whoami_response() {
        let json = include_str!("../../test_data/http_get_whoami.json");

        let response: AxWhoAmI = serde_json::from_str(json).unwrap();

        assert_eq!(response.id, "01JBXR-7QK2-0000");
        assert_eq!(response.username, "trader@example.com");
        assert_eq!(response.pseudonym.as_deref(), Some("quiet-amber-heron"));
        assert_eq!(
            response.created_at,
            "2025-12-18T02:20:42.675817Z".parse::<Timestamp>().unwrap()
        );
        assert!(!response.require_2fa);
        assert!(response.is_onboarded);
        assert!(!response.is_frozen);
        assert!(!response.is_admin);
        assert_eq!(
            response.fiat_deposit_code.as_deref(),
            Some("01JBXR7QK20000Y")
        );
        assert_eq!(response.accounts.len(), 1);

        let account = &response.accounts[0];

        assert_eq!(account.id, "01JBXR-7QK2-0000");
        assert_eq!(account.name, "trader@example.com");
        assert!(!account.is_close_only);
        assert_eq!(account.maker_fee, Some(dec!(0.0002)));
        assert_eq!(account.taker_fee, Some(dec!(0.0025)));
        assert!(account.can_list);
        assert!(account.can_read);
        assert!(account.can_set_limits);
        assert!(account.can_reduce_or_close);
        assert!(account.can_trade);
    }

    #[rstest]
    #[case(json!(""), None)]
    #[case(json!(null), None)]
    #[case(json!("0"), Some(Decimal::ZERO))]
    #[case(json!("0.0002"), Some(dec!(0.0002)))]
    fn test_deserialize_whoami_account_fee_distinguishes_absent_from_zero(
        #[case] wire_value: serde_json::Value,
        #[case] expected: Option<Decimal>,
    ) {
        // A zero rate is valid, so an absent rate must not deserialize to zero
        let json = json!({
            "id": "01JBXR-7QK2-0000",
            "name": "trader@example.com",
            "is_close_only": false,
            "maker_fee": wire_value,
            "taker_fee": wire_value,
            "can_list": true,
            "can_read": true,
            "can_set_limits": true,
            "can_reduce_or_close": true,
            "can_trade": true,
        })
        .to_string();

        let account: AxWhoAmIAccount = serde_json::from_str(&json).unwrap();

        assert_eq!(account.maker_fee, expected);
        assert_eq!(account.taker_fee, expected);
    }

    #[rstest]
    fn test_deserialize_whoami_account_rejects_malformed_fee() {
        let json = json!({
            "id": "01JBXR-7QK2-0000",
            "name": "trader@example.com",
            "is_close_only": false,
            "maker_fee": "not-a-decimal",
            "taker_fee": "0.0025",
            "can_list": true,
            "can_read": true,
            "can_set_limits": true,
            "can_reduce_or_close": true,
            "can_trade": true,
        })
        .to_string();

        let error = serde_json::from_str::<AxWhoAmIAccount>(&json).unwrap_err();

        assert!(
            error.to_string().contains("Invalid decimal"),
            "unexpected error: {error}"
        );
    }

    #[rstest]
    fn test_deserialize_whoami_response_without_optional_profile_fields() {
        let json = json!({
            "id": "01JBXR-7QK2-0001",
            "username": "sub@example.com",
            "created_at": "2025-12-18T02:20:42.675817Z",
            "is_onboarded": true,
            "is_frozen": false,
            "is_admin": false,
            "require_2fa": true,
            "accounts": [],
        })
        .to_string();

        let response: AxWhoAmI = serde_json::from_str(&json).unwrap();

        assert!(response.require_2fa);
        assert_eq!(response.pseudonym, None);
        assert_eq!(response.fiat_deposit_code, None);
        assert!(response.accounts.is_empty());
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
        assert_eq!(response.total_count, 3);
        assert_eq!(response.limit, 100);
        assert_eq!(response.offset, 0);
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
    fn test_deserialize_funding_slots_response() {
        let json = include_str!("../../test_data/http_get_funding_slots.json");
        let response: AxFundingSlotsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.symbol, "EURUSD-PERP");
        assert_eq!(response.date, Date::new(2026, 7, 6).unwrap());
        assert_eq!(response.timezone, "America/New_York");
        assert_eq!(response.variant, AxFundingVariant::IntradayTwap);
        assert_eq!(response.interval_count, 4);
        assert_eq!(
            response.cap_bps.map(|d| d.to_string()),
            Some("5.0".to_string())
        );
        assert_eq!(response.slots.len(), 4);

        let first = &response.slots[0];
        assert_eq!(first.index, 1);
        assert_eq!(first.status, AxFundingSlotStatus::Realized);
        assert!(!first.capped);
        assert_eq!(
            first.funding_rate_bps.map(|d| d.to_string()),
            Some("0.0921".to_string())
        );
        assert!(first.reason.is_none());

        let capped = &response.slots[1];
        assert!(capped.capped);
        assert_eq!(
            capped.funding_rate_bps.map(|d| d.to_string()),
            Some("5.0000".to_string())
        );

        let projected = &response.slots[2];
        assert_eq!(projected.status, AxFundingSlotStatus::Projected);

        let skipped = &response.slots[3];
        assert_eq!(skipped.status, AxFundingSlotStatus::Skipped);
        assert!(skipped.mark_twap.is_none());
        assert!(skipped.funding_rate_bps.is_none());
        assert_eq!(skipped.reason.as_deref(), Some("holiday"));

        assert_eq!(response.realized_sum_bps.to_string(), "5.0921");
        assert_eq!(response.projected_eod_bps.to_string(), "5.1842");
    }

    #[rstest]
    fn test_funding_variant_and_slot_status_deserialization() {
        let daily: AxFundingVariant =
            serde_json::from_value(serde_json::json!("daily_close")).unwrap();
        let twap: AxFundingVariant =
            serde_json::from_value(serde_json::json!("intraday_twap")).unwrap();
        assert_eq!(daily, AxFundingVariant::DailyClose);
        assert_eq!(twap, AxFundingVariant::IntradayTwap);

        let statuses = [
            ("realized", AxFundingSlotStatus::Realized),
            ("projected", AxFundingSlotStatus::Projected),
            ("skipped", AxFundingSlotStatus::Skipped),
            ("pending", AxFundingSlotStatus::Pending),
        ];

        for (raw, expected) in statuses {
            let parsed: AxFundingSlotStatus =
                serde_json::from_value(serde_json::json!(raw)).unwrap();
            assert_eq!(parsed, expected);
        }
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
        assert_eq!(response.total_count, 2);
        assert_eq!(response.limit, 100);
        assert_eq!(response.offset, 0);
    }

    #[rstest]
    fn test_deserialize_fills_response() {
        let json = include_str!("../../test_data/http_get_fills.json");
        let response: AxFillsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.fills.len(), 2);
        assert_eq!(response.fills[0].side, AxOrderSide::Buy);
        assert!(response.fills[0].is_taker);
        assert!(!response.fills[1].is_taker);
        assert_eq!(response.fills[0].is_block_trade, Some(false));
        assert_eq!(response.fills[0].is_final_settlement, Some(false));
        assert_eq!(response.total_count, Some(2));
        assert_eq!(response.limit, Some(100));
        assert_eq!(response.next_cursor, None);
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
            response.risk_snapshot.account_id,
            Ustr::from("3c90c3cc-0d44-4b50-8888-8dd25736052a")
        );
        assert_eq!(response.risk_snapshot.per_symbol.len(), 2);
        assert!(
            response
                .risk_snapshot
                .per_symbol
                .contains_key("EURUSD-PERP")
        );
        assert_eq!(
            response.risk_snapshot.per_symbol["GBPUSD-PERP"].average_price,
            None
        );
    }

    #[rstest]
    fn test_deserialize_transactions_response() {
        let json = include_str!("../../test_data/http_get_transactions.json");
        let response: AxTransactionsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.transactions.len(), 2);
        assert_eq!(response.total_count, Some(2));
        assert_eq!(response.limit, Some(100));
        assert_eq!(response.transactions[0].account_id, Ustr::from("account-1"));
        assert_eq!(response.transactions[0].transaction_type, "deposit");
        assert!(response.transactions[0].initiated_by_user_id.is_some());
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
        let _response: AxCancelAllOrdersResponse = serde_json::from_str(json).unwrap();
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
        assert_eq!(response.status.reject_reason, None);
        assert_eq!(response.status.reject_message, None);
    }

    #[rstest]
    fn test_deserialize_orders_response() {
        let json = include_str!("../../test_data/http_get_orders.json");
        let response: AxOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.orders.len(), 2);
        assert_eq!(response.total_count, Some(2));
        assert_eq!(response.limit, Some(100));
        assert_eq!(response.next_cursor, None);
        assert_eq!(response.orders[0].aid.as_deref(), Some("account-1"));
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

    #[rstest]
    fn test_deserialize_replace_order_response() {
        let json = include_str!("../../test_data/http_replace_order.json");
        let response: AxReplaceOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.oid, "O-01ARZ3NDEKTSV4RRFFQ69G5NEW");
    }

    #[rstest]
    fn test_replace_order_request_serialization() {
        let request = ReplaceOrderRequest::new("O-01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .with_price(Decimal::new(10550, 4))
            .with_quantity(200);

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["oid"], "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(json["p"], "1.0550");
        assert_eq!(json["q"], 200);
        assert!(json.get("po").is_none());
        assert!(json.get("tif").is_none());
        assert!(json.get("trigger_price").is_none());
    }

    #[rstest]
    fn test_replace_order_request_minimal() {
        let request = ReplaceOrderRequest::new("O-TEST");
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["oid"], "O-TEST");
        assert!(json.get("p").is_none());
        assert!(json.get("q").is_none());
    }
}

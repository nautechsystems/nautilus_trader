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

//! Enumerations that model Ax string enums across HTTP and WebSocket payloads.

use nautilus_model::enums::{
    AggressorSide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use super::consts::{
    AX_HTTP_SANDBOX_URL, AX_HTTP_URL, AX_ORDERS_SANDBOX_URL, AX_ORDERS_URL, AX_WS_PRIVATE_URL,
    AX_WS_PUBLIC_URL, AX_WS_SANDBOX_PRIVATE_URL, AX_WS_SANDBOX_PUBLIC_URL,
};

/// AX Exchange API environment.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxEnvironment {
    /// Sandbox/test environment.
    #[default]
    Sandbox,
    /// Production/live environment.
    Production,
}

impl AxEnvironment {
    /// Returns the HTTP API base URL for this environment.
    #[must_use]
    pub const fn http_url(&self) -> &'static str {
        match self {
            Self::Sandbox => AX_HTTP_SANDBOX_URL,
            Self::Production => AX_HTTP_URL,
        }
    }

    /// Returns the Orders API base URL for this environment.
    #[must_use]
    pub const fn orders_url(&self) -> &'static str {
        match self {
            Self::Sandbox => AX_ORDERS_SANDBOX_URL,
            Self::Production => AX_ORDERS_URL,
        }
    }

    /// Returns the market data WebSocket URL for this environment.
    #[must_use]
    pub const fn ws_md_url(&self) -> &'static str {
        match self {
            Self::Sandbox => AX_WS_SANDBOX_PUBLIC_URL,
            Self::Production => AX_WS_PUBLIC_URL,
        }
    }

    /// Returns the orders WebSocket URL for this environment.
    #[must_use]
    pub const fn ws_orders_url(&self) -> &'static str {
        match self {
            Self::Sandbox => AX_WS_SANDBOX_PRIVATE_URL,
            Self::Production => AX_WS_PRIVATE_URL,
        }
    }
}

/// Instrument state as returned by the AX Exchange API.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxInstrumentState {
    /// Instrument is in pre-open state.
    PreOpen,
    /// Instrument is open for trading.
    Open,
    /// Instrument trading is suspended.
    Suspended,
    /// Instrument has been delisted.
    Delisted,
    /// Instrument state is unknown.
    Unknown,
}

/// Order side for trading operations.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxOrderSide {
    /// Buy order.
    #[serde(rename = "B")]
    #[strum(serialize = "B")]
    Buy,
    /// Sell order.
    #[serde(rename = "S")]
    #[strum(serialize = "S")]
    Sell,
}

impl From<AxOrderSide> for AggressorSide {
    fn from(side: AxOrderSide) -> Self {
        match side {
            AxOrderSide::Buy => Self::Buyer,
            AxOrderSide::Sell => Self::Seller,
        }
    }
}

impl From<AxOrderSide> for OrderSide {
    fn from(side: AxOrderSide) -> Self {
        match side {
            AxOrderSide::Buy => Self::Buy,
            AxOrderSide::Sell => Self::Sell,
        }
    }
}

impl From<AxOrderSide> for PositionSide {
    fn from(side: AxOrderSide) -> Self {
        match side {
            AxOrderSide::Buy => Self::Long,
            AxOrderSide::Sell => Self::Short,
        }
    }
}

impl TryFrom<OrderSide> for AxOrderSide {
    type Error = &'static str;

    fn try_from(side: OrderSide) -> Result<Self, Self::Error> {
        match side {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => Err("Invalid order side for AX"),
        }
    }
}

/// Order status as returned by the AX Exchange API.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxOrderStatus {
    /// Order is pending submission.
    Pending,
    /// Order has been accepted by the exchange (OPEN state).
    Accepted,
    /// Order has been partially filled.
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order cancellation is in progress.
    Canceling,
    /// Order has been canceled.
    Canceled,
    /// Order has been rejected.
    Rejected,
    /// Order has expired.
    Expired,
    /// Order has been replaced.
    Replaced,
    /// Order is done for the day.
    DoneForDay,
    /// Order is no longer on the orderbook (terminal state).
    Out,
    /// Order was reconciled out asynchronously.
    ReconciledOut,
    /// Order is in a stale state (expected transitions not occurring).
    Stale,
    /// Order status is unknown.
    Unknown,
}

impl From<AxOrderStatus> for OrderStatus {
    fn from(status: AxOrderStatus) -> Self {
        match status {
            AxOrderStatus::Pending => Self::Submitted,
            AxOrderStatus::Accepted => Self::Accepted,
            AxOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            AxOrderStatus::Filled => Self::Filled,
            AxOrderStatus::Canceling => Self::PendingCancel,
            AxOrderStatus::Canceled => Self::Canceled,
            AxOrderStatus::Rejected => Self::Rejected,
            AxOrderStatus::Expired => Self::Expired,
            AxOrderStatus::Replaced => Self::Accepted,
            AxOrderStatus::DoneForDay => Self::Canceled,
            AxOrderStatus::Out => Self::Canceled,
            AxOrderStatus::ReconciledOut => Self::Canceled,
            AxOrderStatus::Stale => Self::Accepted,
            AxOrderStatus::Unknown => Self::Initialized,
        }
    }
}

/// Time in force for order validity.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxTimeInForce {
    /// Good-Till-Canceled: order remains active until filled or canceled.
    Gtc,
    /// Good-Till-Date: order remains active until specified datetime.
    Gtd,
    /// Day order: valid until end of trading day.
    Day,
    /// Immediate-Or-Cancel: fill immediately or cancel unfilled portion.
    Ioc,
    /// Fill-Or-Kill: execute entire order immediately or cancel.
    Fok,
}

impl From<AxTimeInForce> for TimeInForce {
    fn from(tif: AxTimeInForce) -> Self {
        match tif {
            AxTimeInForce::Gtc => Self::Gtc,
            AxTimeInForce::Gtd => Self::Gtd,
            AxTimeInForce::Day => Self::Day,
            AxTimeInForce::Ioc => Self::Ioc,
            AxTimeInForce::Fok => Self::Fok,
        }
    }
}

impl TryFrom<TimeInForce> for AxTimeInForce {
    type Error = &'static str;

    fn try_from(tif: TimeInForce) -> Result<Self, Self::Error> {
        match tif {
            TimeInForce::Gtc => Ok(Self::Gtc),
            TimeInForce::Gtd => Ok(Self::Gtd),
            TimeInForce::Day => Ok(Self::Day),
            TimeInForce::Ioc => Ok(Self::Ioc),
            TimeInForce::Fok => Ok(Self::Fok),
            _ => Err("Unsupported time-in-force for AX"),
        }
    }
}

/// Order type as defined by the AX Exchange API.
///
/// # References
/// - <https://docs.architect.co/sdk-reference/order-entry>
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxOrderType {
    /// Limit order; execute no worse than the limit price specified.
    #[default]
    Limit,
    /// Stop-limit order; if the trigger price is breached, place a limit order.
    StopLossLimit,
    /// Take-profit order; if the trigger price is breached, place a limit order.
    /// Note: Not currently implemented by Architect.
    TakeProfitLimit,
}

impl From<AxOrderType> for OrderType {
    fn from(order_type: AxOrderType) -> Self {
        match order_type {
            AxOrderType::Limit => Self::Limit,
            AxOrderType::StopLossLimit => Self::StopLimit,
            AxOrderType::TakeProfitLimit => Self::LimitIfTouched,
        }
    }
}

impl TryFrom<OrderType> for AxOrderType {
    type Error = &'static str;

    fn try_from(order_type: OrderType) -> Result<Self, Self::Error> {
        match order_type {
            OrderType::Limit => Ok(Self::Limit),
            OrderType::StopLimit => Ok(Self::StopLossLimit),
            OrderType::LimitIfTouched => Ok(Self::TakeProfitLimit),
            _ => Err("Unsupported order type for AX"),
        }
    }
}

/// Market data subscription level.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxMarketDataLevel {
    /// Level 1: best bid/ask only.
    #[serde(rename = "LEVEL_1")]
    #[strum(serialize = "LEVEL_1")]
    Level1,
    /// Level 2: aggregated price levels.
    #[serde(rename = "LEVEL_2")]
    #[strum(serialize = "LEVEL_2")]
    Level2,
    /// Level 3: individual order quantities.
    #[serde(rename = "LEVEL_3")]
    #[strum(serialize = "LEVEL_3")]
    Level3,
}

/// Candle/bar width for market data subscriptions.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxCandleWidth {
    /// 1-second candles.
    #[serde(rename = "1s")]
    #[strum(serialize = "1s")]
    Seconds1,
    /// 5-second candles.
    #[serde(rename = "5s")]
    #[strum(serialize = "5s")]
    Seconds5,
    /// 1-minute candles.
    #[serde(rename = "1m")]
    #[strum(serialize = "1m")]
    Minutes1,
    /// 5-minute candles.
    #[serde(rename = "5m")]
    #[strum(serialize = "5m")]
    Minutes5,
    /// 15-minute candles.
    #[serde(rename = "15m")]
    #[strum(serialize = "15m")]
    Minutes15,
    /// 1-hour candles.
    #[serde(rename = "1h")]
    #[strum(serialize = "1h")]
    Hours1,
    /// 1-day candles.
    #[serde(rename = "1d")]
    #[strum(serialize = "1d")]
    Days1,
}

/// WebSocket market data message type (server to client).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxMdWsMessageType {
    /// Heartbeat event.
    #[serde(rename = "h")]
    #[strum(serialize = "h")]
    Heartbeat,
    /// Ticker statistics update.
    #[serde(rename = "s")]
    #[strum(serialize = "s")]
    Ticker,
    /// Trade event.
    #[serde(rename = "t")]
    #[strum(serialize = "t")]
    Trade,
    /// Candle/OHLCV update.
    #[serde(rename = "c")]
    #[strum(serialize = "c")]
    Candle,
    /// Level 1 book update (best bid/ask).
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    BookLevel1,
    /// Level 2 book update (aggregated levels).
    #[serde(rename = "2")]
    #[strum(serialize = "2")]
    BookLevel2,
    /// Level 3 book update (individual orders).
    #[serde(rename = "3")]
    #[strum(serialize = "3")]
    BookLevel3,
}

/// WebSocket order message type (server to client).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxOrderWsMessageType {
    /// Heartbeat event.
    #[serde(rename = "h")]
    #[strum(serialize = "h")]
    Heartbeat,
    /// Cancel rejected event.
    #[serde(rename = "e")]
    #[strum(serialize = "e")]
    CancelRejected,
    /// Order acknowledged event.
    #[serde(rename = "n")]
    #[strum(serialize = "n")]
    OrderAcknowledged,
    /// Order canceled event.
    #[serde(rename = "c")]
    #[strum(serialize = "c")]
    OrderCanceled,
    /// Order replaced/amended event.
    #[serde(rename = "r")]
    #[strum(serialize = "r")]
    OrderReplaced,
    /// Order rejected event.
    #[serde(rename = "j")]
    #[strum(serialize = "j")]
    OrderRejected,
    /// Order expired event.
    #[serde(rename = "x")]
    #[strum(serialize = "x")]
    OrderExpired,
    /// Order done for day event.
    #[serde(rename = "d")]
    #[strum(serialize = "d")]
    OrderDoneForDay,
    /// Order partially filled event.
    #[serde(rename = "p")]
    #[strum(serialize = "p")]
    OrderPartiallyFilled,
    /// Order filled event.
    #[serde(rename = "f")]
    #[strum(serialize = "f")]
    OrderFilled,
}

/// Reason for order cancellation.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxCancelReason {
    /// User requested cancellation.
    UserRequested,
}

/// Reason for cancel rejection.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum AxCancelRejectionReason {
    /// Order not found or already canceled.
    OrderNotFound,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(AxInstrumentState::Open, "\"OPEN\"")]
    #[case(AxInstrumentState::PreOpen, "\"PRE_OPEN\"")]
    #[case(AxInstrumentState::Suspended, "\"SUSPENDED\"")]
    #[case(AxInstrumentState::Delisted, "\"DELISTED\"")]
    fn test_instrument_state_serialization(
        #[case] state: AxInstrumentState,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, expected);

        let parsed: AxInstrumentState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[rstest]
    #[case(AxOrderSide::Buy, "\"B\"")]
    #[case(AxOrderSide::Sell, "\"S\"")]
    fn test_order_side_serialization(#[case] side: AxOrderSide, #[case] expected: &str) {
        let json = serde_json::to_string(&side).unwrap();
        assert_eq!(json, expected);

        let parsed: AxOrderSide = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, side);
    }

    #[rstest]
    #[case(AxOrderStatus::Pending, "\"PENDING\"")]
    #[case(AxOrderStatus::Accepted, "\"ACCEPTED\"")]
    #[case(AxOrderStatus::PartiallyFilled, "\"PARTIALLY_FILLED\"")]
    #[case(AxOrderStatus::Filled, "\"FILLED\"")]
    #[case(AxOrderStatus::Canceling, "\"CANCELING\"")]
    #[case(AxOrderStatus::Canceled, "\"CANCELED\"")]
    #[case(AxOrderStatus::Out, "\"OUT\"")]
    #[case(AxOrderStatus::ReconciledOut, "\"RECONCILED_OUT\"")]
    #[case(AxOrderStatus::Stale, "\"STALE\"")]
    fn test_order_status_serialization(#[case] status: AxOrderStatus, #[case] expected: &str) {
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, expected);

        let parsed: AxOrderStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[rstest]
    #[case(AxTimeInForce::Gtc, "\"GTC\"")]
    #[case(AxTimeInForce::Ioc, "\"IOC\"")]
    #[case(AxTimeInForce::Day, "\"DAY\"")]
    #[case(AxTimeInForce::Gtd, "\"GTD\"")]
    #[case(AxTimeInForce::Fok, "\"FOK\"")]
    fn test_time_in_force_serialization(#[case] tif: AxTimeInForce, #[case] expected: &str) {
        let json = serde_json::to_string(&tif).unwrap();
        assert_eq!(json, expected);

        let parsed: AxTimeInForce = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tif);
    }

    #[rstest]
    #[case(AxOrderType::Limit, "\"LIMIT\"")]
    #[case(AxOrderType::StopLossLimit, "\"STOP_LOSS_LIMIT\"")]
    #[case(AxOrderType::TakeProfitLimit, "\"TAKE_PROFIT_LIMIT\"")]
    fn test_order_type_serialization(#[case] order_type: AxOrderType, #[case] expected: &str) {
        let json = serde_json::to_string(&order_type).unwrap();
        assert_eq!(json, expected);

        let parsed: AxOrderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, order_type);
    }

    #[rstest]
    #[case(AxMarketDataLevel::Level1, "\"LEVEL_1\"")]
    #[case(AxMarketDataLevel::Level2, "\"LEVEL_2\"")]
    #[case(AxMarketDataLevel::Level3, "\"LEVEL_3\"")]
    fn test_market_data_level_serialization(
        #[case] level: AxMarketDataLevel,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, expected);

        let parsed: AxMarketDataLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, level);
    }

    #[rstest]
    #[case(AxCandleWidth::Seconds1, "\"1s\"")]
    #[case(AxCandleWidth::Minutes1, "\"1m\"")]
    #[case(AxCandleWidth::Minutes5, "\"5m\"")]
    #[case(AxCandleWidth::Hours1, "\"1h\"")]
    #[case(AxCandleWidth::Days1, "\"1d\"")]
    fn test_candle_width_serialization(#[case] width: AxCandleWidth, #[case] expected: &str) {
        let json = serde_json::to_string(&width).unwrap();
        assert_eq!(json, expected);

        let parsed: AxCandleWidth = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, width);
    }

    #[rstest]
    #[case(AxMdWsMessageType::Heartbeat, "\"h\"")]
    #[case(AxMdWsMessageType::Ticker, "\"s\"")]
    #[case(AxMdWsMessageType::Trade, "\"t\"")]
    #[case(AxMdWsMessageType::Candle, "\"c\"")]
    #[case(AxMdWsMessageType::BookLevel1, "\"1\"")]
    #[case(AxMdWsMessageType::BookLevel2, "\"2\"")]
    #[case(AxMdWsMessageType::BookLevel3, "\"3\"")]
    fn test_md_ws_message_type_serialization(
        #[case] msg_type: AxMdWsMessageType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&msg_type).unwrap();
        assert_eq!(json, expected);

        let parsed: AxMdWsMessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg_type);
    }

    #[rstest]
    #[case(AxOrderWsMessageType::Heartbeat, "\"h\"")]
    #[case(AxOrderWsMessageType::OrderAcknowledged, "\"n\"")]
    #[case(AxOrderWsMessageType::OrderCanceled, "\"c\"")]
    #[case(AxOrderWsMessageType::OrderFilled, "\"f\"")]
    #[case(AxOrderWsMessageType::OrderPartiallyFilled, "\"p\"")]
    fn test_order_ws_message_type_serialization(
        #[case] msg_type: AxOrderWsMessageType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&msg_type).unwrap();
        assert_eq!(json, expected);

        let parsed: AxOrderWsMessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg_type);
    }
}

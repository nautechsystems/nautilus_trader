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

use nautilus_model::{
    data::BarSpecification,
    enums::{
        AggressorSide, AssetClass, BarAggregation, MarketStatusAction, OrderSide, OrderStatus,
        PositionSide, TimeInForce,
    },
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.architect_ax")
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
/// - <https://docs.architect.exchange/api-reference/symbols-instruments/get-instruments>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
pub enum AxInstrumentState {
    /// Instrument is in pre-open state.
    PreOpen,
    /// Instrument is open for trading.
    Open,
    /// Instrument trading is closed.
    Closed,
    /// Instrument trading is closed and frozen.
    ClosedFrozen,
    /// Instrument trading is halted.
    Halted,
    /// Instrument is in a match-and-close auction.
    MatchAndCloseAuction,
    /// Instrument trading is suspended.
    Suspended,
    /// Instrument has been delisted.
    Delisted,
    /// Instrument state is unknown.
    #[serde(other)]
    Unknown,
}

impl AxInstrumentState {
    /// Returns whether the instrument is in a tradeable state.
    #[must_use]
    pub fn is_tradeable(self) -> bool {
        matches!(self, Self::Open | Self::PreOpen)
    }
}

impl From<AxInstrumentState> for MarketStatusAction {
    fn from(state: AxInstrumentState) -> Self {
        match state {
            AxInstrumentState::PreOpen => Self::PreOpen,
            AxInstrumentState::Open => Self::Trading,
            AxInstrumentState::Closed | AxInstrumentState::ClosedFrozen => Self::Close,
            AxInstrumentState::Halted => Self::Halt,
            AxInstrumentState::MatchAndCloseAuction => Self::Cross,
            AxInstrumentState::Suspended => Self::Suspend,
            AxInstrumentState::Delisted | AxInstrumentState::Unknown => {
                Self::NotAvailableForTrading
            }
        }
    }
}

/// Instrument category as returned by the AX Exchange API.
///
/// Unrecognized values map to `Unknown`.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/symbols-instruments/get-instruments>
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AxCategory {
    Fx,
    Equities,
    Metals,
    Energy,
    EnergyEtfs,
    Treasuries,
    Compute,
    Crypto,
    #[serde(other)]
    Unknown,
}

impl From<AxCategory> for AssetClass {
    fn from(category: AxCategory) -> Self {
        match category {
            AxCategory::Fx => Self::FX,
            AxCategory::Equities | AxCategory::EnergyEtfs => Self::Equity,
            AxCategory::Metals | AxCategory::Energy => Self::Commodity,
            AxCategory::Crypto => Self::Cryptocurrency,
            AxCategory::Treasuries => Self::Debt,
            AxCategory::Compute | AxCategory::Unknown => Self::Alternative,
        }
    }
}

/// Order side for trading operations.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
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
            AxOrderSide::Buy => Self::Buy,
            AxOrderSide::Sell => Self::Sell,
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

impl From<OrderSide> for AxOrderSide {
    fn from(side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => Self::Buy,
            OrderSide::Sell => Self::Sell,
        }
    }
}

/// How a perpetual symbol's funding accrues over a trading day.
///
/// # References
/// - <https://docs.architect.exchange/api-reference>
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxFundingVariant {
    /// A single settlement at the trading-day close.
    DailyClose,
    /// A fixed number of intraday slots, each charging its share of the day's TWAP premium.
    IntradayTwap,
    /// A funding classification added by the venue.
    #[serde(other)]
    Unknown,
}

/// Status of one funding slot within a `GET /funding-slots` trading day.
///
/// # References
/// - <https://docs.architect.exchange/api-reference>
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxFundingSlotStatus {
    /// Slot funding has settled.
    Realized,
    /// Slot funding is forecast from current mark and underlying TWAPs.
    Projected,
    /// Slot did not settle (for example a holiday or suspension); see the slot `reason`.
    Skipped,
    /// Slot is scheduled but not yet realized or projected.
    Pending,
    /// A funding classification added by the venue.
    #[serde(other)]
    Unknown,
}

/// Order status as returned by the AX Exchange API.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-open-orders>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
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
    #[serde(other)]
    Unknown,
}

impl TryFrom<AxOrderStatus> for OrderStatus {
    type Error = anyhow::Error;

    fn try_from(status: AxOrderStatus) -> Result<Self, Self::Error> {
        Ok(match status {
            AxOrderStatus::Pending => Self::Submitted,
            AxOrderStatus::Accepted => Self::Accepted,
            AxOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            AxOrderStatus::Filled => Self::Filled,
            AxOrderStatus::Canceling => Self::PendingCancel,
            AxOrderStatus::Canceled => Self::Canceled,
            AxOrderStatus::Rejected => Self::Rejected,
            AxOrderStatus::Expired => Self::Expired,
            AxOrderStatus::Replaced => Self::Canceled,
            AxOrderStatus::DoneForDay => Self::Expired,
            AxOrderStatus::Out => Self::Canceled,
            AxOrderStatus::ReconciledOut => Self::Canceled,
            AxOrderStatus::Stale => Self::Accepted,
            AxOrderStatus::Unknown => anyhow::bail!("Unmapped AX order status"),
        })
    }
}

/// Time in force for order validity.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/place-order>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
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
    /// At-the-Open: execute at market opening or expire.
    Ato,
    /// At-the-Close: execute at market close or expire.
    Atc,
    /// Time in force added by the venue.
    #[serde(other)]
    Unknown,
}

impl TryFrom<AxTimeInForce> for TimeInForce {
    type Error = anyhow::Error;

    fn try_from(tif: AxTimeInForce) -> Result<Self, Self::Error> {
        Ok(match tif {
            AxTimeInForce::Gtc => Self::Gtc,
            AxTimeInForce::Gtd => Self::Gtd,
            AxTimeInForce::Day => Self::Day,
            AxTimeInForce::Ioc => Self::Ioc,
            AxTimeInForce::Fok => Self::Fok,
            AxTimeInForce::Ato => Self::AtTheOpen,
            AxTimeInForce::Atc => Self::AtTheClose,
            AxTimeInForce::Unknown => anyhow::bail!("Unmapped AX time in force"),
        })
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
            TimeInForce::AtTheOpen => Ok(Self::Ato),
            TimeInForce::AtTheClose => Ok(Self::Atc),
        }
    }
}

/// Market data subscription level.
///
/// The AX API uses `LEVEL_1`, `LEVEL_2`, `LEVEL_3` on the wire (with underscore
/// before the digit). Serde and strum per-variant renames handle the wire and
/// string formats correctly, however PyO3's `rename_all` does not insert an
/// underscore at letter-digit boundaries, so the Python variant names are
/// `LEVEL1`, `LEVEL2`, `LEVEL3` (without underscore).
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/md-ws>
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
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.architect_ax")
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
    /// Trade prints only.
    #[serde(rename = "TRADES")]
    #[strum(serialize = "TRADES")]
    Trades,
}

/// Candle/bar width for market data subscriptions.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/md-ws>
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

impl TryFrom<&BarSpecification> for AxCandleWidth {
    type Error = anyhow::Error;

    fn try_from(spec: &BarSpecification) -> Result<Self, Self::Error> {
        let step = spec.step.get();
        match (step, spec.aggregation) {
            (1, BarAggregation::Second) => Ok(Self::Seconds1),
            (5, BarAggregation::Second) => Ok(Self::Seconds5),
            (1, BarAggregation::Minute) => Ok(Self::Minutes1),
            (5, BarAggregation::Minute) => Ok(Self::Minutes5),
            (15, BarAggregation::Minute) => Ok(Self::Minutes15),
            (1, BarAggregation::Hour) => Ok(Self::Hours1),
            (1, BarAggregation::Day) => Ok(Self::Days1),
            _ => anyhow::bail!(
                "Unsupported bar specification for AX: {step}-{:?}",
                spec.aggregation,
            ),
        }
    }
}

/// WebSocket market data request type (client to server).
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/md-ws>
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AxMdRequestType {
    /// Subscribe to market data for a symbol.
    Subscribe,
    /// Unsubscribe from market data for a symbol.
    Unsubscribe,
    /// Subscribe to candle data for a symbol.
    SubscribeCandles,
    /// Unsubscribe from candle data for a symbol.
    UnsubscribeCandles,
}

/// WebSocket order request type (client to server).
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/orders-ws>
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
pub enum AxOrderRequestType {
    /// Place a new order.
    #[serde(rename = "p")]
    #[strum(serialize = "p")]
    PlaceOrder,
    /// Cancel an existing order.
    #[serde(rename = "x")]
    #[strum(serialize = "x")]
    CancelOrder,
    /// Get open orders.
    #[serde(rename = "o")]
    #[strum(serialize = "o")]
    GetOpenOrders,
}

/// WebSocket market data message type (server to client).
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/md-ws>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
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
/// - <https://docs.architect.exchange/api-reference/order-management/orders-ws>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
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
/// - <https://docs.architect.exchange/api-reference/order-management/orders-ws>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
pub enum AxCancelReason {
    /// User requested cancellation.
    UserRequested,
    /// Unrecognized or empty reason from the server.
    #[serde(other)]
    Unknown,
}

/// Reason for cancel rejection.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/orders-ws>
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
    pyo3::pyclass(
        eq,
        eq_int,
        frozen,
        hash,
        module = "nautilus_trader.adapters.architect_ax",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
pub enum AxCancelRejectionReason {
    /// Order not found or already canceled.
    OrderNotFound,
    /// Unrecognized reason from the server.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(AxInstrumentState::Open, "\"OPEN\"")]
    #[case(AxInstrumentState::PreOpen, "\"PRE_OPEN\"")]
    #[case(AxInstrumentState::Closed, "\"CLOSED\"")]
    #[case(AxInstrumentState::ClosedFrozen, "\"CLOSED_FROZEN\"")]
    #[case(AxInstrumentState::Halted, "\"HALTED\"")]
    #[case(AxInstrumentState::MatchAndCloseAuction, "\"MATCH_AND_CLOSE_AUCTION\"")]
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
    fn test_instrument_state_unknown_string_deserializes_as_unknown() {
        let parsed: AxInstrumentState = serde_json::from_str("\"SOME_FUTURE_STATE\"").unwrap();
        assert_eq!(parsed, AxInstrumentState::Unknown);
    }

    #[rstest]
    #[case(AxInstrumentState::PreOpen, true)]
    #[case(AxInstrumentState::Open, true)]
    #[case(AxInstrumentState::Closed, false)]
    #[case(AxInstrumentState::ClosedFrozen, false)]
    #[case(AxInstrumentState::Halted, false)]
    #[case(AxInstrumentState::MatchAndCloseAuction, false)]
    #[case(AxInstrumentState::Suspended, false)]
    #[case(AxInstrumentState::Delisted, false)]
    #[case(AxInstrumentState::Unknown, false)]
    fn test_instrument_state_is_tradeable(
        #[case] state: AxInstrumentState,
        #[case] expected: bool,
    ) {
        assert_eq!(state.is_tradeable(), expected);
    }

    #[rstest]
    #[case(AxInstrumentState::PreOpen, MarketStatusAction::PreOpen)]
    #[case(AxInstrumentState::Open, MarketStatusAction::Trading)]
    #[case(AxInstrumentState::Closed, MarketStatusAction::Close)]
    #[case(AxInstrumentState::ClosedFrozen, MarketStatusAction::Close)]
    #[case(AxInstrumentState::Halted, MarketStatusAction::Halt)]
    #[case(AxInstrumentState::MatchAndCloseAuction, MarketStatusAction::Cross)]
    #[case(AxInstrumentState::Suspended, MarketStatusAction::Suspend)]
    #[case(
        AxInstrumentState::Delisted,
        MarketStatusAction::NotAvailableForTrading
    )]
    #[case(AxInstrumentState::Unknown, MarketStatusAction::NotAvailableForTrading)]
    fn test_instrument_state_to_market_status_action(
        #[case] state: AxInstrumentState,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(state), expected);
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
    #[case("\"Buy\"")]
    #[case("\"Sell\"")]
    fn test_order_side_rejects_long_form(#[case] json: &str) {
        let error = serde_json::from_str::<AxOrderSide>(json).unwrap_err();
        assert_eq!(error.classify(), serde_json::error::Category::Data);
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
    #[case(AxTimeInForce::Ato, "\"ATO\"")]
    #[case(AxTimeInForce::Atc, "\"ATC\"")]
    fn test_time_in_force_serialization(#[case] tif: AxTimeInForce, #[case] expected: &str) {
        let json = serde_json::to_string(&tif).unwrap();
        assert_eq!(json, expected);

        let parsed: AxTimeInForce = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tif);
    }

    #[rstest]
    #[case(AxMarketDataLevel::Level1, "\"LEVEL_1\"")]
    #[case(AxMarketDataLevel::Level2, "\"LEVEL_2\"")]
    #[case(AxMarketDataLevel::Level3, "\"LEVEL_3\"")]
    #[case(AxMarketDataLevel::Trades, "\"TRADES\"")]
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

    #[rstest]
    #[case(AxMdRequestType::Subscribe, "\"subscribe\"")]
    #[case(AxMdRequestType::Unsubscribe, "\"unsubscribe\"")]
    #[case(AxMdRequestType::SubscribeCandles, "\"subscribe_candles\"")]
    #[case(AxMdRequestType::UnsubscribeCandles, "\"unsubscribe_candles\"")]
    fn test_md_request_type_serialization(
        #[case] request_type: AxMdRequestType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&request_type).unwrap();
        assert_eq!(json, expected);

        let parsed: AxMdRequestType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, request_type);
    }

    #[rstest]
    #[case(AxOrderRequestType::PlaceOrder, "\"p\"")]
    #[case(AxOrderRequestType::CancelOrder, "\"x\"")]
    #[case(AxOrderRequestType::GetOpenOrders, "\"o\"")]
    fn test_order_request_type_serialization(
        #[case] request_type: AxOrderRequestType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&request_type).unwrap();
        assert_eq!(json, expected);

        let parsed: AxOrderRequestType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, request_type);
    }

    #[rstest]
    #[case("\"fx\"", AxCategory::Fx)]
    #[case("\"equities\"", AxCategory::Equities)]
    #[case("\"metals\"", AxCategory::Metals)]
    #[case("\"energy\"", AxCategory::Energy)]
    #[case("\"energy_etfs\"", AxCategory::EnergyEtfs)]
    #[case("\"treasuries\"", AxCategory::Treasuries)]
    #[case("\"compute\"", AxCategory::Compute)]
    #[case("\"crypto\"", AxCategory::Crypto)]
    #[case("\"something_new\"", AxCategory::Unknown)]
    fn test_category_deserialization(#[case] json: &str, #[case] expected: AxCategory) {
        let parsed: AxCategory = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, expected);
    }

    #[rstest]
    #[case(AxCategory::Fx, AssetClass::FX)]
    #[case(AxCategory::Equities, AssetClass::Equity)]
    #[case(AxCategory::EnergyEtfs, AssetClass::Equity)]
    #[case(AxCategory::Metals, AssetClass::Commodity)]
    #[case(AxCategory::Energy, AssetClass::Commodity)]
    #[case(AxCategory::Crypto, AssetClass::Cryptocurrency)]
    #[case(AxCategory::Treasuries, AssetClass::Debt)]
    #[case(AxCategory::Compute, AssetClass::Alternative)]
    #[case(AxCategory::Unknown, AssetClass::Alternative)]
    fn test_category_asset_class_mapping(
        #[case] category: AxCategory,
        #[case] expected: AssetClass,
    ) {
        assert_eq!(AssetClass::from(category), expected);
    }

    #[rstest]
    fn test_future_order_state_is_preserved_but_not_mapped() {
        let value = serde_json::Value::String("FUTURE_STATE".into());
        let status: AxOrderStatus = serde_json::from_value(value).unwrap();
        assert_eq!(status, AxOrderStatus::Unknown);
        assert_eq!(
            OrderStatus::try_from(status).unwrap_err().to_string(),
            "Unmapped AX order status"
        );
    }

    #[rstest]
    fn test_future_funding_classifications() {
        let value = serde_json::Value::String("future_value".into());
        assert_eq!(
            serde_json::from_value::<AxFundingVariant>(value.clone()).unwrap(),
            AxFundingVariant::Unknown
        );
        assert_eq!(
            serde_json::from_value::<AxFundingSlotStatus>(value).unwrap(),
            AxFundingSlotStatus::Unknown
        );
    }

    #[rstest]
    fn test_energy_etfs_category_text_matches_wire_name() {
        assert_eq!(AxCategory::EnergyEtfs.to_string(), "energy_etfs");
        assert_eq!(
            "energy_etfs".parse::<AxCategory>().unwrap(),
            AxCategory::EnergyEtfs
        );
    }

    #[rstest]
    #[case("CandleWidth", enum_wire_roundtrip::<AxCandleWidth>)]
    #[case("EstimatedFundingRateStatus", enum_wire_roundtrip::<crate::http::models::AxEstimatedFundingStatus>)]
    #[case("FundingSlotStatus", enum_wire_roundtrip::<AxFundingSlotStatus>)]
    #[case("FundingVariant", enum_wire_roundtrip::<AxFundingVariant>)]
    #[case("InstrumentCategory", enum_wire_roundtrip::<AxCategory>)]
    #[case("InstrumentState", enum_wire_roundtrip::<AxInstrumentState>)]
    #[case("Side", enum_wire_roundtrip::<AxOrderSide>)]
    #[case("OrderRejectReason", enum_wire_roundtrip::<crate::http::models::AxOrderRejectReason>)]
    #[case("OrderState", enum_wire_roundtrip::<AxOrderStatus>)]
    #[case("RepriceBehavior", enum_wire_roundtrip::<crate::http::models::AxRepriceBehavior>)]
    #[case("TimeInForce", enum_wire_roundtrip::<AxTimeInForce>)]
    #[case("Environment", enum_wire_roundtrip::<AxEnvironment>)]
    #[case("MarketDataLevel", enum_wire_roundtrip::<AxMarketDataLevel>)]
    #[case("MdRequestType", enum_wire_roundtrip::<AxMdRequestType>)]
    #[case("OrderRequestType", enum_wire_roundtrip::<AxOrderRequestType>)]
    #[case("MdWsMessageType", enum_wire_roundtrip::<AxMdWsMessageType>)]
    #[case("OrderWsMessageType", enum_wire_roundtrip::<AxOrderWsMessageType>)]
    #[case("CancelReason", enum_wire_roundtrip::<AxCancelReason>)]
    #[case("CancelRejectionReason", enum_wire_roundtrip::<AxCancelRejectionReason>)]
    fn test_documented_enum_values(
        #[case] name: &str,
        #[case] roundtrip: fn(serde_json::Value) -> serde_json::Value,
    ) {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/wire_enum_values.json")).unwrap();

        for value in fixtures[name].as_array().unwrap() {
            assert_eq!(roundtrip(value.clone()), *value, "{name}: {value}");
        }
    }

    fn enum_wire_roundtrip<T: serde::de::DeserializeOwned + Serialize>(
        value: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::to_value(serde_json::from_value::<T>(value).unwrap()).unwrap()
    }
    #[rstest]
    #[case(AxOrderStatus::Pending, OrderStatus::Submitted)]
    #[case(AxOrderStatus::Accepted, OrderStatus::Accepted)]
    #[case(AxOrderStatus::PartiallyFilled, OrderStatus::PartiallyFilled)]
    #[case(AxOrderStatus::Filled, OrderStatus::Filled)]
    #[case(AxOrderStatus::Canceled, OrderStatus::Canceled)]
    #[case(AxOrderStatus::Rejected, OrderStatus::Rejected)]
    #[case(AxOrderStatus::Expired, OrderStatus::Expired)]
    #[case(AxOrderStatus::Replaced, OrderStatus::Canceled)]
    #[case(AxOrderStatus::DoneForDay, OrderStatus::Expired)]
    fn test_documented_order_status_mapping(
        #[case] wire: AxOrderStatus,
        #[case] expected: OrderStatus,
    ) {
        assert_eq!(OrderStatus::try_from(wire).unwrap(), expected);
    }
    #[rstest]
    fn test_future_time_in_force_is_not_mapped() {
        let value = serde_json::Value::String("FUTURE_TIF".into());
        let tif: AxTimeInForce = serde_json::from_value(value).unwrap();
        assert_eq!(tif, AxTimeInForce::Unknown);
        assert_eq!(
            TimeInForce::try_from(tif).unwrap_err().to_string(),
            "Unmapped AX time in force"
        );
    }
}

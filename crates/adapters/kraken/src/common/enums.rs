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

//! Enumerations that model Kraken string/int enums across HTTP and WebSocket payloads.

use nautilus_model::enums::{MarketStatusAction, OrderSide, OrderStatus, OrderType};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString, FromRepr};

/// Kraken API environment (mainnet or demo).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        frozen,
        hash,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenEnvironment {
    #[default]
    Mainnet,
    Demo,
}

/// Kraken product type (spot or futures).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        frozen,
        hash,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenProductType {
    #[default]
    Spot,
    Futures,
}

/// Kraken spot order type.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenOrderType {
    Market,
    Limit,
    #[serde(rename = "stop-loss")]
    #[strum(serialize = "stop-loss")]
    StopLoss,
    #[serde(rename = "take-profit")]
    #[strum(serialize = "take-profit")]
    TakeProfit,
    #[serde(rename = "stop-loss-limit")]
    #[strum(serialize = "stop-loss-limit")]
    StopLossLimit,
    #[serde(rename = "take-profit-limit")]
    #[strum(serialize = "take-profit-limit")]
    TakeProfitLimit,
    #[serde(rename = "trailing-stop")]
    #[strum(serialize = "trailing-stop")]
    TrailingStop,
    #[serde(rename = "trailing-stop-limit")]
    #[strum(serialize = "trailing-stop-limit")]
    TrailingStopLimit,
    #[serde(rename = "settle-position")]
    #[strum(serialize = "settle-position")]
    SettlePosition,
}

/// Kraken order side (buy or sell).
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenOrderSide {
    Buy,
    Sell,
}

/// Kraken time-in-force for orders.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(ascii_case_insensitive, serialize_all = "UPPERCASE")]
pub enum KrakenTimeInForce {
    #[serde(rename = "GTC")]
    #[strum(serialize = "GTC")]
    GoodTilCancelled,
    #[serde(rename = "IOC")]
    #[strum(serialize = "IOC")]
    ImmediateOrCancel,
    #[serde(rename = "GTD")]
    #[strum(serialize = "GTD")]
    GoodTilDate,
}

/// Kraken order status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenOrderStatus {
    Pending,
    Open,
    Closed,
    Canceled,
    Expired,
}

/// Kraken position side (long or short).
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenPositionSide {
    Long,
    Short,
}

/// Kraken trading pair status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum KrakenPairStatus {
    Online,
    #[serde(rename = "cancel_only")]
    #[strum(serialize = "cancel_only")]
    CancelOnly,
    #[serde(rename = "post_only")]
    #[strum(serialize = "post_only")]
    PostOnly,
    #[serde(rename = "limit_only")]
    #[strum(serialize = "limit_only")]
    LimitOnly,
    #[serde(rename = "reduce_only")]
    #[strum(serialize = "reduce_only")]
    ReduceOnly,
}

/// Kraken system status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenSystemStatus {
    Online,
    Maintenance,
    #[serde(rename = "cancel_only")]
    #[strum(serialize = "cancel_only")]
    CancelOnly,
    #[serde(rename = "post_only")]
    #[strum(serialize = "post_only")]
    PostOnly,
}

/// Kraken asset class.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenAssetClass {
    Currency,
    #[serde(rename = "tokenized_asset")]
    #[strum(serialize = "tokenized_asset")]
    TokenizedAsset,
}

/// Kraken futures order type.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenFuturesOrderType {
    #[serde(rename = "lmt", alias = "limit")]
    #[strum(serialize = "lmt")]
    Limit,
    #[serde(rename = "ioc")]
    #[strum(serialize = "ioc")]
    Ioc,
    #[serde(rename = "post")]
    #[strum(serialize = "post")]
    Post,
    #[serde(rename = "mkt", alias = "market")]
    #[strum(serialize = "mkt")]
    Market,
    #[serde(rename = "stp")]
    #[strum(serialize = "stp")]
    Stop,
    #[serde(rename = "stop")]
    #[strum(serialize = "stop")]
    StopLower,
    #[serde(rename = "take_profit")]
    #[strum(serialize = "take_profit")]
    TakeProfit,
    #[serde(rename = "stop_loss")]
    #[strum(serialize = "stop_loss")]
    StopLoss,
}

/// Event types from Kraken Futures sendorder/editorder responses.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(ascii_case_insensitive, serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum KrakenFuturesOrderEventType {
    /// Order was placed.
    Place,
    /// Legacy history endpoint fill event.
    Fill,
    /// Send-order execution event.
    Execution,
    /// Order was rejected.
    Reject,
    /// Order was cancelled.
    Cancel,
    /// Order was edited.
    Edit,
    /// Order expired.
    #[serde(alias = "EXPIRED")]
    #[strum(serialize = "EXPIRED")]
    Expire,
}

/// Kraken futures order status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "camelCase")]
#[strum(ascii_case_insensitive, serialize_all = "camelCase")]
pub enum KrakenFuturesOrderStatus {
    Untouched,
    PartiallyFilled,
    Filled,
    Cancelled,
    Expired,
}

/// Kraken futures trigger signal type.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenTriggerSignal {
    #[serde(rename = "last", alias = "last_price")]
    Last,
    #[serde(rename = "mark", alias = "mark_price")]
    Mark,
    #[serde(
        rename = "spot",
        alias = "spot_price",
        alias = "index",
        alias = "index_price"
    )]
    #[strum(
        serialize = "spot",
        serialize = "spot_price",
        serialize = "index",
        serialize = "index_price"
    )]
    Index,
}

/// Trigger reference price for Kraken spot conditional orders.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenSpotTrigger {
    /// Last traded price in the order book.
    Last,
    /// Index price for the broader market.
    Index,
}

/// Kraken fill type (maker or taker).
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenFillType {
    Maker,
    Taker,
}

/// Kraken API result status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenApiResult {
    Success,
    Error,
}

/// Kraken futures instrument type.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum KrakenInstrumentType {
    /// Inverse perpetual futures (e.g., PI_XBTUSD).
    FuturesInverse,
    /// Flexible/linear perpetual futures (e.g., PF_XBTUSD).
    FlexibleFutures,
}

/// Kraken futures send order status.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "camelCase")]
#[strum(ascii_case_insensitive, serialize_all = "camelCase")]
pub enum KrakenSendStatus {
    /// Order was successfully placed.
    Placed,
    /// Order was cancelled.
    Cancelled,
    /// Order was edited.
    Edited,
    /// Order not found.
    NotFound,
    /// No orders matched the cancel-all request.
    ///
    /// Returned by the Kraken Futures `cancelallorders` endpoint as the
    /// `cancelStatus.status` field. The accompanying `cancelledOrders` array
    /// may still be populated for orders that were canceled in the same call,
    /// so callers must inspect that array rather than treating this status
    /// as an error.
    NoOrdersToCancel,
    /// Insufficient available funds.
    InsufficientAvailableFunds,
    /// Invalid order type.
    InvalidOrderType,
    /// Invalid size.
    InvalidSize,
    /// Would cause liquidation.
    WouldCauseLiquidation,
    /// Post-only order would have crossed.
    PostWouldExecute,
    /// Reduce-only order would increase position.
    ReduceOnlyWouldIncreasePosition,
}

/// Kraken futures trigger side for conditional orders.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.kraken",
        eq,
        eq_int,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.kraken")
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum KrakenTriggerSide {
    /// Trigger when price goes above the trigger price.
    #[serde(rename = "trigger_above")]
    #[strum(serialize = "trigger_above")]
    TriggerAbove,
    /// Trigger when price goes below the trigger price.
    #[serde(rename = "trigger_below")]
    #[strum(serialize = "trigger_below")]
    TriggerBelow,
}

impl From<KrakenOrderSide> for OrderSide {
    fn from(value: KrakenOrderSide) -> Self {
        match value {
            KrakenOrderSide::Buy => Self::Buy,
            KrakenOrderSide::Sell => Self::Sell,
        }
    }
}

impl From<KrakenOrderType> for OrderType {
    /// Maps Kraken order types to Nautilus order types for reconciliation.
    ///
    /// Trailing stops map to their non-trailing equivalents because
    /// Kraken reports lack the offset fields required to reconstruct
    /// a trailing order during reconciliation.
    fn from(value: KrakenOrderType) -> Self {
        match value {
            KrakenOrderType::Market => Self::Market,
            KrakenOrderType::Limit => Self::Limit,
            KrakenOrderType::StopLoss => Self::StopMarket,
            KrakenOrderType::TakeProfit => Self::MarketIfTouched,
            KrakenOrderType::StopLossLimit => Self::StopLimit,
            KrakenOrderType::TakeProfitLimit => Self::LimitIfTouched,
            KrakenOrderType::TrailingStop => Self::StopMarket,
            KrakenOrderType::TrailingStopLimit => Self::StopLimit,
            KrakenOrderType::SettlePosition => Self::Market,
        }
    }
}

impl From<KrakenOrderStatus> for OrderStatus {
    fn from(value: KrakenOrderStatus) -> Self {
        match value {
            KrakenOrderStatus::Pending => Self::Initialized,
            KrakenOrderStatus::Open => Self::Accepted,
            KrakenOrderStatus::Closed => Self::Filled,
            KrakenOrderStatus::Canceled => Self::Canceled,
            KrakenOrderStatus::Expired => Self::Expired,
        }
    }
}

impl From<KrakenFuturesOrderType> for OrderType {
    fn from(value: KrakenFuturesOrderType) -> Self {
        match value {
            KrakenFuturesOrderType::Limit
            | KrakenFuturesOrderType::Ioc
            | KrakenFuturesOrderType::Post => Self::Limit,
            KrakenFuturesOrderType::Market => Self::Market,
            KrakenFuturesOrderType::Stop | KrakenFuturesOrderType::StopLower => Self::StopMarket,
            KrakenFuturesOrderType::TakeProfit => Self::MarketIfTouched,
            KrakenFuturesOrderType::StopLoss => Self::StopMarket,
        }
    }
}

impl TryFrom<OrderSide> for KrakenOrderSide {
    type Error = &'static str;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            OrderSide::NoOrderSide => Err("Cannot convert NoOrderSide to KrakenOrderSide"),
        }
    }
}

impl From<KrakenFuturesOrderStatus> for OrderStatus {
    fn from(value: KrakenFuturesOrderStatus) -> Self {
        match value {
            KrakenFuturesOrderStatus::Untouched => Self::Accepted,
            KrakenFuturesOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            KrakenFuturesOrderStatus::Filled => Self::Filled,
            KrakenFuturesOrderStatus::Cancelled => Self::Canceled,
            KrakenFuturesOrderStatus::Expired => Self::Expired,
        }
    }
}

impl From<KrakenPairStatus> for MarketStatusAction {
    fn from(value: KrakenPairStatus) -> Self {
        match value {
            KrakenPairStatus::Online => Self::Trading,
            KrakenPairStatus::CancelOnly => Self::Halt,
            KrakenPairStatus::PostOnly => Self::Pause,
            KrakenPairStatus::LimitOnly => Self::Pause,
            KrakenPairStatus::ReduceOnly => Self::Pause,
        }
    }
}

/// Determines the product type from a Kraken symbol.
///
/// Futures symbols have the following prefixes:
/// - `PI_` - Perpetual Inverse futures (e.g., `PI_XBTUSD`)
/// - `PF_` - Perpetual Fixed-margin futures (e.g., `PF_XBTUSD`)
/// - `PV_` - Perpetual Vanilla futures (e.g., `PV_XRPXBT`)
/// - `FI_` - Fixed maturity Inverse futures (e.g., `FI_XBTUSD_230929`)
/// - `FF_` - Flex futures
///
/// All other symbols are considered spot.
#[must_use]
pub fn product_type_from_symbol(symbol: &str) -> KrakenProductType {
    if symbol.starts_with("PI_")
        || symbol.starts_with("PF_")
        || symbol.starts_with("PV_")
        || symbol.starts_with("FI_")
        || symbol.starts_with("FF_")
    {
        KrakenProductType::Futures
    } else {
        KrakenProductType::Spot
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::{MarketStatusAction, OrderType};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::online(KrakenPairStatus::Online, MarketStatusAction::Trading)]
    #[case::cancel_only(KrakenPairStatus::CancelOnly, MarketStatusAction::Halt)]
    #[case::post_only(KrakenPairStatus::PostOnly, MarketStatusAction::Pause)]
    #[case::limit_only(KrakenPairStatus::LimitOnly, MarketStatusAction::Pause)]
    #[case::reduce_only(KrakenPairStatus::ReduceOnly, MarketStatusAction::Pause)]
    fn test_pair_status_to_market_status_action(
        #[case] input: KrakenPairStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(input), expected);
    }

    #[rstest]
    #[case::trailing_stop(KrakenOrderType::TrailingStop, OrderType::StopMarket)]
    #[case::trailing_stop_limit(KrakenOrderType::TrailingStopLimit, OrderType::StopLimit)]
    fn test_trailing_stop_order_type_mapping(
        #[case] input: KrakenOrderType,
        #[case] expected: OrderType,
    ) {
        assert_eq!(OrderType::from(input), expected);
    }

    #[rstest]
    #[case("\"placed\"", KrakenSendStatus::Placed)]
    #[case("\"cancelled\"", KrakenSendStatus::Cancelled)]
    #[case("\"edited\"", KrakenSendStatus::Edited)]
    #[case("\"notFound\"", KrakenSendStatus::NotFound)]
    #[case("\"noOrdersToCancel\"", KrakenSendStatus::NoOrdersToCancel)]
    #[case(
        "\"insufficientAvailableFunds\"",
        KrakenSendStatus::InsufficientAvailableFunds
    )]
    #[case("\"invalidOrderType\"", KrakenSendStatus::InvalidOrderType)]
    #[case("\"invalidSize\"", KrakenSendStatus::InvalidSize)]
    #[case("\"wouldCauseLiquidation\"", KrakenSendStatus::WouldCauseLiquidation)]
    #[case("\"postWouldExecute\"", KrakenSendStatus::PostWouldExecute)]
    #[case(
        "\"reduceOnlyWouldIncreasePosition\"",
        KrakenSendStatus::ReduceOnlyWouldIncreasePosition
    )]
    fn test_send_status_deserialization(#[case] raw: &str, #[case] expected: KrakenSendStatus) {
        let parsed: KrakenSendStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed, expected);
    }

    #[rstest]
    #[case("\"last\"", KrakenTriggerSignal::Last)]
    #[case("\"last_price\"", KrakenTriggerSignal::Last)]
    #[case("\"mark\"", KrakenTriggerSignal::Mark)]
    #[case("\"mark_price\"", KrakenTriggerSignal::Mark)]
    #[case("\"spot\"", KrakenTriggerSignal::Index)]
    #[case("\"spot_price\"", KrakenTriggerSignal::Index)]
    #[case("\"index\"", KrakenTriggerSignal::Index)]
    #[case("\"index_price\"", KrakenTriggerSignal::Index)]
    fn test_trigger_signal_deserialization(
        #[case] raw: &str,
        #[case] expected: KrakenTriggerSignal,
    ) {
        let parsed: KrakenTriggerSignal = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed, expected);
    }

    #[rstest]
    #[case("\"PLACE\"", KrakenFuturesOrderEventType::Place)]
    #[case("\"FILL\"", KrakenFuturesOrderEventType::Fill)]
    #[case("\"EXECUTION\"", KrakenFuturesOrderEventType::Execution)]
    #[case("\"REJECT\"", KrakenFuturesOrderEventType::Reject)]
    #[case("\"CANCEL\"", KrakenFuturesOrderEventType::Cancel)]
    #[case("\"EDIT\"", KrakenFuturesOrderEventType::Edit)]
    #[case("\"EXPIRE\"", KrakenFuturesOrderEventType::Expire)]
    #[case("\"EXPIRED\"", KrakenFuturesOrderEventType::Expire)]
    fn test_futures_order_event_type_deserialization(
        #[case] raw: &str,
        #[case] expected: KrakenFuturesOrderEventType,
    ) {
        let parsed: KrakenFuturesOrderEventType = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed, expected);
    }
}

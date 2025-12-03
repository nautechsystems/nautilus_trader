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

//! Enumerations that model Kraken string/int enums across HTTP and WebSocket payloads.

use nautilus_model::enums::{OrderSide, OrderStatus, OrderType};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString, FromRepr};

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
        hash
    )
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenEnvironment {
    #[default]
    Mainnet,
    Testnet,
}

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
        hash
    )
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenProductType {
    #[default]
    Spot,
    Futures,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    #[serde(rename = "settle-position")]
    #[strum(serialize = "settle-position")]
    SettlePosition,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenOrderSide {
    Buy,
    Sell,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenPositionSide {
    Long,
    Short,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenAssetClass {
    Currency,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenFuturesOrderType {
    #[serde(rename = "lmt")]
    #[strum(serialize = "lmt")]
    Limit,
    #[serde(rename = "ioc")]
    #[strum(serialize = "ioc")]
    Ioc,
    #[serde(rename = "post")]
    #[strum(serialize = "post")]
    Post,
    #[serde(rename = "mkt")]
    #[strum(serialize = "mkt")]
    Market,
    #[serde(rename = "stp")]
    #[strum(serialize = "stp")]
    Stop,
    #[serde(rename = "take_profit")]
    #[strum(serialize = "take_profit")]
    TakeProfit,
    #[serde(rename = "stop_loss")]
    #[strum(serialize = "stop_loss")]
    StopLoss,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenTriggerSignal {
    Last,
    Mark,
    Index,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenFillType {
    Maker,
    Taker,
}

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenApiResult {
    Success,
    Error,
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
    fn from(value: KrakenOrderType) -> Self {
        match value {
            KrakenOrderType::Market => Self::Market,
            KrakenOrderType::Limit => Self::Limit,
            KrakenOrderType::StopLoss => Self::StopMarket,
            KrakenOrderType::TakeProfit => Self::MarketIfTouched,
            KrakenOrderType::StopLossLimit => Self::StopLimit,
            KrakenOrderType::TakeProfitLimit => Self::LimitIfTouched,
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
            KrakenFuturesOrderType::Stop => Self::StopMarket,
            KrakenFuturesOrderType::TakeProfit => Self::MarketIfTouched,
            KrakenFuturesOrderType::StopLoss => Self::StopMarket,
        }
    }
}

impl From<OrderSide> for KrakenOrderSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Buy => Self::Buy,
            OrderSide::Sell => Self::Sell,
            OrderSide::NoOrderSide => Self::Buy, // Default fallback
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

/// Determines the product type from a Kraken symbol.
///
/// Futures symbols have the following prefixes:
/// - `PI_` - Perpetual Inverse futures (e.g., `PI_XBTUSD`)
/// - `PF_` - Perpetual Fixed-margin futures (e.g., `PF_XBTUSD`)
/// - `FI_` - Fixed maturity Inverse futures (e.g., `FI_XBTUSD_230929`)
/// - `FF_` - Flex futures
///
/// All other symbols are considered spot.
#[must_use]
pub fn product_type_from_symbol(symbol: &str) -> KrakenProductType {
    if symbol.starts_with("PI_")
        || symbol.starts_with("PF_")
        || symbol.starts_with("FI_")
        || symbol.starts_with("FF_")
    {
        KrakenProductType::Futures
    } else {
        KrakenProductType::Spot
    }
}

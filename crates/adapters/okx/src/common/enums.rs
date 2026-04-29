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

//! Enumerations mapping OKX concepts onto idiomatic Nautilus variants.

use nautilus_model::enums::{
    AggressorSide, GreeksConvention, LiquiditySide, OptionKind, OrderSide, OrderSideSpecified,
    OrderStatus, OrderType, PositionSide, TriggerType,
};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::common::consts::{OKX_ADVANCE_ALGO_ORDER_TYPES, OKX_CONDITIONAL_ORDER_TYPES};

/// Represents the type of book action.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum OKXBookAction {
    /// Incremental update.
    Update,
    /// Full snapshot.
    Snapshot,
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXCandleConfirm {
    /// K-line is incomplete.
    #[serde(rename = "0")]
    Partial,
    /// K-line is completed.
    #[serde(rename = "1")]
    Closed,
}

/// Represents the side of an order or trade (Buy/Sell).
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXSide {
    /// Buy side of a trade or order.
    Buy,
    /// Sell side of a trade or order.
    Sell,
}

impl From<OrderSideSpecified> for OKXSide {
    fn from(value: OrderSideSpecified) -> Self {
        match value {
            OrderSideSpecified::Buy => Self::Buy,
            OrderSideSpecified::Sell => Self::Sell,
        }
    }
}

impl From<OKXSide> for AggressorSide {
    fn from(value: OKXSide) -> Self {
        match value {
            OKXSide::Buy => Self::Buyer,
            OKXSide::Sell => Self::Seller,
        }
    }
}

/// Represents the available order types on OKX.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXOrderType {
    /// Market order, executed immediately at current market price.
    Market,
    /// Limit order, executed only at specified price or better.
    Limit,
    PostOnly,        // limit only, requires "px" to be provided
    Fok,             // Market order if "px" is not provided, otherwise limit order
    Ioc,             // Market order if "px" is not provided, otherwise limit order
    OptimalLimitIoc, // Market order with immediate-or-cancel order
    Mmp,             // Market Maker Protection (only applicable to Option in Portfolio Margin mode)
    MmpAndPostOnly, // Market Maker Protection and Post-only order(only applicable to Option in Portfolio Margin mode)
    OpFok,          // Fill-or-Kill for options (only applicable to Option)
    Trigger,        // Conditional/algo order (stop orders, etc.)
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXOrderStatus {
    Canceled,
    Live,
    Effective,
    PartiallyFilled,
    Filled,
    MmpCanceled,
    OrderPlaced,
}

impl TryFrom<OrderStatus> for OKXOrderStatus {
    type Error = OrderStatus;

    /// Converts a Nautilus [`OrderStatus`] into the matching [`OKXOrderStatus`].
    ///
    /// Returns the original [`OrderStatus`] in the error case for any variant
    /// that has no representable OKX equivalent (e.g. `Submitted`, `PendingNew`,
    /// `Triggered`, `PendingCancel`, `Expired`, `Rejected`).
    fn try_from(value: OrderStatus) -> Result<Self, Self::Error> {
        match value {
            OrderStatus::Canceled => Ok(Self::Canceled),
            OrderStatus::Accepted => Ok(Self::Live),
            OrderStatus::PartiallyFilled => Ok(Self::PartiallyFilled),
            OrderStatus::Filled => Ok(Self::Filled),
            other => Err(other),
        }
    }
}

/// Represents the type of execution that generated a trade.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXExecType {
    #[serde(rename = "")]
    #[default]
    None,
    #[serde(rename = "T")]
    Taker,
    #[serde(rename = "M")]
    Maker,
}

impl From<LiquiditySide> for OKXExecType {
    fn from(value: LiquiditySide) -> Self {
        match value {
            LiquiditySide::NoLiquiditySide => Self::None,
            LiquiditySide::Taker => Self::Taker,
            LiquiditySide::Maker => Self::Maker,
        }
    }
}

/// Represents instrument types on OKX.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXInstrumentType {
    #[default]
    Any,
    /// Spot products.
    Spot,
    /// Margin products.
    Margin,
    /// Swap products.
    Swap,
    /// Futures products.
    Futures,
    /// Option products.
    Option,
}

/// Represents an instrument status on OKX.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXInstrumentStatus {
    Live,
    Suspend,
    Preopen,
    Test,
}

/// Represents an instrument contract type on OKX.
#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXContractType {
    #[serde(rename = "")]
    #[default]
    None,
    Linear,
    Inverse,
}

/// Represents an option type on OKX.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXOptionType {
    #[serde(rename = "")]
    None,
    #[serde(rename = "C")]
    Call,
    #[serde(rename = "P")]
    Put,
}

impl TryFrom<OKXOptionType> for OptionKind {
    type Error = OKXOptionType;

    /// Converts an OKX option type into the matching Nautilus [`OptionKind`].
    ///
    /// Returns the source variant in the error case for [`OKXOptionType::None`]
    /// (sent by OKX as an empty `optType` for non-option instruments and the
    /// occasional malformed payload). Callers should skip such instruments
    /// rather than treating the unknown variant as a default option kind.
    fn try_from(option_type: OKXOptionType) -> Result<Self, Self::Error> {
        match option_type {
            OKXOptionType::Call => Ok(Self::Call),
            OKXOptionType::Put => Ok(Self::Put),
            other => Err(other),
        }
    }
}

/// Represents the convention used for option greeks on OKX.
///
/// OKX publishes two parallel greek sets on `opt-summary` and related endpoints:
/// Black-Scholes greeks denominated in USD, and price-adjusted greeks denominated
/// in the underlying/coin units.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE", ascii_case_insensitive)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXGreeksType {
    /// Black-Scholes greeks in USD.
    #[default]
    Bs = 0,
    /// Price-adjusted greeks in the underlying/coin units.
    Pa = 1,
}

impl From<u8> for OKXGreeksType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Bs,
            1 => Self::Pa,
            _ => {
                log::warn!("Invalid OKXGreeksType {value}, defaulting to Bs");
                Self::Bs
            }
        }
    }
}

impl From<GreeksConvention> for OKXGreeksType {
    fn from(convention: GreeksConvention) -> Self {
        match convention {
            GreeksConvention::BlackScholes => Self::Bs,
            GreeksConvention::PriceAdjusted => Self::Pa,
        }
    }
}

impl From<OKXGreeksType> for GreeksConvention {
    fn from(greeks_type: OKXGreeksType) -> Self {
        match greeks_type {
            OKXGreeksType::Bs => Self::BlackScholes,
            OKXGreeksType::Pa => Self::PriceAdjusted,
        }
    }
}

/// Represents the trading mode for OKX orders.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXTradeMode {
    #[default]
    Cash,
    Isolated,
    Cross,
    #[strum(serialize = "spot_isolated")]
    SpotIsolated,
}

/// Represents an OKX account mode.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#overview-account-mode>
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXAccountMode {
    #[serde(rename = "Spot mode")]
    Spot,
    #[serde(rename = "Spot and futures mode")]
    SpotAndFutures,
    #[serde(rename = "Multi-currency margin mode")]
    MultiCurrencyMarginMode,
    #[serde(rename = "Portfolio margin mode")]
    PortfolioMarginMode,
}

/// Represents the margin mode for OKX accounts.
///
/// # Reference
///
/// - <https://www.okx.com/en-au/help/iv-isolated-margin-mode>
/// - <https://www.okx.com/en-au/help/iii-single-currency-margin-cross-margin-trading>
/// - <https://www.okx.com/en-au/help/iv-multi-currency-margin-mode-cross-margin-trading>
#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXMarginMode {
    #[serde(rename = "")]
    #[default]
    None,
    Isolated,
    Cross,
}

/// Represents the position mode for OKX accounts.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-set-position-mode>
#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Display,
    PartialEq,
    Eq,
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
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXPositionMode {
    #[default]
    #[serde(rename = "net_mode")]
    NetMode,
    #[serde(rename = "long_short_mode")]
    LongShortMode,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXPositionSide {
    #[serde(rename = "")]
    None,
    Net,
    Long,
    Short,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXSelfTradePreventionMode {
    #[default]
    #[serde(rename = "")]
    None,
    CancelMaker,
    CancelTaker,
    CancelBoth,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXTakeProfitKind {
    #[serde(rename = "")]
    None,
    Condition,
    Limit,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum OKXTriggerType {
    #[default]
    #[serde(rename = "")]
    None,
    Last,
    Index,
    Mark,
}

impl From<TriggerType> for OKXTriggerType {
    fn from(value: TriggerType) -> Self {
        match value {
            TriggerType::LastPrice => Self::Last,
            TriggerType::MarkPrice => Self::Mark,
            TriggerType::IndexPrice => Self::Index,
            _ => Self::Last,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::enums::{GreeksConvention, OptionKind, OrderStatus};
    use rstest::rstest;

    use super::{OKXGreeksType, OKXOptionType, OKXOrderStatus, OKXOrderType, OKXTriggerType};

    #[rstest]
    fn test_okx_trigger_type_from_str_accepts_snake_case_values() {
        assert_eq!(
            OKXTriggerType::from_str("last").unwrap(),
            OKXTriggerType::Last
        );
        assert_eq!(
            OKXTriggerType::from_str("mark").unwrap(),
            OKXTriggerType::Mark
        );
        assert_eq!(
            OKXTriggerType::from_str("index").unwrap(),
            OKXTriggerType::Index
        );
    }

    #[rstest]
    #[case(OKXGreeksType::Bs, "\"BS\"")]
    #[case(OKXGreeksType::Pa, "\"PA\"")]
    fn test_greeks_type_serde_roundtrip(#[case] input: OKXGreeksType, #[case] expected: &str) {
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, expected);
        let parsed: OKXGreeksType = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, input);
    }

    #[rstest]
    fn test_greeks_type_default_is_bs() {
        assert_eq!(OKXGreeksType::default(), OKXGreeksType::Bs);
    }

    #[rstest]
    fn test_greeks_type_from_u8() {
        assert_eq!(OKXGreeksType::from(0_u8), OKXGreeksType::Bs);
        assert_eq!(OKXGreeksType::from(1_u8), OKXGreeksType::Pa);
        assert_eq!(OKXGreeksType::from(99_u8), OKXGreeksType::Bs);
    }

    #[rstest]
    #[case(GreeksConvention::BlackScholes, OKXGreeksType::Bs)]
    #[case(GreeksConvention::PriceAdjusted, OKXGreeksType::Pa)]
    fn test_greeks_type_convention_roundtrip(
        #[case] convention: GreeksConvention,
        #[case] expected: OKXGreeksType,
    ) {
        let mapped: OKXGreeksType = convention.into();
        assert_eq!(mapped, expected);
        let back: GreeksConvention = mapped.into();
        assert_eq!(back, convention);
    }

    #[rstest]
    fn test_op_fok_serializes_to_snake_case() {
        let json = serde_json::to_string(&OKXOrderType::OpFok).unwrap();
        assert_eq!(json, "\"op_fok\"");
    }

    #[rstest]
    fn test_op_fok_deserializes_from_snake_case() {
        let parsed: OKXOrderType = serde_json::from_str("\"op_fok\"").unwrap();
        assert_eq!(parsed, OKXOrderType::OpFok);
    }

    #[rstest]
    fn test_op_fok_converts_to_limit_order_type() {
        use nautilus_model::enums::OrderType;
        let order_type: OrderType = OKXOrderType::OpFok.into();
        assert_eq!(order_type, OrderType::Limit);
    }

    #[rstest]
    #[case::call(OKXOptionType::Call, Ok(OptionKind::Call))]
    #[case::put(OKXOptionType::Put, Ok(OptionKind::Put))]
    #[case::none(OKXOptionType::None, Err(OKXOptionType::None))]
    fn test_try_from_okx_option_type(
        #[case] input: OKXOptionType,
        #[case] expected: Result<OptionKind, OKXOptionType>,
    ) {
        let actual: Result<OptionKind, OKXOptionType> = input.try_into();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::canceled(OrderStatus::Canceled, Ok(OKXOrderStatus::Canceled))]
    #[case::accepted(OrderStatus::Accepted, Ok(OKXOrderStatus::Live))]
    #[case::partially_filled(OrderStatus::PartiallyFilled, Ok(OKXOrderStatus::PartiallyFilled))]
    #[case::filled(OrderStatus::Filled, Ok(OKXOrderStatus::Filled))]
    #[case::submitted(OrderStatus::Submitted, Err(OrderStatus::Submitted))]
    #[case::pending_update(OrderStatus::PendingUpdate, Err(OrderStatus::PendingUpdate))]
    #[case::pending_cancel(OrderStatus::PendingCancel, Err(OrderStatus::PendingCancel))]
    #[case::triggered(OrderStatus::Triggered, Err(OrderStatus::Triggered))]
    #[case::expired(OrderStatus::Expired, Err(OrderStatus::Expired))]
    #[case::rejected(OrderStatus::Rejected, Err(OrderStatus::Rejected))]
    fn test_try_from_order_status(
        #[case] input: OrderStatus,
        #[case] expected: Result<OKXOrderStatus, OrderStatus>,
    ) {
        let actual: Result<OKXOrderStatus, OrderStatus> = input.try_into();
        assert_eq!(actual, expected);
    }
}

/// Represents the target currency for order quantity.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OKXTargetCurrency {
    /// Base currency.
    BaseCcy,
    /// Quote currency.
    QuoteCcy,
}

/// Represents an OKX order book channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OKXBookChannel {
    /// Standard depth-first book channel (`books`).
    Book,
    /// Low-latency 400-depth channel (`books-l2-tbt`).
    BookL2Tbt,
    /// Low-latency 50-depth channel (`books50-l2-tbt`).
    Books50L2Tbt,
}

/// Represents OKX VIP level tiers for trading fee structure and API limits.
///
/// VIP levels determine:
/// - Trading fee discounts.
/// - API rate limits.
/// - Access to advanced order book channels (L2/L3 depth).
///
/// Higher VIP levels (VIP4+) get access to:
/// - "books50-l2-tbt" channel (50 depth, 10ms updates).
/// - "bbo-tbt" channel (1 depth, 10ms updates).
///
/// VIP5+ get access to:
/// - "books-l2-tbt" channel (400 depth, 10ms updates).
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
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
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXVipLevel {
    /// VIP level 0 (default tier).
    #[serde(rename = "0")]
    #[strum(serialize = "0")]
    Vip0 = 0,
    /// VIP level 1.
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    Vip1 = 1,
    /// VIP level 2.
    #[serde(rename = "2")]
    #[strum(serialize = "2")]
    Vip2 = 2,
    /// VIP level 3.
    #[serde(rename = "3")]
    #[strum(serialize = "3")]
    Vip3 = 3,
    /// VIP level 4 (can access books50-l2-tbt channel).
    #[serde(rename = "4")]
    #[strum(serialize = "4")]
    Vip4 = 4,
    /// VIP level 5 (can access books-l2-tbt channel).
    #[serde(rename = "5")]
    #[strum(serialize = "5")]
    Vip5 = 5,
    /// VIP level 6.
    #[serde(rename = "6")]
    #[strum(serialize = "6")]
    Vip6 = 6,
    /// VIP level 7.
    #[serde(rename = "7")]
    #[strum(serialize = "7")]
    Vip7 = 7,
    /// VIP level 8.
    #[serde(rename = "8")]
    #[strum(serialize = "8")]
    Vip8 = 8,
    /// VIP level 9 (highest tier).
    #[serde(rename = "9")]
    #[strum(serialize = "9")]
    Vip9 = 9,
}

impl From<u8> for OKXVipLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Vip0,
            1 => Self::Vip1,
            2 => Self::Vip2,
            3 => Self::Vip3,
            4 => Self::Vip4,
            5 => Self::Vip5,
            6 => Self::Vip6,
            7 => Self::Vip7,
            8 => Self::Vip8,
            9 => Self::Vip9,
            _ => {
                log::warn!("Invalid VIP level {value}, defaulting to Vip0");
                Self::Vip0
            }
        }
    }
}

impl From<OKXSide> for OrderSide {
    fn from(side: OKXSide) -> Self {
        match side {
            OKXSide::Buy => Self::Buy,
            OKXSide::Sell => Self::Sell,
        }
    }
}

impl From<OKXExecType> for LiquiditySide {
    fn from(exec: OKXExecType) -> Self {
        match exec {
            OKXExecType::Maker => Self::Maker,
            OKXExecType::Taker => Self::Taker,
            OKXExecType::None => Self::NoLiquiditySide,
        }
    }
}

impl From<OKXPositionSide> for PositionSide {
    fn from(side: OKXPositionSide) -> Self {
        match side {
            OKXPositionSide::Long => Self::Long,
            OKXPositionSide::Short => Self::Short,
            _ => Self::Flat,
        }
    }
}

impl From<OKXOrderStatus> for OrderStatus {
    fn from(status: OKXOrderStatus) -> Self {
        match status {
            OKXOrderStatus::Live => Self::Accepted,
            OKXOrderStatus::Effective => Self::Triggered,
            OKXOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            OKXOrderStatus::Filled => Self::Filled,
            OKXOrderStatus::Canceled | OKXOrderStatus::MmpCanceled => Self::Canceled,
            OKXOrderStatus::OrderPlaced => Self::Triggered,
        }
    }
}

impl From<OKXOrderType> for OrderType {
    fn from(ord_type: OKXOrderType) -> Self {
        match ord_type {
            OKXOrderType::Market => Self::Market,
            OKXOrderType::Limit
            | OKXOrderType::PostOnly
            | OKXOrderType::OptimalLimitIoc
            | OKXOrderType::Mmp
            | OKXOrderType::MmpAndPostOnly
            | OKXOrderType::Fok
            | OKXOrderType::OpFok
            | OKXOrderType::Ioc => Self::Limit,
            OKXOrderType::Trigger => Self::StopMarket,
        }
    }
}

impl From<OrderType> for OKXOrderType {
    fn from(value: OrderType) -> Self {
        match value {
            OrderType::Market => Self::Market,
            OrderType::Limit => Self::Limit,
            OrderType::MarketToLimit => Self::Ioc,
            // Conditional orders will be handled separately via algo orders
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
            | OrderType::TrailingStopMarket => {
                panic!("Conditional order types must use OKXAlgoOrderType")
            }
            _ => panic!("Invalid `OrderType` cannot be represented on OKX: {value:?}"),
        }
    }
}

impl From<PositionSide> for OKXPositionSide {
    fn from(value: PositionSide) -> Self {
        match value {
            PositionSide::Long => Self::Long,
            PositionSide::Short => Self::Short,
            _ => Self::None,
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXAlgoOrderType {
    Conditional,
    Oco,
    Trigger,
    MoveOrderStop,
    Iceberg,
    Twap,
}

/// Helper to determine if an order type requires algo order handling.
pub fn is_conditional_order(order_type: OrderType) -> bool {
    OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
}

/// Helper to determine if an order type requires the advance algo cancel endpoint.
pub fn is_advance_algo_order(order_type: OrderType) -> bool {
    OKX_ADVANCE_ALGO_ORDER_TYPES.contains(&order_type)
}

/// Converts Nautilus conditional order types to OKX algo order type.
///
/// # Errors
///
/// Returns an error if the provided `order_type` is not a conditional order type.
pub fn conditional_order_to_algo_type(order_type: OrderType) -> anyhow::Result<OKXAlgoOrderType> {
    match order_type {
        OrderType::StopMarket
        | OrderType::StopLimit
        | OrderType::MarketIfTouched
        | OrderType::LimitIfTouched => Ok(OKXAlgoOrderType::Trigger),
        OrderType::TrailingStopMarket => Ok(OKXAlgoOrderType::MoveOrderStop),
        _ => anyhow::bail!("Not a conditional order type: {order_type:?}"),
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXAlgoOrderStatus {
    Live,
    Pause,
    PartiallyEffective,
    Effective,
    Canceled,
    OrderFailed,
    PartiallyFailed,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXTransactionType {
    #[serde(rename = "1")]
    Buy,
    #[serde(rename = "2")]
    Sell,
    #[serde(rename = "3")]
    OpenLong,
    #[serde(rename = "4")]
    OpenShort,
    #[serde(rename = "5")]
    CloseLong,
    #[serde(rename = "6")]
    CloseShort,
    #[serde(rename = "100")]
    PartialLiquidationCloseLong,
    #[serde(rename = "101")]
    PartialLiquidationCloseShort,
    #[serde(rename = "102")]
    PartialLiquidationBuy,
    #[serde(rename = "103")]
    PartialLiquidationSell,
    #[serde(rename = "104")]
    LiquidationLong,
    #[serde(rename = "105")]
    LiquidationShort,
    #[serde(rename = "106")]
    LiquidationBuy,
    #[serde(rename = "107")]
    LiquidationSell,
    #[serde(rename = "110")]
    LiquidationTransferIn,
    #[serde(rename = "111")]
    LiquidationTransferOut,
    #[serde(rename = "118")]
    SystemTokenConversionTransferIn,
    #[serde(rename = "119")]
    SystemTokenConversionTransferOut,
    #[serde(rename = "125")]
    AdlCloseLong,
    #[serde(rename = "126")]
    AdlCloseShort,
    #[serde(rename = "127")]
    AdlBuy,
    #[serde(rename = "128")]
    AdlSell,
    #[serde(rename = "212")]
    AutoBorrowOfQuickMargin,
    #[serde(rename = "213")]
    AutoRepayOfQuickMargin,
    #[serde(rename = "204")]
    BlockTradeBuy,
    #[serde(rename = "205")]
    BlockTradeSell,
    #[serde(rename = "206")]
    BlockTradeOpenLong,
    #[serde(rename = "207")]
    BlockTradeOpenShort,
    #[serde(rename = "208")]
    BlockTradeCloseOpen,
    #[serde(rename = "209")]
    BlockTradeCloseShort,
    #[serde(rename = "270")]
    SpreadTradingBuy,
    #[serde(rename = "271")]
    SpreadTradingSell,
    #[serde(rename = "272")]
    SpreadTradingOpenLong,
    #[serde(rename = "273")]
    SpreadTradingOpenShort,
    #[serde(rename = "274")]
    SpreadTradingCloseLong,
    #[serde(rename = "275")]
    SpreadTradingCloseShort,
}

/// Represents the category of an order on OKX.
///
/// The category field indicates whether an order is a normal trade, liquidation,
/// auto-deleveraging (ADL) event, or algorithmic order type. This is critical for
/// risk management and proper handling of exchange-generated orders.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#order-book-trading-ws-order-channel>
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXOrderCategory {
    /// Normal trading order.
    Normal,
    /// Full liquidation order (position completely closed by exchange).
    FullLiquidation,
    /// Partial liquidation order (position partially closed by exchange).
    PartialLiquidation,
    /// Auto-deleveraging order (position closed to offset counterparty liquidation).
    Adl,
    /// Time-Weighted Average Price algorithmic order.
    Twap,
    /// Iceberg algorithmic order (hidden quantity).
    Iceberg,
    /// One-Cancels-the-Other algorithmic order.
    Oco,
    /// Conditional/trigger order.
    Conditional,
    /// Move order stop algorithmic order.
    MoveOrderStop,
    /// Delivery and exercise (for futures/options settlement).
    Ddh,
    /// Unknown or future category (graceful fallback).
    #[serde(other)]
    Other,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXBarSize {
    #[serde(rename = "1s")]
    Second1,
    #[serde(rename = "1m")]
    Minute1,
    #[serde(rename = "3m")]
    Minute3,
    #[serde(rename = "5m")]
    Minute5,
    #[serde(rename = "15m")]
    Minute15,
    #[serde(rename = "30m")]
    Minute30,
    #[serde(rename = "1H")]
    Hour1,
    #[serde(rename = "2H")]
    Hour2,
    #[serde(rename = "4H")]
    Hour4,
    #[serde(rename = "6H")]
    Hour6,
    #[serde(rename = "12H")]
    Hour12,
    #[serde(rename = "1D")]
    Day1,
    #[serde(rename = "2D")]
    Day2,
    #[serde(rename = "3D")]
    Day3,
    #[serde(rename = "5D")]
    Day5,
    #[serde(rename = "1W")]
    Week1,
    #[serde(rename = "1M")]
    Month1,
    #[serde(rename = "3M")]
    Month3,
}

/// Options price type for order pricing.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXPriceType {
    /// No price type specified.
    #[default]
    #[serde(rename = "")]
    None,
    /// Standard price.
    Px,
    /// Price in USD.
    Usd,
    /// Price in implied volatility.
    Vol,
}

/// Funding rate settlement state.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXSettlementState {
    /// No settlement state.
    #[default]
    #[serde(rename = "")]
    None,
    /// Settlement in progress.
    Processing,
    /// Settlement completed.
    Settled,
}

/// Quick margin type for order margin management.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OKXQuickMarginType {
    /// No quick margin type.
    #[default]
    #[serde(rename = "")]
    None,
    /// Manual margin management.
    Manual,
    /// Auto borrow margin.
    AutoBorrow,
    /// Auto repay margin.
    AutoRepay,
}

/// OKX API environment.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.okx",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXEnvironment {
    /// Live trading environment.
    #[default]
    Live,
    /// Demo trading environment.
    Demo,
}

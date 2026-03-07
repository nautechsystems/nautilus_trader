// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString};

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    PartialEq,
    Eq,
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
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetEnvironment {
    Mainnet,
    Demo,
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    PartialEq,
    Eq,
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
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetProductType {
    Spot,
    UsdtFutures,
    CoinFutures,
    UsdcFutures,
}

impl BitgetProductType {
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::UsdtFutures => "USDT-FUTURES",
            Self::CoinFutures => "COIN-FUTURES",
            Self::UsdcFutures => "USDC-FUTURES",
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    PartialEq,
    Eq,
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
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetInstrumentKind {
    Spot,
    Perp,
    Delivery,
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    PartialEq,
    Eq,
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
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetWsInstType {
    Spot,
    UsdtFutures,
    CoinFutures,
    UsdcFutures,
}

impl BitgetWsInstType {
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::UsdtFutures => "USDT-FUTURES",
            Self::CoinFutures => "COIN-FUTURES",
            Self::UsdcFutures => "USDC-FUTURES",
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
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
#[strum(serialize_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetOrderSide {
    Buy,
    Sell,
}

impl BitgetOrderSide {
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
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
#[strum(serialize_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetOrderType {
    Limit,
    Market,
}

impl BitgetOrderType {
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Market => "market",
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.bitget",
        from_py_object
    )
)]
pub enum BitgetTimeInForce {
    Gtc,
    Ioc,
    Fok,
    PostOnly,
}

impl BitgetTimeInForce {
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Gtc => "gtc",
            Self::Ioc => "ioc",
            Self::Fok => "fok",
            Self::PostOnly => "postOnly",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_inst_type_api_strings() {
        assert_eq!(BitgetWsInstType::Spot.as_api_str(), "SPOT");
        assert_eq!(BitgetWsInstType::UsdtFutures.as_api_str(), "USDT-FUTURES");
        assert_eq!(BitgetWsInstType::CoinFutures.as_api_str(), "COIN-FUTURES");
        assert_eq!(BitgetWsInstType::UsdcFutures.as_api_str(), "USDC-FUTURES");
    }

    #[test]
    fn order_side_api_string() {
        assert_eq!(BitgetOrderSide::Buy.as_api_str(), "buy");
        assert_eq!(BitgetOrderSide::Sell.as_api_str(), "sell");
    }

    #[test]
    fn order_type_api_string() {
        assert_eq!(BitgetOrderType::Limit.as_api_str(), "limit");
        assert_eq!(BitgetOrderType::Market.as_api_str(), "market");
    }

    #[test]
    fn time_in_force_api_string() {
        assert_eq!(BitgetTimeInForce::Gtc.as_api_str(), "gtc");
        assert_eq!(BitgetTimeInForce::Ioc.as_api_str(), "ioc");
        assert_eq!(BitgetTimeInForce::Fok.as_api_str(), "fok");
        assert_eq!(BitgetTimeInForce::PostOnly.as_api_str(), "postOnly");
    }
}

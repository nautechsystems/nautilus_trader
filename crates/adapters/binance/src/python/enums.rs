//! Python bindings for Binance enums.

use pyo3::prelude::*;

use crate::common::enums::{BinanceEnvironment, BinanceProductType};

#[pymethods]
impl BinanceProductType {
    fn __repr__(&self) -> String {
        format!(
            "BinanceProductType.{}",
            match self {
                Self::Spot => "SPOT",
                Self::Margin => "MARGIN",
                Self::UsdM => "USD_M",
                Self::CoinM => "COIN_M",
                Self::Options => "OPTIONS",
            }
        )
    }

    fn __str__(&self) -> String {
        match self {
            Self::Spot => "SPOT",
            Self::Margin => "MARGIN",
            Self::UsdM => "USD_M",
            Self::CoinM => "COIN_M",
            Self::Options => "OPTIONS",
        }
        .to_string()
    }
}

#[pymethods]
impl BinanceEnvironment {
    fn __repr__(&self) -> String {
        format!(
            "BinanceEnvironment.{}",
            match self {
                Self::Mainnet => "MAINNET",
                Self::Testnet => "TESTNET",
                Self::Demo => "DEMO",
            }
        )
    }

    fn __str__(&self) -> String {
        match self {
            Self::Mainnet => "MAINNET",
            Self::Testnet => "TESTNET",
            Self::Demo => "DEMO",
        }
        .to_string()
    }
}

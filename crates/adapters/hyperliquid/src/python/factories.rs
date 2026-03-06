//! Python bindings for Hyperliquid factory types.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    config::HyperliquidExecClientConfig,
    factories::{
        HyperliquidDataClientFactory, HyperliquidExecFactoryConfig,
        HyperliquidExecutionClientFactory,
    },
};

#[pymethods]
impl HyperliquidDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "HYPERLIQUID"
    }
}

#[pymethods]
impl HyperliquidExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "HYPERLIQUID"
    }
}

#[pymethods]
impl HyperliquidExecFactoryConfig {
    #[new]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        config: HyperliquidExecClientConfig,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            config,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

//! Python bindings for BitMEX factory types.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    config::BitmexExecClientConfig,
    factories::{BitmexDataClientFactory, BitmexExecFactoryConfig, BitmexExecutionClientFactory},
};

#[pymethods]
impl BitmexExecFactoryConfig {
    #[new]
    fn py_new(trader_id: TraderId, account_id: AccountId, config: BitmexExecClientConfig) -> Self {
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

#[pymethods]
impl BitmexDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "BITMEX"
    }
}

#[pymethods]
impl BitmexExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "BITMEX"
    }
}

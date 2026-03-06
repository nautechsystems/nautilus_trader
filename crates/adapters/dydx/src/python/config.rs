//! Python bindings for dYdX configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::config::{DydxDataClientConfig, DydxExecClientConfig};

#[pymethods]
impl DydxDataClientConfig {
    #[new]
    fn py_new() -> Self {
        Self::default()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl DydxExecClientConfig {
    #[new]
    fn py_new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
            ..Self::default()
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

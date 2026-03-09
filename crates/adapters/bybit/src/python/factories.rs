//! Python bindings for Bybit factory types.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::factories::{BybitDataClientFactory, BybitExecutionClientFactory};

#[pymethods]
impl BybitDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "BYBIT"
    }
}

#[pymethods]
impl BybitExecutionClientFactory {
    #[new]
    fn py_new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self::new(trader_id, account_id)
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "BYBIT"
    }
}

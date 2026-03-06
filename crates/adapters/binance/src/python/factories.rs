//! Python bindings for Binance factory types.

use pyo3::prelude::*;

use crate::{
    common::consts::BINANCE,
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
};

#[pymethods]
impl BinanceDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        BINANCE
    }
}

#[pymethods]
impl BinanceExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        BINANCE
    }
}

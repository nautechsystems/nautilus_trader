//! Python bindings for Kraken factory types.

use pyo3::prelude::*;

use crate::factories::{KrakenDataClientFactory, KrakenExecutionClientFactory};

#[pymethods]
impl KrakenDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "KRAKEN"
    }
}

#[pymethods]
impl KrakenExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "KRAKEN"
    }
}

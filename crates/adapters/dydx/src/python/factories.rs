//! Python bindings for dYdX factories.

use pyo3::prelude::*;

use crate::factories::{DydxDataClientFactory, DydxExecutionClientFactory};

#[pymethods]
impl DydxDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "DYDX"
    }
}

#[pymethods]
impl DydxExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "DYDX"
    }
}

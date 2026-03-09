//! Python bindings for Deribit factory types.

use pyo3::prelude::*;

use crate::factories::{DeribitDataClientFactory, DeribitExecutionClientFactory};

#[pymethods]
impl DeribitDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "DERIBIT"
    }
}

#[pymethods]
impl DeribitExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        "DERIBIT"
    }
}

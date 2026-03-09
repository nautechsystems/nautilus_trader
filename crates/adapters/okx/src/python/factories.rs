//! Python bindings for OKX factory types.

use pyo3::prelude::*;

use crate::{
    common::consts::OKX,
    factories::{OKXDataClientFactory, OKXExecutionClientFactory},
};

#[pymethods]
impl OKXDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        OKX
    }
}

#[pymethods]
impl OKXExecutionClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        OKX
    }
}

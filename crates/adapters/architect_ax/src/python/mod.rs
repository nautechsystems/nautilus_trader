//! Python bindings for the Ax adapter.

pub mod http;
pub mod websocket;

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{prelude::*, types::PyType};

use crate::{
    common::enums::{AxEnvironment, AxMarketDataLevel},
    http::client::AxHttpClient,
    websocket::{data::AxMdWebSocketClient, orders::AxOrdersWebSocketClient},
};

#[pymethods]
impl AxEnvironment {
    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(AxEnvironment),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl AxMarketDataLevel {
    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(AxMarketDataLevel),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
}

/// Loaded as `nautilus_pyo3.architect`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn architect(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AxEnvironment>()?;
    m.add_class::<AxMarketDataLevel>()?;
    m.add_class::<AxHttpClient>()?;
    m.add_class::<AxMdWebSocketClient>()?;
    m.add_class::<AxOrdersWebSocketClient>()?;

    Ok(())
}

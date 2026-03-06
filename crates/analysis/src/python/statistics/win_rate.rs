use std::collections::BTreeMap;

#[allow(unused_imports)] // Used in template pattern for returns conversion
use nautilus_core::UnixNanos;
use pyo3::prelude::*;

use crate::{statistic::PortfolioStatistic, statistics::win_rate::WinRate};

#[pymethods]
impl WinRate {
    #[new]
    fn py_new() -> Self {
        Self {}
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "calculate_from_realized_pnls")]
    fn py_calculate_from_realized_pnls(&mut self, realized_pnls: Vec<f64>) -> Option<f64> {
        self.calculate_from_realized_pnls(&realized_pnls)
    }

    #[pyo3(name = "calculate_from_returns")]
    #[allow(unused_variables)] // Pattern preserved for consistency across statistics
    fn py_calculate_from_returns(&mut self, _returns: BTreeMap<u64, f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_positions")]
    fn py_calculate_from_positions(&mut self, _positions: Vec<Py<PyAny>>) -> Option<f64> {
        None
    }
}

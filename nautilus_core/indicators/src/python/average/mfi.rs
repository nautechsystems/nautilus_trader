use nautilus_model::data::bar::Bar;
use pyo3::prelude::*;

use crate::{average::mfi::MoneyFlowIndex, indicator::Indicator};

#[pymethods]
impl MoneyFlowIndex {
    #[new]
    #[pyo3(signature = (period))]
    fn py_new(period: usize) -> Self {
        Self::new(period)
    }

    fn __repr__(&self) -> String {
        format!("MoneyFlowIndex({})", self.period)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    const fn py_period(&self) -> usize {
        self.period
    }

    #[getter]
    #[pyo3(name = "count")]
    const fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, typical_price: f64, volume: f64) -> f64 {
        self.update_raw(typical_price, volume);
        self.value
    }

    #[pyo3(name = "update")]
    fn py_update(&mut self, close: f64, high: f64, low: f64, volume: f64) -> f64 {
        let typical = (high + low + close) / 3.0;
        self.update_raw(typical, volume);
        self.value
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.handle_bar(bar);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

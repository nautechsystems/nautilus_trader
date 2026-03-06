use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{average::lr::LinearRegression, indicator::Indicator};

#[pymethods]
impl LinearRegression {
    #[new]
    #[must_use]
    pub fn py_new(period: usize) -> Self {
        Self::new(period)
    }

    fn __repr__(&self) -> String {
        format!("LinearRegression({})", self.period)
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
    #[pyo3(name = "slope")]
    const fn py_slope(&self) -> f64 {
        self.slope
    }

    #[getter]
    #[pyo3(name = "intercept")]
    const fn py_intercept(&self) -> f64 {
        self.intercept
    }

    #[getter]
    #[pyo3(name = "degree")]
    const fn py_degree(&self) -> f64 {
        self.degree
    }

    #[getter]
    #[pyo3(name = "cfo")]
    const fn py_cfo(&self) -> f64 {
        self.cfo
    }

    #[getter]
    #[pyo3(name = "r2")]
    const fn py_r2(&self) -> f64 {
        self.r2
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, close: f64) {
        self.update_raw(close);
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

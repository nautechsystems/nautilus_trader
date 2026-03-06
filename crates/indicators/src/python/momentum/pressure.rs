use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{average::MovingAverageType, indicator::Indicator, momentum::pressure::Pressure};

#[pymethods]
impl Pressure {
    #[new]
    #[pyo3(signature = (period, ma_type=None, atr_floor=None))]
    #[must_use]
    pub fn py_new(
        period: usize,
        ma_type: Option<MovingAverageType>,
        atr_floor: Option<f64>,
    ) -> Self {
        Self::new(period, ma_type, atr_floor)
    }

    fn __repr__(&self) -> String {
        format!("Pressure({},{})", self.period, self.ma_type)
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
    #[pyo3(name = "value_cumulative")]
    const fn py_value_cumulative(&self) -> f64 {
        self.value_cumulative
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        self.update_raw(high, low, close, volume);
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

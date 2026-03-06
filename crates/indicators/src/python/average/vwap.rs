use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{average::vwap::VolumeWeightedAveragePrice, indicator::Indicator};

#[pymethods]
impl VolumeWeightedAveragePrice {
    #[new]
    #[must_use]
    pub const fn py_new() -> Self {
        Self::new()
    }

    fn __repr__(&self) -> String {
        "VolumeWeightedAveragePrice".to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
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

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.py_update_raw(
            (&bar.close).into(),
            (&bar.volume).into(),
            bar.ts_init.as_f64(),
        );
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64, volume: f64, ts: f64) {
        self.update_raw(value, volume, ts);
    }
}

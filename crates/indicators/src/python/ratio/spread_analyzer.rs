use nautilus_model::{data::QuoteTick, identifiers::InstrumentId};
use pyo3::prelude::*;

use crate::{indicator::Indicator, ratio::spread_analyzer::SpreadAnalyzer};

#[pymethods]
impl SpreadAnalyzer {
    #[new]
    fn py_new(instrument_id: InstrumentId, capacity: usize) -> Self {
        Self::new(capacity, instrument_id)
    }

    fn __repr__(&self) -> String {
        format!("SpreadAnalyzer({})", self.capacity)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "capacity")]
    const fn py_capacity(&self) -> usize {
        self.capacity
    }

    #[getter]
    #[pyo3(name = "current")]
    const fn py_current(&self) -> f64 {
        self.current
    }

    #[getter]
    #[pyo3(name = "average")]
    const fn py_average(&self) -> f64 {
        self.average
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, quote: &QuoteTick) {
        self.handle_quote(quote);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

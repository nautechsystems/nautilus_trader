use nautilus_model::data::{Bar, QuoteTick, TradeTick};
use pyo3::prelude::*;

use crate::{indicator::Indicator, momentum::roc::RateOfChange};

#[pymethods]
impl RateOfChange {
    #[new]
    #[pyo3(signature = (period, use_log=None))]
    #[must_use]
    pub fn py_new(period: usize, use_log: Option<bool>) -> Self {
        Self::new(period, use_log)
    }

    fn __repr__(&self) -> String {
        format!("RateOfChange({},{})", self.period, self.use_log,)
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
    #[pyo3(name = "use_log")]
    const fn py_use_log(&self) -> bool {
        self.use_log
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
    fn py_update_raw(&mut self, price: f64) {
        self.update_raw(price);
    }

    #[pyo3(name = "handle_quote_tick")]
    const fn py_handle_quote_tick(&mut self, _quote: &QuoteTick) {
        // Function body intentionally left blank.
    }

    #[pyo3(name = "handle_trade_tick")]
    const fn py_handle_trade_tick(&mut self, _trade: &TradeTick) {
        // Function body intentionally left blank.
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

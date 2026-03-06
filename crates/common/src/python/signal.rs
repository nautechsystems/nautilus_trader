use nautilus_core::UnixNanos;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::signal::Signal;

#[pymethods]
impl Signal {
    #[new]
    fn py_new(name: &str, value: String, ts_event: u64, ts_init: u64) -> Self {
        Self::new(
            Ustr::from(name),
            value,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        self.name.as_str()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> &str {
        self.value.as_str()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    const fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

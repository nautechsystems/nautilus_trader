//! Python bindings from [PyO3](https://pyo3.rs).

pub mod sessions;
pub mod strategy;

use pyo3::{prelude::*, pymodule};

/// Loaded as `nautilus_pyo3.trading`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn trading(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::sessions::ForexSession>()?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_local_from_utc, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_next_start, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_prev_start, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_next_end, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_prev_end, m)?)?;
    m.add_class::<strategy::PyStrategy>()?;
    m.add_class::<crate::strategy::StrategyConfig>()?;
    m.add_class::<crate::strategy::ImportableStrategyConfig>()?;
    Ok(())
}

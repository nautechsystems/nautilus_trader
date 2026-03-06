//! Python bindings from [PyO3](https://pyo3.rs).

pub mod reconciliation;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.execution`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn execution(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(
        reconciliation::py_adjust_fills_for_partial_window,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        reconciliation::py_calculate_reconciliation_price,
        m
    )?)?;
    Ok(())
}

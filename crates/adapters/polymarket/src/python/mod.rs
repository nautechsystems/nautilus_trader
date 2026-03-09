//! Python bindings from `pyo3`.

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.polymarket`.
#[pymodule]
pub fn polymarket(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

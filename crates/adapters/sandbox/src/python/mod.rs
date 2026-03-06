//! Python bindings from `pyo3`.

pub mod config;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.sandbox`.
///
/// # Errors
///
/// Returns an error if the module registration fails or if adding functions/classes fails.
#[pymodule]
pub fn sandbox(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::SandboxExecutionClientConfig>()?;
    Ok(())
}

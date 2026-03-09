//! Python bindings from [PyO3](https://pyo3.rs).

pub mod node;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.live`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn live(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::node::LiveNode>()?;
    m.add_class::<node::LiveNodeBuilderPy>()?;
    Ok(())
}

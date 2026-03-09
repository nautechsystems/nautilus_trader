//! Python bindings from [PyO3](https://pyo3.rs).

pub mod node;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.backtest`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn backtest(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::BacktestEngineConfig>()?;
    m.add_class::<crate::config::BacktestVenueConfig>()?;
    m.add_class::<crate::config::BacktestDataConfig>()?;
    m.add_class::<crate::config::BacktestRunConfig>()?;
    m.add_class::<crate::engine::BacktestResult>()?;
    m.add_class::<crate::node::BacktestNode>()?;
    Ok(())
}

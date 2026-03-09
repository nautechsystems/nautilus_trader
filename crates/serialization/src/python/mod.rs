//! Python bindings from [PyO3](https://pyo3.rs).

#[cfg(feature = "arrow")]
pub mod arrow;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.serialization`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
// Allow unused `m` when no feature-gated content registers on the module
#[allow(unused_variables)]
#[pymodule]
pub fn serialization(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(feature = "arrow")]
    {
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::get_arrow_schema_map,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::pyobjects_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_book_deltas_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_book_depth10_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_quotes_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_trades_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_bars_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_mark_prices_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_index_prices_to_arrow_record_batch_bytes,
            m
        )?)?;
        m.add_function(wrap_pyfunction!(
            crate::python::arrow::py_instrument_closes_to_arrow_record_batch_bytes,
            m
        )?)?;
    }

    Ok(())
}

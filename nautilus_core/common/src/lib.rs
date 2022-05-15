use pyo3::prelude::*;

pub mod logging;

#[pymodule]
fn common(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    logging::register_module(py, m)?;
    Ok(())
}

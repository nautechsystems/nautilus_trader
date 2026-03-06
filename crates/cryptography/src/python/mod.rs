//! Python bindings from [PyO3](https://pyo3.rs).

pub mod signing;

use pyo3::prelude::*;

use crate::python;

/// Loaded as `nautilus_pyo3.cryptography`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn cryptography(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::signing::py_hmac_signature, m)?)?;
    m.add_function(wrap_pyfunction!(python::signing::py_rsa_signature, m)?)?;
    m.add_function(wrap_pyfunction!(python::signing::py_ed25519_signature, m)?)?;
    Ok(())
}

//! Python-specific runtime initialization.
//!
//! This module handles the Python interpreter initialization that must occur
//! before the Tokio runtime is used from Python extension modules.

use std::sync::Once;

use pyo3::Python;

static PYTHON_INIT: Once = Once::new();

/// Initializes the Python interpreter for use with the async runtime.
///
/// Python hosts the process when we build as an extension module. This function
/// keeps the interpreter alive for the lifetime of the shared Tokio runtime
/// so every worker thread sees a prepared PyO3 environment before using it.
///
/// This function is idempotent and safe to call multiple times.
pub fn initialize_python() {
    PYTHON_INIT.call_once(|| {
        Python::initialize();
    });
}

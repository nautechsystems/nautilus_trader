use logging::{LogFormat, LogLevel, Logger};
use pyo3::prelude::*;

pub mod clock;
pub mod logging;
pub mod timer;

/// This module is implemented in Rust.
#[pymodule]
fn common(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let logging = PyModule::new(py, "logging")?;
    logging.add_class::<LogFormat>()?;
    logging.add_class::<LogLevel>()?;
    logging.add_class::<Logger>()?;

    m.add_submodule(logging)?;
    Ok(())
}

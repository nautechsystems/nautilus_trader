// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings from [PyO3](https://pyo3.rs).

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]
#![allow(
    clippy::unused_self,
    reason = "PyO3 stub methods take &self for Python API parity even when the body is empty"
)]

pub mod actor;
pub mod cache;
pub mod clock;
pub mod custom;
pub mod enums;
pub mod fifo;
pub mod greeks;
pub mod indicators;
pub mod listener;
pub mod logging;
pub mod msgbus;
pub mod order_factory;
pub mod runtime;
pub mod signal;
pub mod timer;
pub mod xrate;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyErr, prelude::*};

use crate::config::ConfigError;

/// Converts a config validation failure to a Python `ValueError`.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Result::map_err passes owned errors to conversion functions"
)]
pub fn config_error_to_pyvalue_err(e: ConfigError) -> PyErr {
    to_pyvalue_err(e)
}

/// Loaded as `nautilus_pyo3.common`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[rustfmt::skip]
#[pymodule]
pub fn common(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::custom::CustomData>()?;
    m.add_class::<crate::signal::Signal>()?;
    m.add_class::<crate::timer::TimeEvent>()?;
    m.add_class::<crate::cache::CacheConfig>()?;
    m.add_class::<crate::python::actor::PyDataActor>()?;
    m.add_class::<crate::python::cache::PyCache>()?;
    m.add_class::<crate::python::fifo::PyFifoCache>()?;
    m.add_class::<crate::python::clock::PyClock>()?;
    m.add_class::<crate::python::order_factory::PyOrderFactory>()?;
    m.add_class::<crate::python::greeks::PyGreeksCalculator>()?;
    m.add_class::<crate::python::logging::PyLogger>()?;
    m.add_class::<crate::actor::data_actor::DataActorConfig>()?;
    m.add_class::<crate::actor::data_actor::ImportableActorConfig>()?;
    m.add_class::<crate::msgbus::BusMessage>()?;
    m.add_class::<crate::msgbus::config::MessageBusConfig>()?;
    m.add_class::<crate::python::msgbus::PyMessageBus>()?;
    m.add_class::<crate::enums::ComponentState>()?;
    m.add_class::<crate::enums::ComponentTrigger>()?;
    m.add_class::<crate::enums::Environment>()?;
    m.add_class::<crate::enums::LogColor>()?;
    m.add_class::<crate::enums::LogLevel>()?;
    m.add_class::<crate::enums::LogFormat>()?;
    m.add_class::<crate::logging::logger::LoggerConfig>()?;
    m.add_class::<crate::logging::logger::LogGuard>()?;
    m.add_class::<crate::logging::writer::FileWriterConfig>()?;
    m.add_function(wrap_pyfunction!(logging::py_init_logging, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logger_flush, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logging_sync_to_disk, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logger_log, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_log_header, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_log_sysinfo, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logging_clock_set_static_mode, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logging_clock_set_realtime_mode, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logging_clock_set_static_time, m)?)?;
    #[cfg(feature = "tracing-bridge")]
    m.add_function(wrap_pyfunction!(logging::py_tracing_is_initialized, m)?)?;
    #[cfg(feature = "tracing-bridge")]
    m.add_function(wrap_pyfunction!(logging::py_init_tracing, m)?)?;
    m.add_function(wrap_pyfunction!(xrate::py_get_exchange_rate, m)?)?;

    #[cfg(feature = "live")]
    m.add_class::<crate::live::listener::MessageBusListener>()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::{Python, exceptions::PyValueError};
    use rstest::rstest;

    use super::*;

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            Python::initialize();
        });
    }

    #[rstest]
    fn test_config_error_to_pyvalue_err_preserves_display_text() {
        ensure_python_initialized();

        let error = ConfigError::invalid_format("rate_limit", "expected 'limit/HH:MM:SS'");

        Python::attach(|py| {
            let py_err = config_error_to_pyvalue_err(error);

            assert!(py_err.is_instance_of::<PyValueError>(py));
            assert_eq!(
                py_err.value(py).to_string(),
                "invalid rate_limit: expected 'limit/HH:MM:SS'"
            );
        });
    }
}

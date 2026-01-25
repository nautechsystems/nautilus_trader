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

pub mod actor;
pub mod cache;
pub mod clock;
pub mod custom;
pub mod enums;
pub mod fifo;
pub mod listener;
pub mod logging;
pub mod msgbus;
pub mod runtime;
pub mod signal;
pub mod timer;
pub mod xrate;

use pyo3::prelude::*;

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
    m.add_class::<crate::cache::CacheConfig>()?;
    m.add_class::<crate::cache::Cache>()?;
    m.add_class::<crate::python::actor::PyDataActor>()?;
    m.add_class::<crate::python::cache::PyCache>()?;
    m.add_class::<crate::python::fifo::PyFifoCache>()?;
    m.add_class::<crate::python::clock::PyClock>()?;
    m.add_class::<crate::python::logging::PyLogger>()?;
    m.add_class::<crate::actor::data_actor::DataActorConfig>()?;
    m.add_class::<crate::actor::data_actor::ImportableActorConfig>()?;
    m.add_class::<crate::msgbus::BusMessage>()?;
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

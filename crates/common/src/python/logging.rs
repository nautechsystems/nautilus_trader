// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use log::LevelFilter;
use nautilus_core::{UUID4, python::to_pyvalue_err};
use nautilus_model::identifiers::TraderId;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{
    enums::{LogColor, LogLevel},
    logging::{
        self, headers,
        logger::{self, LogGuard, LoggerConfig},
        logging_clock_set_realtime_mode, logging_clock_set_static_mode,
        logging_clock_set_static_time, logging_set_bypass, map_log_level_to_filter,
        parse_level_filter_str,
        writer::FileWriterConfig,
    },
};

#[pymethods]
impl LoggerConfig {
    /// Creates a [`LoggerConfig`] from a spec string.
    ///
    /// # Errors
    ///
    /// Returns a Python exception if the spec string is invalid.
    #[staticmethod]
    #[pyo3(name = "from_spec")]
    pub fn py_from_spec(spec: String) -> PyResult<Self> {
        Self::from_spec(&spec).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl FileWriterConfig {
    #[new]
    #[pyo3(signature = (directory=None, file_name=None, file_format=None, file_rotate=None))]
    #[must_use]
    pub fn py_new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
        file_rotate: Option<(u64, u32)>,
    ) -> Self {
        Self::new(directory, file_name, file_format, file_rotate)
    }
}

/// Initialize tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level only using
/// by passing a configuration using the `RUST_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
///
/// # Errors
///
/// Returns an error if tracing subscriber fails to initialize.
#[pyfunction()]
#[pyo3(name = "init_tracing")]
pub fn py_init_tracing() -> PyResult<()> {
    logging::init_tracing().map_err(to_pyvalue_err)
}

/// Initialize logging.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the [nautilus_trader](https://pypi.org/project/nautilus_trader) package.
/// Logging can be configured to filter components and write up to a specific level only
/// by passing a configuration using the `NAUTILUS_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
/// Initializes logging via Python interface.
///
/// # Errors
///
/// Returns a Python exception if logger initialization fails.
#[pyfunction]
#[pyo3(name = "init_logging")]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (trader_id, instance_id, level_stdout, level_file=None, component_levels=None, directory=None, file_name=None, file_format=None, file_rotate=None, is_colored=None, is_bypassed=None, print_config=None, log_components_only=None))]
pub fn py_init_logging(
    trader_id: TraderId,
    instance_id: UUID4,
    level_stdout: LogLevel,
    level_file: Option<LogLevel>,
    component_levels: Option<HashMap<String, String>>,
    directory: Option<String>,
    file_name: Option<String>,
    file_format: Option<String>,
    file_rotate: Option<(u64, u32)>,
    is_colored: Option<bool>,
    is_bypassed: Option<bool>,
    print_config: Option<bool>,
    log_components_only: Option<bool>,
) -> PyResult<LogGuard> {
    let level_file = level_file.map_or(LevelFilter::Off, map_log_level_to_filter);

    let component_levels = parse_component_levels(component_levels).map_err(to_pyvalue_err)?;

    let config = LoggerConfig::new(
        map_log_level_to_filter(level_stdout),
        level_file,
        component_levels,
        log_components_only.unwrap_or(false),
        is_colored.unwrap_or(true),
        print_config.unwrap_or(false),
    );

    let file_config = FileWriterConfig::new(directory, file_name, file_format, file_rotate);

    if is_bypassed.unwrap_or(false) {
        logging_set_bypass();
    }

    logging::init_logging(trader_id, instance_id, config, file_config).map_err(to_pyvalue_err)
}

#[pyfunction()]
#[pyo3(name = "logger_flush")]
pub fn py_logger_flush() {
    log::logger().flush();
}

fn parse_component_levels(
    original_map: Option<HashMap<String, String>>,
) -> anyhow::Result<HashMap<Ustr, LevelFilter>> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let level = parse_level_filter_str(&value)?;
                new_map.insert(ustr_key, level);
            }
            Ok(new_map)
        }
        None => Ok(HashMap::new()),
    }
}

/// Create a new log event.
#[pyfunction]
#[pyo3(name = "logger_log")]
pub fn py_logger_log(level: LogLevel, color: LogColor, component: &str, message: &str) {
    logger::log(level, color, Ustr::from(component), message);
}

/// Logs the standard Nautilus system header.
#[pyfunction]
#[pyo3(name = "log_header")]
pub fn py_log_header(trader_id: TraderId, machine_id: &str, instance_id: UUID4, component: &str) {
    headers::log_header(trader_id, machine_id, instance_id, Ustr::from(component));
}

/// Logs system information.
#[pyfunction]
#[pyo3(name = "log_sysinfo")]
pub fn py_log_sysinfo(component: &str) {
    headers::log_sysinfo(Ustr::from(component));
}

#[pyfunction]
#[pyo3(name = "logging_clock_set_static_mode")]
pub fn py_logging_clock_set_static_mode() {
    logging_clock_set_static_mode();
}

#[pyfunction]
#[pyo3(name = "logging_clock_set_realtime_mode")]
pub fn py_logging_clock_set_realtime_mode() {
    logging_clock_set_realtime_mode();
}

#[pyfunction]
#[pyo3(name = "logging_clock_set_static_time")]
pub fn py_logging_clock_set_static_time(time_ns: u64) {
    logging_clock_set_static_time(time_ns);
}

/// A thin wrapper around the global Rust logger which exposes ergonomic
/// logging helpers for Python code.
///
/// It mirrors the familiar Python `logging` interface while forwarding
/// all records through the Nautilus logging infrastructure so that log levels
/// and formatting remain consistent across Rust and Python.
#[pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "Logger",
    unsendable
)]
#[derive(Debug, Clone)]
pub struct PyLogger {
    name: Ustr,
}

impl PyLogger {
    pub fn new(name: &str) -> Self {
        Self {
            name: Ustr::from(name),
        }
    }
}

#[pymethods]
impl PyLogger {
    /// Create a new `Logger` instance.
    #[new]
    #[pyo3(signature = (name="Python"))]
    fn py_new(name: &str) -> Self {
        Self::new(name)
    }

    /// The component identifier carried by this logger.
    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    /// Emit a TRACE level record.
    #[pyo3(name = "trace")]
    fn py_trace(&self, message: &str, color: Option<LogColor>) {
        self._log(LogLevel::Trace, color, message);
    }

    /// Emit a DEBUG level record.
    #[pyo3(name = "debug")]
    fn py_debug(&self, message: &str, color: Option<LogColor>) {
        self._log(LogLevel::Debug, color, message);
    }

    /// Emit an INFO level record.
    #[pyo3(name = "info")]
    fn py_info(&self, message: &str, color: Option<LogColor>) {
        self._log(LogLevel::Info, color, message);
    }

    /// Emit a WARNING level record.
    #[pyo3(name = "warning")]
    fn py_warning(&self, message: &str, color: Option<LogColor>) {
        self._log(LogLevel::Warning, color, message);
    }

    /// Emit an ERROR level record.
    #[pyo3(name = "error")]
    fn py_error(&self, message: &str, color: Option<LogColor>) {
        self._log(LogLevel::Error, color, message);
    }

    /// Emit an ERROR level record with the active Python exception info.
    #[pyo3(name = "exception")]
    #[pyo3(signature = (message="", color=None))]
    fn py_exception(&self, py: Python, message: &str, color: Option<LogColor>) {
        let mut full_msg = message.to_owned();

        if pyo3::PyErr::occurred(py) {
            let err = PyErr::fetch(py);
            let err_str = err.to_string();
            if full_msg.is_empty() {
                full_msg = err_str;
            } else {
                full_msg = format!("{full_msg}: {err_str}");
            }
        }

        self._log(LogLevel::Error, color, &full_msg);
    }

    /// Flush buffered log records.
    #[pyo3(name = "flush")]
    fn py_flush(&self) {
        log::logger().flush();
    }

    fn _log(&self, level: LogLevel, color: Option<LogColor>, message: &str) {
        let color = color.unwrap_or(LogColor::Normal);
        logger::log(level, color, self.name, message);
    }
}

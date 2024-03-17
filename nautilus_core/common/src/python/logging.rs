// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{
    enums::{LogColor, LogLevel},
    logging::{
        self, headers,
        logger::{self, LogGuard, LoggerConfig},
        logging_set_bypass, map_log_level_to_filter, parse_level_filter_str,
        writer::FileWriterConfig,
    },
};

#[pymethods]
impl LoggerConfig {
    #[staticmethod]
    #[pyo3(name = "from_spec")]
    pub fn py_from_spec(spec: String) -> Self {
        LoggerConfig::from_spec(&spec)
    }
}

#[pymethods]
impl FileWriterConfig {
    #[new]
    pub fn py_new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
    ) -> Self {
        Self::new(directory, file_name, file_format)
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
#[pyfunction()]
#[pyo3(name = "init_tracing")]
pub fn py_init_tracing() {
    logging::init_tracing();
}

/// Initialize logging.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the main `nautilus_trader` package.
/// Logging can be configured to filter components and write up to a specific level only
/// by passing a configuration using the `NAUTILUS_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
#[pyo3(name = "init_logging")]
#[allow(clippy::too_many_arguments)]
pub fn py_init_logging(
    trader_id: TraderId,
    instance_id: UUID4,
    level_stdout: LogLevel,
    level_file: Option<LogLevel>,
    component_levels: Option<HashMap<String, String>>,
    directory: Option<String>,
    file_name: Option<String>,
    file_format: Option<String>,
    is_colored: Option<bool>,
    is_bypassed: Option<bool>,
    print_config: Option<bool>,
) -> LogGuard {
    let level_file = level_file
        .map(map_log_level_to_filter)
        .unwrap_or(LevelFilter::Off);

    let config = LoggerConfig::new(
        map_log_level_to_filter(level_stdout),
        level_file,
        parse_component_levels(component_levels),
        is_colored.unwrap_or(true),
        print_config.unwrap_or(false),
    );

    let file_config = FileWriterConfig::new(directory, file_name, file_format);

    if is_bypassed.unwrap_or(false) {
        logging_set_bypass();
    }

    logging::init_logging(trader_id, instance_id, config, file_config)
}

fn parse_component_levels(
    original_map: Option<HashMap<String, String>>,
) -> HashMap<Ustr, LevelFilter> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let value = parse_level_filter_str(&value);
                new_map.insert(ustr_key, value);
            }
            new_map
        }
        None => HashMap::new(),
    }
}

/// Create a new log event.
#[pyfunction]
#[pyo3(name = "logger_log")]
pub fn py_logger_log(level: LogLevel, color: LogColor, component: String, message: String) {
    logger::log(level, color, Ustr::from(&component), message.as_str());
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
    headers::log_sysinfo(Ustr::from(component))
}

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

use std::{
    collections::HashMap,
    ffi::{c_char, CStr},
    str::FromStr,
};

use log::LevelFilter;
use nautilus_core::{
    ffi::{
        parsing::{optional_bytes_to_json, u8_as_bool},
        string::{cstr_to_ustr, optional_cstr_to_string},
    },
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use serde_json::Value;
use ustr::Ustr;

use crate::{
    enums::{LogColor, LogLevel},
    logging::{self, FileWriterConfig, LoggerConfig},
};

/// Returns whether the core logger is enabled.
#[no_mangle]
pub extern "C" fn logging_is_initialized() -> u8 {
    log::log_enabled!(log::Level::Error) as u8
}

/// Initializes tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level only using
/// by passing a configuration using the `RUST_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[no_mangle]
pub extern "C" fn tracing_init() {
    logging::init_tracing();
}

/// Initializes logging.
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
///
/// - Assume `directory_ptr` is either NULL or a valid C string pointer.
/// - Assume `file_name_ptr` is either NULL or a valid C string pointer.
/// - Assume `file_format_ptr` is either NULL or a valid C string pointer.
/// - Assume `component_level_ptr` is either NULL or a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logging_init(
    trader_id: TraderId,
    instance_id: UUID4,
    level_stdout: LogLevel,
    level_file: LogLevel,
    file_logging: u8,
    directory_ptr: *const c_char,
    file_name_ptr: *const c_char,
    file_format_ptr: *const c_char,
    component_levels_ptr: *const c_char,
    is_colored: u8,
    is_bypassed: u8,
    print_config: u8,
) {
    let level_stdout = map_log_level_to_filter(level_stdout);
    let level_file = if u8_as_bool(file_logging) {
        map_log_level_to_filter(level_file)
    } else {
        LevelFilter::Off
    };

    let component_levels_json = optional_bytes_to_json(component_levels_ptr);
    let component_levels = parse_component_levels(component_levels_json);

    let config = LoggerConfig::new(
        level_stdout,
        level_file,
        component_levels,
        u8_as_bool(is_colored),
        u8_as_bool(is_bypassed),
        u8_as_bool(print_config),
    );

    let directory = optional_cstr_to_string(directory_ptr);
    let file_name = optional_cstr_to_string(file_name_ptr);
    let file_format = optional_cstr_to_string(file_format_ptr);

    let file_config = FileWriterConfig::new(directory, file_name, file_format);

    logging::init_logging(trader_id, instance_id, config, file_config);
}

fn map_log_level_to_filter(log_level: LogLevel) -> LevelFilter {
    match log_level {
        LogLevel::Debug => LevelFilter::Debug,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Warning => LevelFilter::Warn,
        LogLevel::Error => LevelFilter::Error,
    }
}

fn parse_level_filter_str(s: &str) -> LevelFilter {
    let mut log_level_str = s.to_string().to_uppercase();
    if log_level_str == "WARNING" {
        log_level_str = "WARN".to_string()
    }
    LevelFilter::from_str(&log_level_str)
        .unwrap_or_else(|_| panic!("Invalid `LevelFilter` string, was {log_level_str}"))
}

fn parse_component_levels(
    original_map: Option<HashMap<String, Value>>,
) -> HashMap<Ustr, LevelFilter> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let value = parse_level_filter_str(&value.to_string());
                new_map.insert(ustr_key, value);
            }
            new_map
        }
        None => HashMap::new(),
    }
}

/// Creates a new log event.
///
/// # Safety
///
/// - Assumes `component_ptr` is a valid C string pointer.
/// - Assumes `message_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logger_log(
    timestamp_ns: u64,
    level: LogLevel,
    color: LogColor,
    component_ptr: *const c_char,
    message_ptr: *const c_char,
) {
    let component = cstr_to_ustr(component_ptr);
    let message = CStr::from_ptr(message_ptr).to_string_lossy();

    logging::log(timestamp_ns, level, color, component, message);
}

/// Flushes logger buffers.
#[no_mangle]
pub extern "C" fn logger_flush() {
    log::logger().flush()
}

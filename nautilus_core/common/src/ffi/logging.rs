// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{env, ffi::c_char};

use log::{
    debug, error, info,
    kv::{ToValue, Value},
    warn,
};
use nautilus_core::{
    ffi::string::{cstr_to_string, optional_cstr_to_string},
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use tracing_subscriber::EnvFilter;

use crate::{
    clock::get_atomic_clock,
    enums::{LogColor, LogLevel},
    logging::{FileWriterConfig, Logger, LoggerConfig},
};

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
#[no_mangle]
pub extern "C" fn tracing_init() {
    // Skip tracing initialization if `RUST_LOG` is not set
    if let Ok(v) = env::var("RUST_LOG") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(v.clone()))
            .try_init()
            .unwrap_or_else(|e| eprintln!("Cannot set tracing subscriber because of error: {e}"));
        println!("Initialized tracing logs with RUST_LOG={v}");
    }
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
///
/// - Assume `config_spec_ptr` is a valid C string pointer.
/// - Assume `file_directory_ptr` is either NULL or a valid C string pointer.
/// - Assume `file_name_ptr` is either NULL or a valid C string pointer.
/// - Assume `file_format_ptr` is either NULL or a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logging_init(
    trader_id: TraderId,
    instance_id: UUID4,
    config_spec_ptr: *const c_char,
    file_directory_ptr: *const c_char,
    file_name_ptr: *const c_char,
    file_format_ptr: *const c_char,
) {
    let config_spec = cstr_to_string(config_spec_ptr);
    let config = LoggerConfig::from_spec(&config_spec);

    let directory = optional_cstr_to_string(file_directory_ptr);
    let file_name = optional_cstr_to_string(file_name_ptr);
    let file_format = optional_cstr_to_string(file_format_ptr);
    let file_writer_config = FileWriterConfig::new(directory, file_name, file_format);

    Logger::init_with_config(
        trader_id,
        instance_id,
        file_writer_config,
        config,
        Some(get_atomic_clock()),
    );
}

/// Create a new log event.
///
/// # Safety
///
/// - Assumes `component_ptr` is a valid C string pointer.
/// - Assumes `message_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logger_log(
    level: LogLevel,
    color: LogColor,
    component_ptr: *const c_char,
    message_ptr: *const c_char,
) {
    // TODO use ustr for components
    let component = cstr_to_string(component_ptr);
    let message = cstr_to_string(message_ptr);
    match level {
        LogLevel::Debug => {
            debug!(component = component.to_value(), color = Value::from(color as u8); "{}", message);
        }
        LogLevel::Info => {
            info!(component = component.to_value(), color = Value::from(color as u8); "{}", message);
        }
        LogLevel::Warning => {
            warn!(component = component.to_value(), color = Value::from(color as u8); "{}", message);
        }
        LogLevel::Error => {
            error!(component = component.to_value(), color = Value::from(color as u8); "{}", message);
        }
    }
}

/// Flush logger buffers.
#[no_mangle]
pub extern "C" fn logger_flush() {
    log::logger().flush()
}

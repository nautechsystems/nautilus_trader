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

use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::{
    ffi::{
        parsing::optional_bytes_to_json,
        string::{cstr_to_string, optional_cstr_to_string, str_to_cstr},
    },
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;

use crate::{
    enums::{LogColor, LogLevel},
    logging::Logger,
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying [`Logger`].
///
/// This struct wraps `Logger` in a way that makes it compatible with C function
/// calls, enabling interaction with `Logger` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `Logger_API` to be
/// dereferenced to `Logger`, providing access to `Logger`'s methods without
/// having to manually access the underlying `Logger` instance.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct Logger_API(Box<Logger>);

impl Deref for Logger_API {
    type Target = Logger;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Logger_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Creates a new logger.
///
/// # Safety
///
/// - Assumes `trader_id_ptr` is a valid C string pointer.
/// - Assumes `machine_id_ptr` is a valid C string pointer.
/// - Assumes `instance_id_ptr` is a valid C string pointer.
/// - Assumes `directory_ptr` is a valid C string pointer or NULL.
/// - Assumes `file_name_ptr` is a valid C string pointer or NULL.
/// - Assumes `file_format_ptr` is a valid C string pointer or NULL.
/// - Assumes `component_levels_ptr` is a valid C string pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn logger_new(
    trader_id_ptr: *const c_char,
    machine_id_ptr: *const c_char,
    instance_id_ptr: *const c_char,
    level_stdout: LogLevel,
    level_file: LogLevel,
    file_logging: u8,
    directory_ptr: *const c_char,
    file_name_ptr: *const c_char,
    file_format_ptr: *const c_char,
    component_levels_ptr: *const c_char,
    is_colored: u8,
    is_bypassed: u8,
) -> Logger_API {
    Logger_API(Box::new(Logger::new(
        TraderId::from(cstr_to_string(trader_id_ptr).as_str()),
        String::from(&cstr_to_string(machine_id_ptr)),
        UUID4::from(cstr_to_string(instance_id_ptr).as_str()),
        level_stdout,
        if file_logging != 0 {
            Some(level_file)
        } else {
            None
        },
        optional_cstr_to_string(directory_ptr),
        optional_cstr_to_string(file_name_ptr),
        optional_cstr_to_string(file_format_ptr),
        optional_bytes_to_json(component_levels_ptr),
        is_colored != 0,
        is_bypassed != 0,
    )))
}

#[no_mangle]
pub extern "C" fn logger_drop(logger: Logger_API) {
    drop(logger); // Memory freed here
}

#[no_mangle]
pub extern "C" fn logger_get_trader_id_cstr(logger: &Logger_API) -> *const c_char {
    str_to_cstr(&logger.trader_id.to_string())
}

#[no_mangle]
pub extern "C" fn logger_get_machine_id_cstr(logger: &Logger_API) -> *const c_char {
    str_to_cstr(&logger.machine_id)
}

#[no_mangle]
pub extern "C" fn logger_get_instance_id(logger: &Logger_API) -> UUID4 {
    logger.instance_id
}

#[no_mangle]
pub extern "C" fn logger_is_colored(logger: &Logger_API) -> u8 {
    u8::from(logger.is_colored)
}

#[no_mangle]
pub extern "C" fn logger_is_bypassed(logger: &Logger_API) -> u8 {
    u8::from(logger.is_bypassed)
}

/// Create a new log event.
///
/// # Safety
///
/// - Assumes `component_ptr` is a valid C string pointer.
/// - Assumes `message_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logger_log(
    logger: &mut Logger_API,
    timestamp_ns: u64,
    level: LogLevel,
    color: LogColor,
    component_ptr: *const c_char,
    message_ptr: *const c_char,
) {
    let component = cstr_to_string(component_ptr);
    let message = cstr_to_string(message_ptr);
    logger.send(timestamp_ns, level, color, component, message);
}

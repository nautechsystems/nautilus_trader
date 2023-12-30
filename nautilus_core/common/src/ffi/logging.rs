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

use std::ffi::c_char;

use log::{
    debug, error, info,
    kv::{ToValue, Value},
    warn,
};
use nautilus_core::ffi::string::cstr_to_string;

use crate::enums::{LogColor, LogLevel};

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
            debug!(component = component.to_value(), color = Value::from(color as u8); "{}", message)
        }
        LogLevel::Info => {
            info!(component = component.to_value(), color = Value::from(color as u8); "{}", message)
        }
        LogLevel::Warning => {
            warn!(component = component.to_value(), color = Value::from(color as u8); "{}", message)
        }
        LogLevel::Error => {
            error!(component = component.to_value(), color = Value::from(color as u8); "{}", message)
        }
        // Don't support this anymore
        LogLevel::Critical => {}
    }
}

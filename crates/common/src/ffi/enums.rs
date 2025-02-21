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

use std::{ffi::c_char, str::FromStr};

use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};

use crate::enums::{ComponentState, ComponentTrigger, LogColor, LogLevel};

#[unsafe(no_mangle)]
pub extern "C" fn component_state_to_cstr(value: ComponentState) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn component_state_from_cstr(ptr: *const c_char) -> ComponentState {
    let value = unsafe { cstr_as_str(ptr) };
    ComponentState::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ComponentState` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn component_trigger_to_cstr(value: ComponentTrigger) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn component_trigger_from_cstr(ptr: *const c_char) -> ComponentTrigger {
    let value = unsafe { cstr_as_str(ptr) };
    ComponentTrigger::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ComponentTrigger` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn log_level_to_cstr(value: LogLevel) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn log_level_from_cstr(ptr: *const c_char) -> LogLevel {
    let value = unsafe { cstr_as_str(ptr) };
    LogLevel::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LogLevel` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn log_color_to_cstr(value: LogColor) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn log_color_from_cstr(ptr: *const c_char) -> LogColor {
    let value = unsafe { cstr_as_str(ptr) };
    LogColor::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LogColor` enum string value, was '{value}'"))
}

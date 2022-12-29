// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Debug;
use std::str::FromStr;

use pyo3::ffi;
use strum::{Display, EnumString, FromRepr};

use nautilus_core::string::{pystr_to_string, string_to_pystr};

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialOrd, PartialEq, Eq, FromRepr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LogLevel {
    #[strum(serialize = "DBG", serialize = "DEBUG")]
    Debug = 10,
    #[strum(serialize = "INF", serialize = "INFO")]
    Info = 20,
    #[strum(serialize = "WRN", serialize = "WARNING")]
    Warning = 30,
    #[strum(serialize = "ERR", serialize = "ERROR")]
    Error = 40,
    #[strum(serialize = "CRT", serialize = "CRITICAL")]
    Critical = 50,
}

// Override `strum` implementation
impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogLevel::Debug => "DBG",
            LogLevel::Info => "INF",
            LogLevel::Warning => "WRN",
            LogLevel::Error => "ERR",
            LogLevel::Critical => "CRT",
        };
        write!(f, "{}", display)
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LogColor {
    #[strum(serialize = "")]
    Normal = 0,
    #[strum(serialize = "\x1b[92m")]
    Green = 1,
    #[strum(serialize = "\x1b[94m")]
    Blue = 2,
    #[strum(serialize = "\x1b[35m")]
    Magenta = 3,
    #[strum(serialize = "\x1b[36m")]
    Cyan = 4,
    #[strum(serialize = "\x1b[1;33m")]
    Yellow = 5,
    #[strum(serialize = "\x1b[1;31m")]
    Red = 6,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LogFormat {
    #[strum(serialize = "\x1b[95m")]
    Header,
    #[strum(serialize = "\x1b[0m")]
    Endc,
    #[strum(serialize = "\x1b[1m")]
    Bold,
    #[strum(serialize = "\x1b[4m")]
    Underline,
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn log_level_to_pystr(value: LogLevel) -> *mut ffi::PyObject {
    string_to_pystr(&value.to_string())
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn log_level_from_pystr(ptr: *mut ffi::PyObject) -> LogLevel {
    LogLevel::from_str(&pystr_to_string(ptr)).expect("Error when parsing enum string value")
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn log_color_to_pystr(value: LogColor) -> *mut ffi::PyObject {
    string_to_pystr(&value.to_string())
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn log_color_from_pystr(ptr: *mut ffi::PyObject) -> LogColor {
    LogColor::from_str(&pystr_to_string(ptr)).expect("Error when parsing enum string value")
}

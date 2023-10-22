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

use std::{ffi::c_char, fmt::Debug, str::FromStr};

use nautilus_core::ffi::string::{cstr_to_string, str_to_cstr};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, FromRepr};

/// The state of a component within the system.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FromRepr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum ComponentState {
    /// When a component is instantiated, but not yet ready to fulfill its specification.
    PreInitialized = 0,
    /// When a component is able to be started.
    Ready = 1,
    /// When a component is executing its actions on `start`.
    Starting = 2,
    /// When a component is operating normally and can fulfill its specification.
    Running = 3,
    /// When a component is executing its actions on `stop`.
    Stopping = 4,
    /// When a component has successfully stopped.
    Stopped = 5,
    /// When a component is started again after its initial start.
    Resuming = 6,
    /// When a component is executing its actions on `reset`.
    Resetting = 7,
    /// When a component is executing its actions on `dispose`.
    Disposing = 8,
    /// When a component has successfully shut down and released all of its resources.
    Disposed = 9,
    /// When a component is executing its actions on `degrade`.
    Degrading = 10,
    /// When a component has successfully degraded and may not meet its full specification.
    Degraded = 11,
    /// When a component is executing its actions on `fault`.
    Faulting = 12,
    /// When a component has successfully shut down due to a detected fault.
    Faulted = 13,
}

/// A trigger condition for a component within the system.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FromRepr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum ComponentTrigger {
    /// A trigger for the component to initialize.
    Initialize = 1,
    /// A trigger for the component to start.
    Start = 2,
    /// A trigger when the component has successfully started.
    StartCompleted = 3,
    /// A trigger for the component to stop.
    Stop = 4,
    /// A trigger when the component has successfully stopped.
    StopCompleted = 5,
    /// A trigger for the component to resume (after being stopped).
    Resume = 6,
    /// A trigger when the component has successfully resumed.
    ResumeCompleted = 7,
    /// A trigger for the component to reset.
    Reset = 8,
    /// A trigger when the component has successfully reset.
    ResetCompleted = 9,
    /// A trigger for the component to dispose and release resources.
    Dispose = 10,
    /// A trigger when the component has successfully disposed.
    DisposeCompleted = 11,
    /// A trigger for the component to degrade.
    Degrade = 12,
    /// A trigger when the component has successfully degraded.
    DegradeCompleted = 13,
    /// A trigger for the component to fault.
    Fault = 14,
    /// A trigger when the component has successfully faulted.
    FaultCompleted = 15,
}

/// The log level for log messages.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FromRepr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum LogLevel {
    /// The **DBG** debug log level.
    #[strum(serialize = "DBG", serialize = "DEBUG")]
    #[serde(rename = "DEBUG")]
    Debug = 10,
    /// The **INF** info log level.
    #[strum(serialize = "INF", serialize = "INFO")]
    #[serde(rename = "INFO")]
    Info = 20,
    /// The **WRN** warning log level.
    #[strum(serialize = "WRN", serialize = "WARNING")]
    #[serde(rename = "WARNING")]
    Warning = 30,
    /// The **ERR** error log level.
    #[strum(serialize = "ERR", serialize = "ERROR")]
    #[serde(rename = "ERROR")]
    Error = 40,
    /// The **CRT** critical log level.
    #[strum(serialize = "CRT", serialize = "CRITICAL")]
    #[serde(rename = "CRITICAL")]
    Critical = 50,
}

// Override `strum` implementation
impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            Self::Debug => "DBG",
            Self::Info => "INF",
            Self::Warning => "WRN",
            Self::Error => "ERR",
            Self::Critical => "CRT",
        };
        write!(f, "{display}")
    }
}

/// The log color for log messages.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    FromRepr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum LogColor {
    /// The default/normal log color.
    #[strum(serialize = "")]
    Normal = 0,
    /// The green log color, typically used with [`LogLevel::Info`] log levels and associated with success events.
    #[strum(serialize = "\x1b[92m")]
    Green = 1,
    /// The blue log color, typically used with [`LogLevel::Info`] log levels and associated with user actions.
    #[strum(serialize = "\x1b[94m")]
    Blue = 2,
    /// The magenta log color, typically used with [`LogLevel::Info`] log levels.
    #[strum(serialize = "\x1b[35m")]
    Magenta = 3,
    /// The cyan log color, typically used with [`LogLevel::Info`] log levels.
    #[strum(serialize = "\x1b[36m")]
    Cyan = 4,
    /// The yellow log color, typically used with [`LogLevel::Warning`] log levels.
    #[strum(serialize = "\x1b[1;33m")]
    Yellow = 5,
    /// The red log color, typically used with [`LogLevel::Error`] or [`LogLevel::Critical`] log levels.
    #[strum(serialize = "\x1b[1;31m")]
    Red = 6,
}

/// An ANSI log line format specifier.
/// This is used for formatting log messages with ANSI escape codes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum LogFormat {
    /// Header log format. This ANSI escape code is used for magenta text color,
    /// often used for headers or titles in the log output.
    #[strum(serialize = "\x1b[95m")]
    Header,

    /// Endc log format. This ANSI escape code is used to reset all format attributes
    /// to their defaults. It should be used after applying other formats.
    #[strum(serialize = "\x1b[0m")]
    Endc,

    /// Bold log format. This ANSI escape code is used to make the text bold in the log output.
    #[strum(serialize = "\x1b[1m")]
    Bold,

    /// Underline log format. This ANSI escape code is used to underline the text in the log output.
    #[strum(serialize = "\x1b[4m")]
    Underline,
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn component_state_to_cstr(value: ComponentState) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn component_state_from_cstr(ptr: *const c_char) -> ComponentState {
    let value = cstr_to_string(ptr);
    ComponentState::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `ComponentState` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn component_trigger_to_cstr(value: ComponentTrigger) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn component_trigger_from_cstr(ptr: *const c_char) -> ComponentTrigger {
    let value = cstr_to_string(ptr);
    ComponentTrigger::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `ComponentTrigger` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn log_level_to_cstr(value: LogLevel) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn log_level_from_cstr(ptr: *const c_char) -> LogLevel {
    let value = cstr_to_string(ptr);
    LogLevel::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `LogLevel` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn log_color_to_cstr(value: LogColor) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn log_color_from_cstr(ptr: *const c_char) -> LogColor {
    let value = cstr_to_string(ptr);
    LogColor::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `LogColor` enum string value, was '{value}'"))
}

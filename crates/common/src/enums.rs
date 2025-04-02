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

//! Enumerations for common components.

use std::fmt::Debug;

use log::Level;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, FromRepr};

/// The state of a component within the system.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum ComponentState {
    /// When a component is instantiated, but not yet ready to fulfill its specification.
    #[default]
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
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

/// Represents the environment context for a Nautilus system.
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum Environment {
    Backtest,
    Sandbox,
    Live,
}

/// The log level for log messages.
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum LogLevel {
    /// The **OFF** log level. A level lower than all other log levels (off).
    #[strum(serialize = "OFF")]
    #[serde(rename = "OFF")]
    Off = 0,
    /// The **TRACE** log level. Only available in Rust for debug/development builds.
    #[strum(serialize = "TRACE")]
    #[serde(rename = "TRACE")]
    Trace = 1,
    /// The **DEBUG** log level.
    #[strum(serialize = "DEBUG")]
    #[serde(rename = "DEBUG")]
    Debug = 2,
    /// The **INFO** log level.
    #[strum(serialize = "INFO")]
    #[serde(rename = "INFO")]
    Info = 3,
    /// The **WARNING** log level.
    #[strum(serialize = "WARN", serialize = "WARNING")]
    #[serde(rename = "WARNING")]
    Warning = 4,
    /// The **ERROR** log level.
    #[strum(serialize = "ERROR")]
    #[serde(rename = "ERROR")]
    Error = 5,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum LogColor {
    /// The default/normal log color.
    #[strum(serialize = "NORMAL")]
    Normal = 0,
    /// The green log color, typically used with [`LogLevel::Info`] log levels and associated with success events.
    #[strum(serialize = "GREEN")]
    Green = 1,
    /// The blue log color, typically used with [`LogLevel::Info`] log levels and associated with user actions.
    #[strum(serialize = "BLUE")]
    Blue = 2,
    /// The magenta log color, typically used with [`LogLevel::Info`] log levels.
    #[strum(serialize = "MAGENTA")]
    Magenta = 3,
    /// The cyan log color, typically used with [`LogLevel::Info`] log levels.
    #[strum(serialize = "CYAN")]
    Cyan = 4,
    /// The yellow log color, typically used with [`LogLevel::Warning`] log levels.
    #[strum(serialize = "YELLOW")]
    Yellow = 5,
    /// The red log color, typically used with [`LogLevel::Error`] level.
    #[strum(serialize = "RED")]
    Red = 6,
}

impl LogColor {
    #[must_use]
    pub const fn as_ansi(&self) -> &str {
        match *self {
            Self::Normal => "",
            Self::Green => "\x1b[92m",
            Self::Blue => "\x1b[94m",
            Self::Magenta => "\x1b[35m",
            Self::Cyan => "\x1b[36m",
            Self::Yellow => "\x1b[1;33m",
            Self::Red => "\x1b[1;31m",
        }
    }
}

impl From<u8> for LogColor {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Green,
            2 => Self::Blue,
            3 => Self::Magenta,
            4 => Self::Cyan,
            5 => Self::Yellow,
            6 => Self::Red,
            _ => Self::Normal,
        }
    }
}

impl From<Level> for LogColor {
    fn from(value: Level) -> Self {
        match value {
            Level::Error => Self::Red,
            Level::Warn => Self::Yellow,
            Level::Info => Self::Normal,
            Level::Debug => Self::Normal,
            Level::Trace => Self::Normal,
        }
    }
}

/// An ANSI log line format specifier.
/// This is used for formatting log messages with ANSI escape codes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
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

/// The serialization encoding.
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum SerializationEncoding {
    /// The MessagePack encoding.
    #[serde(rename = "msgpack")]
    MsgPack = 0,
    /// The JavaScript Object Notation (JSON) encoding.
    #[serde(rename = "json")]
    Json = 1,
}

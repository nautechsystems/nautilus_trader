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

use std::{
    fmt::Display,
    io::{self, BufWriter, Stderr, Stdout, Write},
    ops::{Deref, DerefMut},
};

use pyo3::ffi;

use nautilus_core::datetime::unix_nanos_to_iso8601;
use nautilus_core::string::{pystr_to_string, string_to_pystr};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Debug = 10,
    Info = 20,
    Warning = 30,
    Error = 40,
    Critical = 50,
}

impl Display for LogLevel {
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
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogColor {
    Normal = 0,
    Green = 1,
    Blue = 2,
    Magenta = 3,
    Cyan = 4,
    Yellow = 5,
    Red = 6,
}

impl Display for LogColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogColor::Normal => "",
            LogColor::Green => "\x1b[92m",
            LogColor::Blue => "\x1b[94m",
            LogColor::Magenta => "\x1b[35m",
            LogColor::Cyan => "\x1b[36m",
            LogColor::Yellow => "\x1b[1;33m",
            LogColor::Red => "\x1b[1;31m",
        };
        write!(f, "{}", display)
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogFormat {
    Header,
    Endc,
    Bold,
    Underline,
}

impl Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogFormat::Header => "\x1b[95m",
            LogFormat::Endc => "\x1b[0m",
            LogFormat::Bold => "\x1b[1m",
            LogFormat::Underline => "\x1b[4m",
        };
        write!(f, "{}", display)
    }
}

pub struct Logger {
    pub trader_id: TraderId,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub level_stdout: LogLevel,
    pub is_bypassed: bool,
    out: BufWriter<Stdout>,
    err: BufWriter<Stderr>,
}

impl Logger {
    fn new(
        trader_id: TraderId,
        machine_id: String,
        instance_id: UUID4,
        level_stdout: LogLevel,
        is_bypassed: bool,
    ) -> Self {
        Logger {
            trader_id,
            machine_id,
            instance_id,
            level_stdout,
            is_bypassed,
            out: BufWriter::new(io::stdout()),
            err: BufWriter::new(io::stderr()),
        }
    }

    #[inline]
    fn log(
        &mut self,
        timestamp_ns: u64,
        level: LogLevel,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        let fmt_line = format!(
            "{bold}{ts}{startc} {color}[{level}] {trader_id}.{component}: {msg}{endc}\n",
            bold = LogFormat::Bold,
            ts = unix_nanos_to_iso8601(timestamp_ns),
            startc = LogFormat::Endc,
            color = color,
            level = level,
            trader_id = self.trader_id,
            component = component,
            msg = msg,
            endc = LogFormat::Endc,
        );
        if level >= LogLevel::Error {
            self.err.write_all(fmt_line.as_bytes())?;
            self.err.flush()
        } else if level >= self.level_stdout {
            self.out.write_all(fmt_line.as_bytes())?;
            self.out.flush()
        } else {
            Ok(())
        }
    }

    #[inline]
    pub fn debug(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::Debug, color, component, msg)
    }

    #[inline]
    pub fn info(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::Info, color, component, msg)
    }

    #[inline]
    pub fn warn(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::Warning, color, component, msg)
    }

    #[inline]
    pub fn error(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::Error, color, component, msg)
    }

    #[inline]
    pub fn critical(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: &str,
        msg: &str,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::Critical, color, component, msg)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), io::Error> {
        self.out.flush()?;
        self.err.flush()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Logger is not C FFI safe, so we box and pass it as an opaque pointer.
/// This works because Logger fields don't need to be accessed, only functions
/// are called.
#[repr(C)]
pub struct CLogger(Box<Logger>);

impl Deref for CLogger {
    type Target = Logger;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CLogger {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Creates a logger from a valid Python object pointer and a defined logging level.
///
/// # Safety
/// - Assumes `trader_id_ptr` is borrowed from a valid Python UTF-8 `str`.
/// - Assumes `machine_id_ptr` is borrowed from a valid Python UTF-8 `str`.
/// - Assumes `instance_id_ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn logger_new(
    trader_id_ptr: *mut ffi::PyObject,
    machine_id_ptr: *mut ffi::PyObject,
    instance_id_ptr: *mut ffi::PyObject,
    level_stdout: LogLevel,
    is_bypassed: u8,
) -> CLogger {
    CLogger(Box::new(Logger::new(
        TraderId::new(pystr_to_string(trader_id_ptr).as_str()),
        String::from(pystr_to_string(machine_id_ptr).as_str()),
        UUID4::from(pystr_to_string(instance_id_ptr).as_str()),
        level_stdout,
        is_bypassed != 0,
    )))
}

#[no_mangle]
pub extern "C" fn logger_free(mut logger: CLogger) {
    let _ = logger.flush(); // ignore flushing error if any
    drop(logger); // Memory freed here
}

#[no_mangle]
pub extern "C" fn flush(logger: &mut CLogger) {
    let _ = logger.flush();
}

/// Return the loggers trader ID.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn logger_get_trader_id(logger: &CLogger) -> *mut ffi::PyObject {
    string_to_pystr(logger.trader_id.to_string().as_str())
}

/// Return the loggers machine ID.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn logger_get_machine_id(logger: &CLogger) -> *mut ffi::PyObject {
    string_to_pystr(logger.machine_id.as_str())
}

#[no_mangle]
pub extern "C" fn logger_get_instance_id(logger: &CLogger) -> UUID4 {
    logger.instance_id.clone()
}

#[no_mangle]
pub extern "C" fn logger_is_bypassed(logger: &CLogger) -> u8 {
    logger.is_bypassed as u8
}

/// Log a message from valid Python object pointers.
///
/// # Safety
/// - Assumes `component_ptr` is borrowed from a valid Python UTF-8 `str`.
/// - Assumes `msg_ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn logger_log(
    logger: &mut CLogger,
    timestamp_ns: u64,
    level: LogLevel,
    color: LogColor,
    component_ptr: *mut ffi::PyObject,
    msg_ptr: *mut ffi::PyObject,
) {
    let component = pystr_to_string(component_ptr);
    let msg = pystr_to_string(msg_ptr);
    let _ = logger.log(timestamp_ns, level, color, component.as_str(), msg.as_str());
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::logging::{LogColor, LogLevel, Logger};
    use nautilus_core::uuid::UUID4;
    use nautilus_model::identifiers::trader_id::TraderId;

    #[test]
    fn test_new_logger() {
        let logger = Logger::new(
            TraderId::new("TRADER-000"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Debug,
            false,
        );

        assert_eq!(logger.trader_id, TraderId::new("TRADER-000"));
        assert_eq!(logger.level_stdout, LogLevel::Debug);
    }

    #[test]
    fn test_logger_debug() {
        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            false,
        );

        logger
            .info(
                1650000000000000,
                LogColor::Normal,
                "RiskEngine",
                "This is a test.",
            )
            .expect("Error while logging");
    }
}

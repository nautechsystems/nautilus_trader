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

use nautilus_core::string::pystr_to_string;
use pyo3::{ffi, prelude::*, types::PyString, Python};

#[repr(C)]
#[pyclass]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogLevel {
    DBG,
    INF,
    WRN,
    ERR,
    CRT,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogLevel::DBG => "DEBUG",
            LogLevel::INF => "INFO",
            LogLevel::WRN => "WARNING",
            LogLevel::ERR => "ERROR",
            LogLevel::CRT => "CRITICAL",
        };
        write!(f, "{}", display)
    }
}

#[repr(C)]
#[pyclass]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogFormat {
    HEADER,
    GREEN,
    BLUE,
    MAGENTA,
    CYAN,
    YELLOW,
    RED,
    ENDC,
    BOLD,
    UNDERLINE,
}

impl Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogFormat::HEADER => "\x1b[95m",
            LogFormat::GREEN => "\x1b[92m",
            LogFormat::BLUE => "\x1b[94m",
            LogFormat::MAGENTA => "\x1b[35m",
            LogFormat::CYAN => "\x1b[36m",
            LogFormat::YELLOW => "\x1b[1;33m",
            LogFormat::RED => "\x1b[1;31m",
            LogFormat::ENDC => "\x1b[0m",
            LogFormat::BOLD => "\x1b[1m",
            LogFormat::UNDERLINE => "\x1b[4m",
        };
        write!(f, "{}", display)
    }
}

/// BufWriter is not C ffi safe
#[pyclass]
pub struct Logger {
    trader_id: Box<String>,
    level_stdout: LogLevel,
    out: BufWriter<Stdout>,
    err: BufWriter<Stderr>,
}

#[pymethods]
impl Logger {
    #[new]
    fn new(trader_id: Option<String>, level_stdout: LogLevel) -> Self {
        Logger {
            trader_id: trader_id
                .map_or_else(|| Box::new("TRADER-000".to_string()), |val| Box::new(val)),
            level_stdout,
            out: BufWriter::new(io::stdout()),
            err: BufWriter::new(io::stderr()),
        }
    }

    #[inline]
    fn log(
        &mut self,
        timestamp_ns: u64,
        level: LogLevel,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        let fmt_line = format!(
            "{bold}{dt}{startc} {color}[{level}] {trader_id}{component}: {msg}{endc}\n",
            bold = LogFormat::BOLD,
            dt = timestamp_ns,
            startc = LogFormat::ENDC,
            color = color,
            level = level,
            trader_id = self.trader_id,
            component = component,
            msg = msg,
            endc = LogFormat::ENDC,
        );
        if level >= LogLevel::ERR {
            self.out.write_all(fmt_line.as_bytes())
        } else if level >= self.level_stdout {
            self.err.write_all(fmt_line.as_bytes())
        } else {
            Ok(())
        }
    }

    fn debug(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::DBG, color, component, msg)
    }
    fn info(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::INF, color, component, msg)
    }
    fn warning(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::WRN, color, component, msg)
    }
    fn error(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::ERR, color, component, msg)
    }
    fn critical(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::CRT, color, component, msg)
    }
    fn flush(&mut self) -> Result<(), io::Error> {
        self.out.flush()?;
        self.err.flush()
    }
}

/// C API

/// BufWriter is not C ffi safe. Box logger and pass it to as an opaque
/// pointer. This works because Logger fields don't need to be accessed only
/// functions are called.
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

#[no_mangle]
pub extern "C" fn clogger_free(mut logger: CLogger) {
    let _ = logger.flush(); // ignore flushing error if any
    drop(logger); // Memory freed here
}

/// Creates a logger from a valid Python object pointer
/// and a defined logging level
///
/// # Safety
///
/// - `ptr` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn clogger_new(ptr: *mut ffi::PyObject, level_stdout: LogLevel) -> CLogger {
    CLogger(Box::new(Logger {
        trader_id: Box::new(pystr_to_string(ptr)),
        level_stdout,
        out: BufWriter::new(io::stdout()),
        err: BufWriter::new(io::stderr()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn debug(
    logger: &mut CLogger,
    timestamp_ns: u64,
    color: LogFormat,
    component: &PyString,
    msg: &PyString,
) {
    let _ = logger.log(timestamp_ns, LogLevel::DBG, color, component, msg);
}

#[no_mangle]
pub unsafe extern "C" fn info(
    logger: &mut CLogger,
    timestamp_ns: u64,
    color: LogFormat,
    component: &PyString,
    msg: &PyString,
) {
    let _ = logger.log(timestamp_ns, LogLevel::INF, color, component, msg);
}

#[no_mangle]
pub unsafe extern "C" fn warn(
    logger: &mut CLogger,
    timestamp_ns: u64,
    color: LogFormat,
    component: &PyString,
    msg: &PyString,
) {
    let _ = logger.log(timestamp_ns, LogLevel::WRN, color, component, msg);
}

#[no_mangle]
pub unsafe extern "C" fn error(
    logger: &mut CLogger,
    timestamp_ns: u64,
    color: LogFormat,
    component: &PyString,
    msg: &PyString,
) {
    let _ = logger.log(timestamp_ns, LogLevel::ERR, color, component, msg);
}

#[no_mangle]
pub unsafe extern "C" fn critical(
    logger: &mut CLogger,
    timestamp_ns: u64,
    color: LogFormat,
    component: &PyString,
    msg: &PyString,
) {
    let _ = logger.log(timestamp_ns, LogLevel::CRT, color, component, msg);
}

#[no_mangle]
pub unsafe extern "C" fn flush(logger: &mut CLogger) {
    let _ = logger.flush();
}

/// Register python sub module
pub fn register_module(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let logging = PyModule::new(py, "logging")?;
    logging.add_class::<LogFormat>()?;
    logging.add_class::<LogLevel>()?;
    logging.add_class::<Logger>()?;

    m.add_submodule(logging)?;
    Ok(())
}

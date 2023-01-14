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
use std::{
    io::{self, BufWriter, Stderr, Stdout, Write},
    ops::{Deref, DerefMut},
};

use nautilus_core::datetime::unix_nanos_to_iso8601;
use nautilus_core::string::{cstr_to_string, string_to_cstr};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;

use crate::enums::{LogColor, LogLevel};

pub struct Logger {
    pub trader_id: TraderId,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub level_stdout: LogLevel,
    pub is_bypassed: bool,
    log_template: String,
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
            log_template: String::from(
                "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {msg}\x1b[0m\n",
            ),
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
        if level < self.level_stdout {
            return Ok(());
        }

        let fmt_line = self
            .log_template
            .replace("{ts}", &unix_nanos_to_iso8601(timestamp_ns))
            .replace("{color}", &color.to_string())
            .replace("{level}", &level.to_string())
            .replace("{trader_id}", &self.trader_id.to_string())
            .replace("{component}", component)
            .replace("{msg}", msg);

        if level >= LogLevel::Error {
            self.err.write_all(fmt_line.as_bytes())?;
            self.err.flush()
        } else {
            self.out.write_all(fmt_line.as_bytes())?;
            self.out.flush()
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

/// Creates a new logger.
///
/// # Safety
/// - Assumes `trader_id_ptr` is a valid C string pointer.
/// - Assumes `machine_id_ptr` is a valid C string pointer.
/// - Assumes `instance_id_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logger_new(
    trader_id_ptr: *const c_char,
    machine_id_ptr: *const c_char,
    instance_id_ptr: *const c_char,
    level_stdout: LogLevel,
    is_bypassed: u8,
) -> CLogger {
    CLogger(Box::new(Logger::new(
        TraderId::new(&cstr_to_string(trader_id_ptr)),
        String::from(&cstr_to_string(machine_id_ptr)),
        UUID4::from(cstr_to_string(instance_id_ptr).as_str()),
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

#[no_mangle]
pub extern "C" fn logger_get_trader_id_cstr(logger: &CLogger) -> *const c_char {
    string_to_cstr(&logger.trader_id.to_string())
}

#[no_mangle]
pub extern "C" fn logger_get_machine_id_cstr(logger: &CLogger) -> *const c_char {
    string_to_cstr(&logger.machine_id)
}

#[no_mangle]
pub extern "C" fn logger_get_instance_id(logger: &CLogger) -> UUID4 {
    logger.instance_id.clone()
}

#[no_mangle]
pub extern "C" fn logger_is_bypassed(logger: &CLogger) -> u8 {
    logger.is_bypassed as u8
}

/// Log a message.
///
/// # Safety
/// - Assumes `component_ptr` is a valid C string pointer.
/// - Assumes `msg_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn logger_log(
    logger: &mut CLogger,
    timestamp_ns: u64,
    level: LogLevel,
    color: LogColor,
    component_ptr: *const c_char,
    msg_ptr: *const c_char,
) {
    let component = cstr_to_string(component_ptr);
    let msg = cstr_to_string(msg_ptr);
    let _ = logger.log(timestamp_ns, level, color, &component, &msg);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::uuid::UUID4;
    use nautilus_model::identifiers::trader_id::TraderId;

    use crate::logging::{LogColor, LogLevel, Logger};

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

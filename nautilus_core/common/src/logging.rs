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
use std::io::{Stderr, Stdout};
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::time::{Duration, Instant};
use std::{
    io::{self, BufWriter, Write},
    ops::{Deref, DerefMut},
    thread,
};

use nautilus_core::datetime::unix_nanos_to_iso8601;
use nautilus_core::string::{cstr_to_string, string_to_cstr};
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;

use crate::enums::{LogColor, LogLevel};

pub struct Logger {
    /// The trader ID for the logger.
    pub trader_id: TraderId,
    /// The machine ID for the logger.
    pub machine_id: String,
    /// The instance ID for the logger.
    pub instance_id: UUID4,
    /// The maximum log level to write to stdout.
    pub level_stdout: LogLevel,
    /// The maximum messages per second which can be flushed to stdout or stderr.
    pub rate_limit: usize,
    /// If logging is bypassed.
    pub is_bypassed: bool,
    tx: Sender<LogMessage>,
}

#[derive(Clone, Debug)]
pub struct LogMessage {
    timestamp_ns: UnixNanos,
    level: LogLevel,
    color: LogColor,
    component: String,
    msg: String,
}

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives `LogMessage` structs over the
/// channel. Rate limiting is implemented using a simple token bucket algorithm (maximum messages
/// per second).
impl Logger {
    fn new(
        trader_id: TraderId,
        machine_id: String,
        instance_id: UUID4,
        level_stdout: LogLevel,
        rate_limit: usize,
        is_bypassed: bool,
    ) -> Self {
        let trader_id_clone = trader_id.value.to_string();
        let (tx, rx) = channel::<LogMessage>();

        thread::spawn(move || {
            Self::handle_messages(&trader_id_clone, level_stdout, rate_limit, rx)
        });

        Logger {
            trader_id,
            machine_id,
            instance_id,
            level_stdout,
            rate_limit,
            is_bypassed,
            tx,
        }
    }

    fn handle_messages(
        trader_id: &str,
        level_stdout: LogLevel,
        rate_limit: usize,
        rx: Receiver<LogMessage>,
    ) {
        let mut out = BufWriter::new(io::stdout());
        let mut err = BufWriter::new(io::stderr());

        let log_template = String::from(
            "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {msg}\x1b[0m\n",
        );

        let mut msg_count = 0;
        let mut bucket_time = Instant::now();

        // Continue to receive and handle log messages until channel is hung up
        while let Ok(log_msg) = rx.recv() {
            if log_msg.level < level_stdout {
                continue;
            }

            while msg_count >= rate_limit {
                if bucket_time.elapsed().as_secs() >= 1 {
                    msg_count = 0;
                    bucket_time = Instant::now();
                } else {
                    thread::sleep(Duration::from_millis(10));
                }
            }

            let fmt_line = log_template
                .replace("{ts}", &unix_nanos_to_iso8601(log_msg.timestamp_ns))
                .replace("{color}", &log_msg.color.to_string())
                .replace("{level}", &log_msg.level.to_string())
                .replace("{trader_id}", trader_id)
                .replace("{component}", &log_msg.component)
                .replace("{msg}", &log_msg.msg);

            if log_msg.level >= LogLevel::Error {
                Self::write_stderr(&mut err, fmt_line);
                Self::flush_stderr(&mut err);
            } else {
                Self::write_stdout(&mut out, fmt_line);
                Self::flush_stdout(&mut out);
            }

            msg_count += 1;
        }

        // Finally ensure remaining buffers are flushed
        Self::flush_stderr(&mut err);
        Self::flush_stdout(&mut out);
    }

    fn write_stdout(out: &mut BufWriter<Stdout>, line: String) {
        match out.write_all(line.as_bytes()) {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to stdout: {e:?}"),
        }
    }

    fn flush_stdout(out: &mut BufWriter<Stdout>) {
        match out.flush() {
            Ok(_) => {}
            Err(e) => eprintln!("Error flushing stdout: {e:?}"),
        }
    }

    fn write_stderr(err: &mut BufWriter<Stderr>, line: String) {
        match err.write_all(line.as_bytes()) {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to stderr: {e:?}"),
        }
    }

    fn flush_stderr(err: &mut BufWriter<Stderr>) {
        match err.flush() {
            Ok(_) => {}
            Err(e) => eprintln!("Error flushing stderr: {e:?}"),
        }
    }

    fn send(
        &mut self,
        timestamp_ns: u64,
        level: LogLevel,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        let log_message = LogMessage {
            timestamp_ns,
            level,
            color,
            component,
            msg,
        };
        self.tx.send(log_message)
    }

    pub fn debug(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        self.send(timestamp_ns, LogLevel::Debug, color, component, msg)
    }

    pub fn info(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        self.send(timestamp_ns, LogLevel::Info, color, component, msg)
    }

    pub fn warn(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        self.send(timestamp_ns, LogLevel::Warning, color, component, msg)
    }

    pub fn error(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        self.send(timestamp_ns, LogLevel::Error, color, component, msg)
    }

    pub fn critical(
        &mut self,
        timestamp_ns: u64,
        color: LogColor,
        component: String,
        msg: String,
    ) -> Result<(), SendError<LogMessage>> {
        self.send(timestamp_ns, LogLevel::Critical, color, component, msg)
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
    rate_limit: usize,
    is_bypassed: u8,
) -> CLogger {
    CLogger(Box::new(Logger::new(
        TraderId::new(&cstr_to_string(trader_id_ptr)),
        String::from(&cstr_to_string(machine_id_ptr)),
        UUID4::from(cstr_to_string(instance_id_ptr).as_str()),
        level_stdout,
        rate_limit,
        is_bypassed != 0,
    )))
}

#[no_mangle]
pub extern "C" fn logger_free(logger: CLogger) {
    drop(logger); // Memory freed here
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
    let _ = logger.send(timestamp_ns, level, color, component, msg);
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
            100_000,
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
            100_000,
            false,
        );

        logger
            .info(
                1650000000000000,
                LogColor::Normal,
                String::from("RiskEngine"),
                String::from("This is a test."),
            )
            .expect("Error while logging");
    }
}

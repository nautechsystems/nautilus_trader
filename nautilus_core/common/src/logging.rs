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

use std::collections::HashMap;
use std::ffi::c_char;
use std::fs::File;
use std::io::{Stderr, Stdout};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::{
    io::{self, BufWriter, Write},
    ops::{Deref, DerefMut},
    thread,
};

use governor::clock::{Clock, DefaultClock};
use governor::{Quota, RateLimiter};
use nautilus_core::datetime::unix_nanos_to_iso8601;
use nautilus_core::parsing::optional_bytes_to_json;
use nautilus_core::string::{cstr_to_string, optional_cstr_to_string, string_to_cstr};
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::enums::{LogColor, LogLevel};

pub struct Logger {
    tx: Sender<LogMessage>,
    /// The trader ID for the logger.
    pub trader_id: TraderId,
    /// The machine ID for the logger.
    pub machine_id: String,
    /// The instance ID for the logger.
    pub instance_id: UUID4,
    /// The minimum log level to write to stdout.
    pub level_stdout: LogLevel,
    /// The minimum log level to write to a log file.
    pub level_file: LogLevel,
    /// The maximum messages per second which can be flushed to stdout or stderr.
    pub rate_limit: usize,
    /// If logging is bypassed.
    pub is_bypassed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogMessage {
    timestamp_ns: UnixNanos,
    level: LogLevel,
    #[serde(skip_serializing)]
    color: LogColor,
    component: String,
    msg: String,
}

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives `LogMessage` structs over the
/// channel. Rate limiting is implemented using a simple token bucket algorithm (maximum messages
/// per second).
#[allow(clippy::too_many_arguments)]
impl Logger {
    pub fn new(
        trader_id: TraderId,
        machine_id: String,
        instance_id: UUID4,
        level_stdout: LogLevel,
        level_file: LogLevel,
        file_path: Option<PathBuf>,
        file_format: Option<String>,
        component_levels: Option<HashMap<String, Value>>,
        rate_limit: usize,
        is_bypassed: bool,
    ) -> Self {
        let trader_id_clone = trader_id.value.to_string();
        let (tx, rx) = channel::<LogMessage>();

        let mut level_filters = HashMap::<String, LogLevel>::new();

        if let Some(component_levels_map) = component_levels {
            for (key, value) in component_levels_map {
                match serde_json::from_value::<LogLevel>(value) {
                    Ok(level) => {
                        level_filters.insert(key, level);
                    }
                    Err(e) => {
                        // Handle the error, e.g. log a warning or ignore the entry
                        eprintln!("Error parsing log level: {:?}", e);
                    }
                }
            }
        }

        thread::spawn(move || {
            Self::handle_messages(
                &trader_id_clone,
                level_stdout,
                level_file,
                file_path,
                file_format,
                level_filters,
                rate_limit,
                rx,
            )
        });

        Logger {
            trader_id,
            machine_id,
            instance_id,
            level_stdout,
            level_file,
            rate_limit,
            is_bypassed,
            tx,
        }
    }

    fn handle_messages(
        trader_id: &str,
        level_stdout: LogLevel,
        level_file: LogLevel,
        file_path: Option<PathBuf>,
        file_format: Option<String>,
        level_filters: HashMap<String, LogLevel>,
        rate_limit: usize,
        rx: Receiver<LogMessage>,
    ) {
        // Setup std I/O buffers
        let mut out_buf = BufWriter::new(io::stdout());
        let mut err_buf = BufWriter::new(io::stderr());

        // Setup log file
        let file = file_path.map(|path| {
            File::options()
                .create(true)
                .append(true)
                .open(path)
                .expect("Error creating log file")
        });
        let mut file_buf = file.map(BufWriter::new);

        // Setup templates and formatting
        let template_console = String::from(
            "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {msg}\x1b[0m\n",
        );
        let template_file = String::from("{ts} [{level}] {trader_id}.{component}: {msg}\n");

        let is_json_format = match file_format.as_ref().map(|s| s.to_lowercase()) {
            Some(ref format) if format == "json" => true,
            None => false,
            Some(ref unrecognized) => {
                eprintln!(
                    "Error: Unrecognized log file format: {}. Using plain text format as default.",
                    unrecognized
                );
                false
            }
        };

        // Setup rate limiting
        let quota = Quota::per_second(NonZeroU32::new(rate_limit as u32).unwrap());
        let clock = DefaultClock::default();
        let limiter = RateLimiter::direct(quota);

        // Continue to receive and handle log messages until channel is hung up
        while let Ok(log_msg) = rx.recv() {
            let component_level = level_filters.get(&log_msg.component);

            // Check if the component exists in level_filters and if its level is greater than log_msg.level
            if let Some(&filter_level) = component_level {
                if log_msg.level < filter_level {
                    continue;
                }
            }

            if log_msg.level >= LogLevel::Error {
                // Check rate limiter
                loop {
                    match limiter.check() {
                        Ok(()) => break,
                        Err(minimum_time) => {
                            let wait_time = minimum_time.wait_time_from(clock.now());
                            thread::sleep(wait_time);
                        }
                    }
                }

                let line = Self::format_log_line_console(&log_msg, trader_id, &template_console);
                Self::write_stderr(&mut err_buf, &line);
                Self::flush_stderr(&mut err_buf);
            } else if log_msg.level >= level_stdout {
                // Check rate limiter
                loop {
                    match limiter.check() {
                        Ok(()) => break,
                        Err(minimum_time) => {
                            let wait_time = minimum_time.wait_time_from(clock.now());
                            thread::sleep(wait_time);
                        }
                    }
                }

                let line = Self::format_log_line_console(&log_msg, trader_id, &template_console);
                Self::write_stdout(&mut out_buf, &line);
                Self::flush_stdout(&mut out_buf);
            }

            if log_msg.level >= level_file {
                let line =
                    Self::format_log_line_file(&log_msg, trader_id, &template_file, is_json_format);
                Self::write_file(&mut file_buf, &line);
                Self::flush_file(&mut file_buf);
            }
        }
        // Finally ensure remaining buffers are flushed
        Self::flush_stderr(&mut err_buf);
        Self::flush_stdout(&mut out_buf);
        Self::flush_file(&mut file_buf);
    }

    fn format_log_line_console(log_msg: &LogMessage, trader_id: &str, template: &str) -> String {
        template
            .replace("{ts}", &unix_nanos_to_iso8601(log_msg.timestamp_ns))
            .replace("{color}", &log_msg.color.to_string())
            .replace("{level}", &log_msg.level.to_string())
            .replace("{trader_id}", trader_id)
            .replace("{component}", &log_msg.component)
            .replace("{msg}", &log_msg.msg)
    }

    fn format_log_line_file(
        log_msg: &LogMessage,
        trader_id: &str,
        template: &str,
        is_json_format: bool,
    ) -> String {
        if is_json_format {
            let json_string =
                serde_json::to_string(log_msg).expect("Error serializing log message to string");
            format!("{}\n", json_string)
        } else {
            template
                .replace("{ts}", &unix_nanos_to_iso8601(log_msg.timestamp_ns))
                .replace("{level}", &log_msg.level.to_string())
                .replace("{trader_id}", trader_id)
                .replace("{component}", &log_msg.component)
                .replace("{msg}", &log_msg.msg)
        }
    }

    fn write_stdout(out_buf: &mut BufWriter<Stdout>, line: &str) {
        match out_buf.write_all(line.as_bytes()) {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to stdout: {e:?}"),
        }
    }

    fn flush_stdout(out_buf: &mut BufWriter<Stdout>) {
        match out_buf.flush() {
            Ok(_) => {}
            Err(e) => eprintln!("Error flushing stdout: {e:?}"),
        }
    }

    fn write_stderr(err_buf: &mut BufWriter<Stderr>, line: &str) {
        match err_buf.write_all(line.as_bytes()) {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to stderr: {e:?}"),
        }
    }

    fn flush_stderr(err_buf: &mut BufWriter<Stderr>) {
        match err_buf.flush() {
            Ok(_) => {}
            Err(e) => eprintln!("Error flushing stderr: {e:?}"),
        }
    }

    fn write_file(file_buf: &mut Option<BufWriter<File>>, line: &str) {
        match file_buf {
            Some(file) => match file.write_all(line.as_bytes()) {
                Ok(_) => {}
                Err(e) => eprintln!("Error writing to file: {e:?}"),
            },
            None => {}
        }
    }

    fn flush_file(file_buf: &mut Option<BufWriter<File>>) {
        match file_buf {
            Some(file) => match file.flush() {
                Ok(_) => {}
                Err(e) => eprintln!("Error writing to file: {e:?}"),
            },
            None => {}
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
    level_file: LogLevel,
    file_path_ptr: *const c_char,
    file_format_ptr: *const c_char,
    component_levels_ptr: *const c_char,
    rate_limit: usize,
    is_bypassed: u8,
) -> CLogger {
    CLogger(Box::new(Logger::new(
        TraderId::new(&cstr_to_string(trader_id_ptr)),
        String::from(&cstr_to_string(machine_id_ptr)),
        UUID4::from(cstr_to_string(instance_id_ptr).as_str()),
        level_stdout,
        level_file,
        optional_cstr_to_string(file_path_ptr).map(PathBuf::from),
        optional_cstr_to_string(file_format_ptr),
        optional_bytes_to_json(component_levels_ptr),
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
    use crate::testing::wait_until;

    use super::*;
    use nautilus_core::uuid::UUID4;
    use nautilus_model::identifiers::trader_id::TraderId;
    use std::{fs, time::Duration};
    use tempfile::NamedTempFile;

    #[test]
    fn log_message_serialization() {
        let log_message = LogMessage {
            timestamp_ns: 1_000_000_000,
            level: LogLevel::Info,
            color: LogColor::Normal,
            component: "Portfolio".to_string(),
            msg: "This is a log message".to_string(),
        };

        let serialized_json = serde_json::to_string(&log_message).unwrap();
        let deserialized_value: Value = serde_json::from_str(&serialized_json).unwrap();

        assert_eq!(deserialized_value["timestamp_ns"], 1_000_000_000);
        assert_eq!(deserialized_value["level"], "INFO");
        assert_eq!(deserialized_value["component"], "Portfolio");
        assert_eq!(deserialized_value["msg"], "This is a log message");
    }

    #[test]
    fn test_new_logger() {
        let logger = Logger::new(
            TraderId::new("TRADER-000"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Debug,
            LogLevel::Debug,
            None,
            None,
            None,
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
            LogLevel::Debug,
            None,
            None,
            None,
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

    #[test]
    fn test_logging_to_file() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            LogLevel::Debug,
            Some(log_file_path.to_path_buf()),
            None,
            None,
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

        let mut log_contents = String::new();

        wait_until(
            || {
                log_contents =
                    fs::read_to_string(log_file_path).expect("Error while reading log file");
                !log_contents.is_empty()
            },
            Duration::from_secs(3),
        );

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INF] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[test]
    fn test_log_component_level_filtering() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        let component_levels = HashMap::from_iter(std::iter::once((
            String::from("RiskEngine"),
            Value::from("ERROR"), // <-- This should be filtered
        )));

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            LogLevel::Debug,
            Some(log_file_path.to_path_buf()),
            None,
            Some(component_levels),
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

        thread::sleep(Duration::from_secs(1));

        assert!(fs::read_to_string(log_file_path)
            .expect("Error while reading log file")
            .is_empty());
    }

    #[test]
    fn test_logging_to_file_in_json_format() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            LogLevel::Debug,
            Some(log_file_path.to_path_buf()),
            Some("JSON".to_string()),
            None,
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

        let mut log_contents = String::new();

        wait_until(
            || {
                log_contents =
                    fs::read_to_string(log_file_path).expect("Error while reading log file");
                !log_contents.is_empty()
            },
            Duration::from_secs(3),
        );

        assert_eq!(
            log_contents,
            "{\"timestamp_ns\":1650000000000000,\"level\":\"INFO\",\"component\":\"RiskEngine\",\"msg\":\"This is a test.\"}\n"
        );
    }
}

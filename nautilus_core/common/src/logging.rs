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
use std::fmt;
use std::fs::{create_dir_all, File};
use std::io::{Stderr, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::{
    io::{self, BufWriter, Write},
    ops::{Deref, DerefMut},
    thread,
};

use chrono::prelude::*;
use chrono::Utc;
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
    pub level_file: Option<LogLevel>,
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

impl fmt::Display for LogMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] {}: {}",
            self.timestamp_ns, self.level, self.component, self.msg
        )
    }
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
        level_file: Option<LogLevel>,
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
        component_levels: Option<HashMap<String, Value>>,
        is_bypassed: bool,
    ) -> Self {
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

        let trader_id_clone = trader_id.value.to_string();
        let instance_id_clone = instance_id.value.to_string();

        thread::spawn(move || {
            Self::handle_messages(
                &trader_id_clone,
                &instance_id_clone,
                level_stdout,
                level_file,
                directory,
                file_name,
                file_format,
                level_filters,
                rx,
            )
        });

        Logger {
            trader_id,
            machine_id,
            instance_id,
            level_stdout,
            level_file,
            is_bypassed,
            tx,
        }
    }

    fn handle_messages(
        trader_id: &str,
        instance_id: &str,
        level_stdout: LogLevel,
        level_file: Option<LogLevel>,
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
        level_filters: HashMap<String, LogLevel>,
        rx: Receiver<LogMessage>,
    ) {
        // Setup std I/O buffers
        let mut out_buf = BufWriter::new(io::stdout());
        let mut err_buf = BufWriter::new(io::stderr());

        // Setup log file
        let is_json_format = match file_format.as_ref().map(|s| s.to_lowercase()) {
            Some(ref format) if format == "json" => true,
            None => false,
            Some(ref unrecognized) => {
                eprintln!(
                    "Unrecognized log file format: {}. Using plain text format as default.",
                    unrecognized
                );
                false
            }
        };

        let file_path = PathBuf::new();
        let file = if level_file.is_some() {
            let file_path = Self::create_log_file_path(
                &directory,
                &file_name,
                trader_id,
                instance_id,
                is_json_format,
            );

            Some(
                File::options()
                    .create(true)
                    .append(true)
                    .open(file_path)
                    .expect("Error creating log file"),
            )
        } else {
            None
        };

        let mut file_buf = file.map(BufWriter::new);

        // Setup templates for formatting
        let template_console = String::from(
            "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {msg}\x1b[0m\n",
        );
        let template_file = String::from("{ts} [{level}] {trader_id}.{component}: {msg}\n");

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
                let line = Self::format_log_line_console(&log_msg, trader_id, &template_console);
                Self::write_stderr(&mut err_buf, &line);
                Self::flush_stderr(&mut err_buf);
            } else if log_msg.level >= level_stdout {
                let line = Self::format_log_line_console(&log_msg, trader_id, &template_console);
                Self::write_stdout(&mut out_buf, &line);
                Self::flush_stdout(&mut out_buf);
            }

            if let Some(level_file) = level_file {
                if Self::should_rotate_file(&file_path) {
                    // Ensure previous file buffer flushed
                    if let Some(file_buf) = file_buf.as_mut() {
                        Self::flush_file(file_buf);
                    };

                    let file_path = Self::create_log_file_path(
                        &directory,
                        &file_name,
                        trader_id,
                        instance_id,
                        is_json_format,
                    );

                    let file = File::options()
                        .create(true)
                        .append(true)
                        .open(file_path)
                        .expect("Error creating log file");

                    file_buf = Some(BufWriter::new(file));
                }

                if log_msg.level >= level_file {
                    if let Some(file_buf) = file_buf.as_mut() {
                        let line = Self::format_log_line_file(
                            &log_msg,
                            trader_id,
                            &template_file,
                            is_json_format,
                        );
                        Self::write_file(file_buf, &line);
                        Self::flush_file(file_buf);
                    }
                }
            }
        }

        // Finally ensure remaining buffers are flushed
        Self::flush_stderr(&mut err_buf);
        Self::flush_stdout(&mut out_buf);
    }

    fn should_rotate_file(file_path: &Path) -> bool {
        if file_path.exists() {
            let current_date_utc = Utc::now().date_naive();
            let metadata = file_path
                .metadata()
                .expect("Failed to read log file metadata");
            let creation_time = metadata
                .created()
                .expect("Failed to get log file creation time");

            let creation_time_utc: DateTime<Utc> = creation_time.into();
            let creation_date_utc = creation_time_utc.date_naive();

            current_date_utc != creation_date_utc
        } else {
            false
        }
    }

    fn default_log_file_basename(trader_id: &str, instance_id: &str) -> String {
        let current_date_utc = Utc::now().format("%Y-%m-%d");
        format!("{}_{}_{}", trader_id, current_date_utc, instance_id)
    }

    fn create_log_file_path(
        directory: &Option<String>,
        file_name: &Option<String>,
        trader_id: &str,
        instance_id: &str,
        is_json_format: bool,
    ) -> PathBuf {
        let basename = if let Some(file_name) = file_name {
            file_name.to_owned()
        } else {
            Self::default_log_file_basename(trader_id, instance_id)
        };

        let suffix = if is_json_format { "json" } else { "log" };
        let mut file_path = PathBuf::new();

        if let Some(directory) = directory {
            file_path.push(directory);
            create_dir_all(&file_path).expect("Failed to create directories for log file");
        }

        file_path.push(basename);
        file_path.set_extension(suffix);
        file_path
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

    fn write_file(file_buf: &mut BufWriter<File>, line: &str) {
        match file_buf.write_all(line.as_bytes()) {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to file: {e:?}"),
        }
    }

    fn flush_file(file_buf: &mut BufWriter<File>) {
        match file_buf.flush() {
            Ok(_) => {}
            Err(e) => eprintln!("Error writing to file: {e:?}"),
        }
    }

    fn send(
        &mut self,
        timestamp_ns: u64,
        level: LogLevel,
        color: LogColor,
        component: String,
        msg: String,
    ) {
        let log_message = LogMessage {
            timestamp_ns,
            level,
            color,
            component,
            msg,
        };
        if let Err(SendError(msg)) = self.tx.send(log_message) {
            eprintln!("Error sending log message: {}", msg);
        }
    }

    pub fn debug(&mut self, timestamp_ns: u64, color: LogColor, component: String, msg: String) {
        self.send(timestamp_ns, LogLevel::Debug, color, component, msg)
    }

    pub fn info(&mut self, timestamp_ns: u64, color: LogColor, component: String, msg: String) {
        self.send(timestamp_ns, LogLevel::Info, color, component, msg)
    }

    pub fn warn(&mut self, timestamp_ns: u64, color: LogColor, component: String, msg: String) {
        self.send(timestamp_ns, LogLevel::Warning, color, component, msg)
    }

    pub fn error(&mut self, timestamp_ns: u64, color: LogColor, component: String, msg: String) {
        self.send(timestamp_ns, LogLevel::Error, color, component, msg)
    }

    pub fn critical(&mut self, timestamp_ns: u64, color: LogColor, component: String, msg: String) {
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
    file_logging: u8,
    directory_ptr: *const c_char,
    file_name_ptr: *const c_char,
    file_format_ptr: *const c_char,
    component_levels_ptr: *const c_char,
    is_bypassed: u8,
) -> CLogger {
    CLogger(Box::new(Logger::new(
        TraderId::new(&cstr_to_string(trader_id_ptr)),
        String::from(&cstr_to_string(machine_id_ptr)),
        UUID4::from(cstr_to_string(instance_id_ptr).as_str()),
        level_stdout,
        if file_logging != 0 {
            Some(level_file)
        } else {
            None
        },
        optional_cstr_to_string(directory_ptr),
        optional_cstr_to_string(file_name_ptr),
        optional_cstr_to_string(file_format_ptr),
        optional_bytes_to_json(component_levels_ptr),
        is_bypassed != 0,
    )))
}

#[no_mangle]
pub extern "C" fn logger_drop(logger: CLogger) {
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
    logger.send(timestamp_ns, level, color, component, msg);
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
    use std::{cell::RefCell, fs, path::PathBuf, time::Duration};
    use tempfile::NamedTempFile;

    fn create_logger() -> Logger {
        Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            None,
            None,
            None,
            None,
            None,
            false,
        )
    }

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
        let logger = create_logger();

        assert_eq!(logger.trader_id, TraderId::new("TRADER-001"));
        assert_eq!(logger.level_stdout, LogLevel::Info);
        assert_eq!(logger.level_file, None);
        assert!(!logger.is_bypassed);
    }

    #[test]
    fn test_logger_debug() {
        let mut logger = create_logger();

        logger.debug(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test debug message."),
        );
    }

    #[test]
    fn test_logger_info() {
        let mut logger = create_logger();

        logger.info(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test info message."),
        );
    }

    #[test]
    fn test_logger_error() {
        let mut logger = create_logger();

        logger.error(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test error message."),
        );
    }

    #[test]
    fn test_logger_critical() {
        let mut logger = create_logger();

        logger.critical(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test critical message."),
        );
    }

    #[ignore]
    #[test]
    fn test_logging_to_file() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        // Add the ".log" suffix to the log file path
        let mut log_file_path_with_suffix = PathBuf::from(log_file_path);
        log_file_path_with_suffix.set_extension("log");
        let log_file_path_with_suffix_str =
            RefCell::new(log_file_path_with_suffix.to_str().unwrap().to_string());

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            Some(log_file_path.to_str().unwrap().to_string()),
            None,
            None,
            None,
            false,
        );

        logger.info(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        let mut log_contents = String::new();

        wait_until(
            || {
                log_contents = fs::read_to_string(log_file_path_with_suffix_str.borrow().clone())
                    .expect("Error while reading log file");
                !log_contents.is_empty()
            },
            Duration::from_secs(3),
        );

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INF] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[ignore]
    #[test]
    fn test_log_component_level_filtering() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        // Add the ".log" suffix to the log file path
        let mut log_file_path_with_suffix = PathBuf::from(log_file_path);
        log_file_path_with_suffix.set_extension("log");
        let log_file_path_with_suffix_str =
            RefCell::new(log_file_path_with_suffix.to_str().unwrap().to_string());

        let component_levels = HashMap::from_iter(std::iter::once((
            String::from("RiskEngine"),
            Value::from("ERROR"), // <-- This should be filtered
        )));

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            Some(log_file_path.to_str().unwrap().to_string()),
            None,
            None,
            Some(component_levels),
            false,
        );

        logger.info(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        thread::sleep(Duration::from_secs(1));

        assert!(
            fs::read_to_string(log_file_path_with_suffix_str.borrow().clone())
                .expect("Error while reading log file")
                .is_empty()
        );
    }

    #[ignore]
    #[test]
    fn test_logging_to_file_in_json_format() {
        let temp_log_file = NamedTempFile::new().expect("Failed to create temporary log file");
        let log_file_path = temp_log_file.path();

        // Add the ".log" suffix to the log file path
        let mut log_file_path_with_suffix = PathBuf::from(log_file_path);
        log_file_path_with_suffix.set_extension("json");
        let log_file_path_with_suffix_str =
            RefCell::new(log_file_path_with_suffix.to_str().unwrap().to_string());

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            None,
            Some(log_file_path.to_str().unwrap().to_string()),
            Some("json".to_string()),
            None,
            false,
        );

        logger.info(
            1650000000000000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        let mut log_contents = String::new();

        wait_until(
            || {
                log_contents = fs::read_to_string(log_file_path_with_suffix_str.borrow().clone())
                    .expect("Error while reading log file");
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

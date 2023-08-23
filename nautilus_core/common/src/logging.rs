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

use std::{
    collections::HashMap,
    fmt,
    fs::{create_dir_all, File},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, SendError, Sender},
    thread,
};

use chrono::{prelude::*, Utc};
use nautilus_core::{datetime::unix_nanos_to_iso8601, time::UnixNanos, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::enums::{LogColor, LogLevel};

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives [`LogEvent`] structs over the
/// channel.
pub struct Logger {
    tx: Sender<LogEvent>,
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

/// Represents a log event which includes a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEvent {
    /// The UNIX nanoseconds timestamp when the log event occurred.
    timestamp: UnixNanos,
    /// The log level for the event.
    level: LogLevel,
    /// The color for the log message content.
    color: LogColor,
    /// The Nautilus system component the log event originated from.
    component: String,
    /// The log message content.
    message: String,
}

impl fmt::Display for LogEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] {}: {}",
            self.timestamp, self.level, self.component, self.message
        )
    }
}

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
        let (tx, rx) = channel::<LogEvent>();
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
        let instance_id_clone = instance_id.to_string();

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
        rx: Receiver<LogEvent>,
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
            "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {message}\x1b[0m\n",
        );
        let template_file = String::from("{ts} [{level}] {trader_id}.{component}: {message}\n");

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            let component_level = level_filters.get(&event.component);

            // Check if the component exists in level_filters and if its level is greater than event.level
            if let Some(&filter_level) = component_level {
                if event.level < filter_level {
                    continue;
                }
            }

            if event.level >= LogLevel::Error {
                let line = Self::format_log_line_console(&event, trader_id, &template_console);
                Self::write_stderr(&mut err_buf, &line);
                Self::flush_stderr(&mut err_buf);
            } else if event.level >= level_stdout {
                let line = Self::format_log_line_console(&event, trader_id, &template_console);
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

                if event.level >= level_file {
                    if let Some(file_buf) = file_buf.as_mut() {
                        let line = Self::format_log_line_file(
                            &event,
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

    fn format_log_line_console(event: &LogEvent, trader_id: &str, template: &str) -> String {
        template
            .replace("{ts}", &unix_nanos_to_iso8601(event.timestamp))
            .replace("{color}", &event.color.to_string())
            .replace("{level}", &event.level.to_string())
            .replace("{trader_id}", trader_id)
            .replace("{component}", &event.component)
            .replace("{message}", &event.message)
    }

    fn format_log_line_file(
        event: &LogEvent,
        trader_id: &str,
        template: &str,
        is_json_format: bool,
    ) -> String {
        if is_json_format {
            let json_string =
                serde_json::to_string(event).expect("Error serializing log event to string");
            format!("{}\n", json_string)
        } else {
            template
                .replace("{ts}", &unix_nanos_to_iso8601(event.timestamp))
                .replace("{level}", &event.level.to_string())
                .replace("{trader_id}", trader_id)
                .replace("{component}", &event.component)
                .replace("{message}", &event.message)
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

    pub fn send(
        &mut self,
        timestamp: u64,
        level: LogLevel,
        color: LogColor,
        component: String,
        message: String,
    ) {
        let event = LogEvent {
            timestamp,
            level,
            color,
            component,
            message,
        };
        if let Err(SendError(e)) = self.tx.send(event) {
            eprintln!("Error sending log event: {}", e);
        }
    }

    pub fn debug(&mut self, timestamp: u64, color: LogColor, component: String, message: String) {
        self.send(timestamp, LogLevel::Debug, color, component, message)
    }

    pub fn info(&mut self, timestamp: u64, color: LogColor, component: String, message: String) {
        self.send(timestamp, LogLevel::Info, color, component, message)
    }

    pub fn warn(&mut self, timestamp: u64, color: LogColor, component: String, message: String) {
        self.send(timestamp, LogLevel::Warning, color, component, message)
    }

    pub fn error(&mut self, timestamp: u64, color: LogColor, component: String, message: String) {
        self.send(timestamp, LogLevel::Error, color, component, message)
    }

    pub fn critical(
        &mut self,
        timestamp: u64,
        color: LogColor,
        component: String,
        message: String,
    ) {
        self.send(timestamp, LogLevel::Critical, color, component, message)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nautilus_core::uuid::UUID4;
    use nautilus_model::identifiers::trader_id::TraderId;
    use tempfile::tempdir;

    use super::*;
    use crate::testing::wait_until;

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
        let log_message = LogEvent {
            timestamp: 1_000_000_000,
            level: LogLevel::Info,
            color: LogColor::Normal,
            component: "Portfolio".to_string(),
            message: "This is a log message".to_string(),
        };

        let serialized_json = serde_json::to_string(&log_message).unwrap();
        let deserialized_value: Value = serde_json::from_str(&serialized_json).unwrap();

        assert_eq!(deserialized_value["timestamp"], 1_000_000_000);
        assert_eq!(deserialized_value["level"], "INFO");
        assert_eq!(deserialized_value["component"], "Portfolio");
        assert_eq!(deserialized_value["message"], "This is a log message");
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
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test debug message."),
        );
    }

    #[test]
    fn test_logger_info() {
        let mut logger = create_logger();

        logger.info(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test info message."),
        );
    }

    #[test]
    fn test_logger_error() {
        let mut logger = create_logger();

        logger.error(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test error message."),
        );
    }

    #[test]
    fn test_logger_critical() {
        let mut logger = create_logger();

        logger.critical(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test critical message."),
        );
    }

    #[test]
    fn test_logging_to_file() {
        let temp_dir = tempdir().expect("Failed to create temporary directory");

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            Some(temp_dir.path().to_str().unwrap().to_string()),
            None,
            None,
            None,
            false,
        );

        logger.info(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        let mut log_contents = String::new();

        wait_until(
            || {
                let log_file_exists = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .next()
                    .is_some();

                log_file_exists
            },
            Duration::from_secs(2),
        );

        wait_until(
            || {
                let log_file_path = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .next()
                    .expect("No files found in directory")
                    .path();
                log_contents =
                    std::fs::read_to_string(&log_file_path).expect("Error while reading log file");
                !log_contents.is_empty()
            },
            Duration::from_secs(2),
        );

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INF] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[test]
    fn test_log_component_level_filtering() {
        let temp_dir = tempdir().expect("Failed to create temporary directory");

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            Some(temp_dir.path().to_str().unwrap().to_string()),
            None,
            None,
            Some(HashMap::from_iter(std::iter::once((
                String::from("RiskEngine"),
                Value::from("ERROR"), // <-- This should be filtered
            )))),
            false,
        );

        logger.info(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        wait_until(
            || {
                if let Some(log_file) = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .next()
                {
                    let log_file_path = log_file.path();
                    let log_contents = std::fs::read_to_string(&log_file_path)
                        .expect("Error while reading log file");
                    !log_contents.contains("RiskEngine")
                } else {
                    false
                }
            },
            Duration::from_secs(3),
        );

        assert!(
            std::fs::read_dir(&temp_dir)
                .expect("Failed to read directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file())
                .next()
                .is_some(),
            "Log file exists"
        );
    }

    #[test]
    fn test_logging_to_file_in_json_format() {
        let temp_dir = tempdir().expect("Failed to create temporary directory");

        let mut logger = Logger::new(
            TraderId::new("TRADER-001"),
            String::from("user-01"),
            UUID4::new(),
            LogLevel::Info,
            Some(LogLevel::Debug),
            Some(temp_dir.path().to_str().unwrap().to_string()),
            None,
            Some("json".to_string()),
            None,
            false,
        );

        logger.info(
            1_650_000_000_000_000,
            LogColor::Normal,
            String::from("RiskEngine"),
            String::from("This is a test."),
        );

        let mut log_contents = String::new();

        wait_until(
            || {
                if let Some(log_file) = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .next()
                {
                    let log_file_path = log_file.path();
                    log_contents = std::fs::read_to_string(&log_file_path)
                        .expect("Error while reading log file");
                    !log_contents.is_empty()
                } else {
                    false
                }
            },
            Duration::from_secs(2),
        );

        assert_eq!(
        log_contents,
        "{\"timestamp\":1650000000000000,\"level\":\"INFO\",\"color\":\"Normal\",\"component\":\"RiskEngine\",\"message\":\"This is a test.\"}\n"
    );
    }
}

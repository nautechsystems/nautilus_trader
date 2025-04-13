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

use std::{
    collections::HashMap,
    env,
    fmt::Display,
    str::FromStr,
    sync::{atomic::Ordering, mpsc::SendError},
};

use indexmap::IndexMap;
use log::{
    Level, LevelFilter, Log, STATIC_MAX_LEVEL,
    kv::{ToValue, Value},
    set_boxed_logger, set_max_level,
};
use nautilus_core::{
    UUID4, UnixNanos,
    datetime::unix_nanos_to_iso8601,
    time::{get_atomic_clock_realtime, get_atomic_clock_static},
};
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize, Serializer};
use ustr::Ustr;

use super::{LOGGING_BYPASSED, LOGGING_REALTIME};
use crate::{
    enums::{LogColor, LogLevel},
    logging::writer::{FileWriter, FileWriterConfig, LogWriter, StderrWriter, StdoutWriter},
};

const LOGGING: &str = "logging";

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerConfig {
    /// Maximum log level to write to stdout.
    pub stdout_level: LevelFilter,
    /// Maximum log level to write to file (disabled is `Off`).
    pub fileout_level: LevelFilter,
    /// Per-component log levels, allowing finer-grained control.
    component_level: HashMap<Ustr, LevelFilter>,
    /// If logger is using ANSI color codes.
    pub is_colored: bool,
    /// If the configuration should be printed to stdout at initialization.
    pub print_config: bool,
}

impl Default for LoggerConfig {
    /// Creates a new default [`LoggerConfig`] instance.
    fn default() -> Self {
        Self {
            stdout_level: LevelFilter::Info,
            fileout_level: LevelFilter::Off,
            component_level: HashMap::new(),
            is_colored: false,
            print_config: false,
        }
    }
}

impl LoggerConfig {
    /// Creates a new [`LoggerConfig`] instance.
    #[must_use]
    pub const fn new(
        stdout_level: LevelFilter,
        fileout_level: LevelFilter,
        component_level: HashMap<Ustr, LevelFilter>,
        is_colored: bool,
        print_config: bool,
    ) -> Self {
        Self {
            stdout_level,
            fileout_level,
            component_level,
            is_colored,
            print_config,
        }
    }

    pub fn from_spec(spec: &str) -> anyhow::Result<Self> {
        let mut config = Self::default();
        for kv in spec.split(';') {
            let kv = kv.trim();
            if kv.is_empty() {
                continue;
            }
            let kv_lower = kv.to_lowercase(); // For case-insensitive comparison
            if kv_lower == "is_colored" {
                config.is_colored = true;
            } else if kv_lower == "print_config" {
                config.print_config = true;
            } else {
                let parts: Vec<&str> = kv.split('=').collect();
                if parts.len() != 2 {
                    anyhow::bail!("Invalid spec pair: {}", kv);
                }
                let k = parts[0].trim(); // Trim key
                let v = parts[1].trim(); // Trim value
                let lvl = LevelFilter::from_str(v)
                    .map_err(|_| anyhow::anyhow!("Invalid log level: {}", v))?;
                let k_lower = k.to_lowercase(); // Case-insensitive key matching
                match k_lower.as_str() {
                    "stdout" => config.stdout_level = lvl,
                    "fileout" => config.fileout_level = lvl,
                    _ => {
                        config.component_level.insert(Ustr::from(k), lvl);
                    }
                }
            }
        }
        Ok(config)
    }

    /// Retrieves the logger configuration from the "`NAUTILUS_LOG`" environment variable.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable is unset or invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        let spec = env::var("NAUTILUS_LOG")?;
        Self::from_spec(&spec)
    }
}

/// A high-performance logger utilizing a MPSC channel under the hood.
///
/// A logger is initialized with a [`LoggerConfig`] to set up different logging levels for
/// stdout, file, and components. The logger spawns a thread that listens for [`LogEvent`]s
/// sent via an MPSC channel.
#[derive(Debug)]
pub struct Logger {
    /// Configuration for logging levels and behavior.
    pub config: LoggerConfig,
    /// Transmitter for sending log events to the 'logging' thread.
    tx: std::sync::mpsc::Sender<LogEvent>,
}

/// Represents a type of log event.
pub enum LogEvent {
    /// A log line event.
    Log(LogLine),
    /// A command to flush all logger buffers.
    Flush,
}

/// Represents a log event which includes a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogLine {
    /// The timestamp for the event.
    pub timestamp: UnixNanos,
    /// The log level for the event.
    pub level: Level,
    /// The color for the log message content.
    pub color: LogColor,
    /// The Nautilus system component the log event originated from.
    pub component: Ustr,
    /// The log message content.
    pub message: String,
}

impl Display for LogLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.level, self.component, self.message)
    }
}

/// A wrapper around a log line that provides formatted and cached representations.
///
/// This struct contains a log line and provides various formatted versions
/// of it, such as plain string, colored string, and JSON. It also caches the
/// results for repeated calls, optimizing performance when the same message
/// needs to be logged multiple times in different formats.
pub struct LogLineWrapper {
    /// The underlying log line that contains the log data.
    line: LogLine,
    /// Cached plain string representation of the log line.
    cache: Option<String>,
    /// Cached colored string representation of the log line.
    colored: Option<String>,
    /// The ID of the trader associated with this log event.
    trader_id: Ustr,
}

impl LogLineWrapper {
    /// Creates a new [`LogLineWrapper`] instance.
    #[must_use]
    pub const fn new(line: LogLine, trader_id: Ustr) -> Self {
        Self {
            line,
            cache: None,
            colored: None,
            trader_id,
        }
    }

    /// Returns the plain log message string, caching the result.
    ///
    /// This method constructs the log line format and caches it for repeated calls. Useful when the
    /// same log message needs to be printed multiple times.
    pub fn get_string(&mut self) -> &str {
        self.cache.get_or_insert_with(|| {
            format!(
                "{} [{}] {}.{}: {}\n",
                unix_nanos_to_iso8601(self.line.timestamp),
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message,
            )
        })
    }

    /// Returns the colored log message string, caching the result.
    ///
    /// This method constructs the colored log line format and caches the result
    /// for repeated calls, providing the message with ANSI color codes if the
    /// logger is configured to use colors.
    pub fn get_colored(&mut self) -> &str {
        self.colored.get_or_insert_with(|| {
            format!(
                "\x1b[1m{}\x1b[0m {}[{}] {}.{}: {}\x1b[0m\n",
                unix_nanos_to_iso8601(self.line.timestamp),
                &self.line.color.as_ansi(),
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message,
            )
        })
    }

    /// Returns the log message as a JSON string.
    ///
    /// This method serializes the log line and its associated metadata
    /// (timestamp, trader ID, etc.) into a JSON string format. This is useful
    /// for structured logging or when logs need to be stored in a JSON format.
    #[must_use]
    pub fn get_json(&self) -> String {
        let json_string =
            serde_json::to_string(&self).expect("Error serializing log event to string");
        format!("{json_string}\n")
    }
}

impl Serialize for LogLineWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut json_obj = IndexMap::new();
        let timestamp = unix_nanos_to_iso8601(self.line.timestamp);
        json_obj.insert("timestamp".to_string(), timestamp);
        json_obj.insert("trader_id".to_string(), self.trader_id.to_string());
        json_obj.insert("level".to_string(), self.line.level.to_string());
        json_obj.insert("color".to_string(), self.line.color.to_string());
        json_obj.insert("component".to_string(), self.line.component.to_string());
        json_obj.insert("message".to_string(), self.line.message.to_string());

        json_obj.serialize(serializer)
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !LOGGING_BYPASSED.load(Ordering::Relaxed)
            && (metadata.level() == Level::Error
                || metadata.level() <= self.config.stdout_level
                || metadata.level() <= self.config.fileout_level)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let timestamp = if LOGGING_REALTIME.load(Ordering::Relaxed) {
                get_atomic_clock_realtime().get_time_ns()
            } else {
                get_atomic_clock_static().get_time_ns()
            };
            let level = record.level();
            let key_values = record.key_values();
            let color: LogColor = key_values
                .get("color".into())
                .and_then(|v| v.to_u64().map(|v| (v as u8).into()))
                .unwrap_or(level.into());
            let component = key_values.get("component".into()).map_or_else(
                || Ustr::from(record.metadata().target()),
                |v| Ustr::from(&v.to_string()),
            );

            let line = LogLine {
                timestamp,
                level,
                color,
                component,
                message: format!("{}", record.args()),
            };
            if let Err(SendError(LogEvent::Log(line))) = self.tx.send(LogEvent::Log(line)) {
                eprintln!("Error sending log event (receiver closed): {line}");
            }
        }
    }

    fn flush(&self) {
        if let Err(e) = self.tx.send(LogEvent::Flush) {
            eprintln!("Error sending flush log event (receiver closed): {e}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
impl Logger {
    pub fn init_with_env(
        trader_id: TraderId,
        instance_id: UUID4,
        file_config: FileWriterConfig,
    ) -> anyhow::Result<LogGuard> {
        let config = LoggerConfig::from_env()?;
        Self::init_with_config(trader_id, instance_id, config, file_config)
    }

    /// Initializes the logger with the given configuration.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error");
    /// let file_config = FileWriterConfig::default();
    /// let log_guard = Logger::init_with_config(trader_id, instance_id, config, file_config);
    /// ```
    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
        file_config: FileWriterConfig,
    ) -> anyhow::Result<LogGuard> {
        let (tx, rx) = std::sync::mpsc::channel::<LogEvent>();

        let logger = Self {
            tx,
            config: config.clone(),
        };

        let print_config = config.print_config;
        if print_config {
            println!("STATIC_MAX_LEVEL={STATIC_MAX_LEVEL}");
            println!("Logger initialized with {config:?} {file_config:?}");
        }

        let handle: Option<std::thread::JoinHandle<()>>;
        match set_boxed_logger(Box::new(logger)) {
            Ok(()) => {
                handle = Some(
                    std::thread::Builder::new()
                        .name(LOGGING.to_string())
                        .spawn(move || {
                            Self::handle_messages(
                                trader_id.to_string(),
                                instance_id.to_string(),
                                config,
                                file_config,
                                rx,
                            );
                        })?,
                );

                let max_level = log::LevelFilter::Trace;
                set_max_level(max_level);
                if print_config {
                    println!("Logger set as `log` implementation with max level {max_level}");
                }
            }
            Err(e) => {
                anyhow::bail!("Cannot initialize logger because of error: {e}");
            }
        }

        Ok(LogGuard::new(handle))
    }

    fn handle_messages(
        trader_id: String,
        instance_id: String,
        config: LoggerConfig,
        file_config: FileWriterConfig,
        rx: std::sync::mpsc::Receiver<LogEvent>,
    ) {
        let LoggerConfig {
            stdout_level,
            fileout_level,
            ref component_level,
            is_colored,
            print_config: _,
        } = config;

        let trader_id_cache = Ustr::from(&trader_id);

        // Set up std I/O buffers
        let mut stdout_writer = StdoutWriter::new(stdout_level, is_colored);
        let mut stderr_writer = StderrWriter::new(is_colored);

        // Conditionally create file writer based on fileout_level
        let mut file_writer_opt = if fileout_level == LevelFilter::Off {
            None
        } else {
            FileWriter::new(trader_id, instance_id, file_config, fileout_level)
        };

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            match event {
                LogEvent::Flush => {
                    break;
                }
                LogEvent::Log(line) => {
                    let component_level = component_level.get(&line.component);

                    // Check if the component exists in level_filters,
                    // and if its level is greater than event.level.
                    if let Some(&filter_level) = component_level {
                        if line.level > filter_level {
                            continue;
                        }
                    }

                    let mut wrapper = LogLineWrapper::new(line, trader_id_cache);

                    if stderr_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stderr_writer.write(wrapper.get_colored());
                        } else {
                            stderr_writer.write(wrapper.get_string());
                        }
                    }

                    if stdout_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stdout_writer.write(wrapper.get_colored());
                        } else {
                            stdout_writer.write(wrapper.get_string());
                        }
                    }

                    if let Some(ref mut writer) = file_writer_opt {
                        if writer.enabled(&wrapper.line) {
                            if writer.json_format {
                                writer.write(&wrapper.get_json());
                            } else {
                                writer.write(wrapper.get_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn log<T: AsRef<str>>(level: LogLevel, color: LogColor, component: Ustr, message: T) {
    let color = Value::from(color as u8);

    match level {
        LogLevel::Off => {}
        LogLevel::Trace => {
            log::trace!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Debug => {
            log::debug!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Info => {
            log::info!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Warning => {
            log::warn!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Error => {
            log::error!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug)]
pub struct LogGuard {
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LogGuard {
    /// Creates a new [`LogGuard`] instance.
    #[must_use]
    pub const fn new(handle: Option<std::thread::JoinHandle<()>>) -> Self {
        Self { handle }
    }
}

impl Default for LogGuard {
    /// Creates a new default [`LogGuard`] instance.
    fn default() -> Self {
        Self::new(None)
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        log::logger().flush();
        if let Some(handle) = self.handle.take() {
            handle.join().expect("Error joining logging handle");
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, thread::sleep, time::Duration};

    use log::LevelFilter;
    use nautilus_core::UUID4;
    use nautilus_model::identifiers::TraderId;
    use rstest::*;
    use serde_json::Value;
    use tempfile::tempdir;
    use ustr::Ustr;

    use super::*;
    use crate::{
        enums::LogColor,
        logging::{logging_clock_set_static_mode, logging_clock_set_static_time},
        testing::wait_until,
    };

    #[rstest]
    fn log_message_serialization() {
        let log_message = LogLine {
            timestamp: UnixNanos::default(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Portfolio"),
            message: "This is a log message".to_string(),
        };

        let serialized_json = serde_json::to_string(&log_message).unwrap();
        let deserialized_value: Value = serde_json::from_str(&serialized_json).unwrap();

        assert_eq!(deserialized_value["level"], "INFO");
        assert_eq!(deserialized_value["component"], "Portfolio");
        assert_eq!(deserialized_value["message"], "This is a log message");
    }

    #[rstest]
    fn log_config_parsing() {
        let config =
            LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Error")
                .unwrap();
        assert_eq!(
            config,
            LoggerConfig {
                stdout_level: LevelFilter::Info,
                fileout_level: LevelFilter::Debug,
                component_level: HashMap::from_iter(vec![(
                    Ustr::from("RiskEngine"),
                    LevelFilter::Error
                )]),
                is_colored: true,
                print_config: false,
            }
        );
    }

    #[rstest]
    fn log_config_parsing2() {
        let config = LoggerConfig::from_spec("stdout=Warn;print_config;fileout=Error;").unwrap();
        assert_eq!(
            config,
            LoggerConfig {
                stdout_level: LevelFilter::Warn,
                fileout_level: LevelFilter::Error,
                component_level: HashMap::new(),
                is_colored: false,
                print_config: true,
            }
        );
    }

    #[rstest]
    fn test_logging_to_file() {
        let config = LoggerConfig {
            fileout_level: LevelFilter::Debug,
            ..Default::default()
        };

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        let log_guard = Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static_mode();
        logging_clock_set_static_time(1_650_000_000_000_000);

        log::info!(
            component = "RiskEngine";
            "This is a test."
        );

        let mut log_contents = String::new();

        wait_until(
            || {
                std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .any(|entry| entry.path().is_file())
            },
            Duration::from_secs(2),
        );

        drop(log_guard); // Ensure log buffers are flushed

        wait_until(
            || {
                let log_file_path = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .find(|entry| entry.path().is_file())
                    .expect("No files found in directory")
                    .path();
                dbg!(&log_file_path);
                log_contents =
                    std::fs::read_to_string(log_file_path).expect("Error while reading log file");
                !log_contents.is_empty()
            },
            Duration::from_secs(2),
        );

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INFO] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[rstest]
    fn test_log_component_level_filtering() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error").unwrap();

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        let log_guard = Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static_mode();
        logging_clock_set_static_time(1_650_000_000_000_000);

        log::info!(
            component = "RiskEngine";
            "This is a test."
        );

        drop(log_guard); // Ensure log buffers are flushed

        wait_until(
            || {
                if let Some(log_file) = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .find(|entry| entry.path().is_file())
                {
                    let log_file_path = log_file.path();
                    let log_contents = std::fs::read_to_string(log_file_path)
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
                .any(|entry| entry.path().is_file()),
            "Log file exists"
        );
    }

    #[rstest]
    fn test_logging_to_file_in_json_format() {
        let config =
            LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Info")
                .unwrap();

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            file_format: Some("json".to_string()),
            ..Default::default()
        };

        let log_guard = Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static_mode();
        logging_clock_set_static_time(1_650_000_000_000_000);

        log::info!(
            component = "RiskEngine";
            "This is a test."
        );

        let mut log_contents = String::new();

        drop(log_guard); // Ensure log buffers are flushed

        wait_until(
            || {
                if let Some(log_file) = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .find(|entry| entry.path().is_file())
                {
                    let log_file_path = log_file.path();
                    log_contents = std::fs::read_to_string(log_file_path)
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
            "{\"timestamp\":\"1970-01-20T02:20:00.000000000Z\",\"trader_id\":\"TRADER-001\",\"level\":\"INFO\",\"color\":\"NORMAL\",\"component\":\"RiskEngine\",\"message\":\"This is a test.\"}\n"
        );
    }

    #[ignore = "Flaky test: Passing locally on some systems, failing in CI"]
    #[rstest]
    fn test_file_rotation_and_backup_limits() {
        // Create a temporary directory for log files
        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let dir_path = temp_dir.path().to_str().unwrap().to_string();

        // Configure a small max file size to trigger rotation quickly
        let max_backups = 3;
        let max_file_size = 100;
        let file_config = FileWriterConfig {
            directory: Some(dir_path.clone()),
            file_name: None,
            file_format: Some("log".to_string()),
            file_rotate: Some((max_file_size, max_backups).into()), // 100 bytes max size, 3 max backups
        };

        // Create the file writer
        let config = LoggerConfig::from_spec("fileout=Info;Test=Info").unwrap();
        let log_guard = Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        log::info!(
            component = "Test";
            "Test log message with enough content to exceed our small max file size limit"
        );

        sleep(Duration::from_millis(100));

        // Count the number of log files in the directory
        let files: Vec<_> = std::fs::read_dir(&dir_path)
            .expect("Failed to read directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "log"))
            .collect();

        // We should have multiple files due to rotation
        assert_eq!(files.len(), 1);

        log::info!(
            component = "Test";
            "Test log message with enough content to exceed our small max file size limit"
        );

        sleep(Duration::from_millis(100));

        // Count the number of log files in the directory
        let files: Vec<_> = std::fs::read_dir(&dir_path)
            .expect("Failed to read directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "log"))
            .collect();

        // We should have multiple files due to rotation
        assert_eq!(files.len(), 2);

        for _ in 0..5 {
            // Write enough data to trigger a few rotations
            log::info!(
            component = "Test";
            "Test log message with enough content to exceed our small max file size limit"
            );

            sleep(Duration::from_millis(100));
        }

        // Count the number of log files in the directory
        let files: Vec<_> = std::fs::read_dir(&dir_path)
            .expect("Failed to read directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "log"))
            .collect();

        // We should have at most max_backups + 1 files (current file + backups)
        assert!(
            files.len() == max_backups as usize + 1,
            "Expected at most {} log files, found {}",
            max_backups,
            files.len()
        );

        // Clean up
        drop(log_guard);
        drop(temp_dir);
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    kv::{ToValue, Value},
    set_boxed_logger, set_max_level, Level, LevelFilter, Log, STATIC_MAX_LEVEL,
};
use nautilus_core::{
    datetime::unix_nanos_to_iso8601,
    nanos::UnixNanos,
    time::{get_atomic_clock_realtime, get_atomic_clock_static},
    uuid::UUID4,
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

    #[must_use]
    pub fn from_spec(spec: &str) -> Self {
        let Self {
            mut stdout_level,
            mut fileout_level,
            mut component_level,
            mut is_colored,
            mut print_config,
        } = Self::default();
        spec.split(';').for_each(|kv| {
            if kv == "is_colored" {
                is_colored = true;
            } else if kv == "print_config" {
                print_config = true;
            } else {
                let mut kv = kv.split('=');
                if let (Some(k), Some(Ok(lvl))) = (kv.next(), kv.next().map(LevelFilter::from_str))
                {
                    if k == "stdout" {
                        stdout_level = lvl;
                    } else if k == "fileout" {
                        fileout_level = lvl;
                    } else {
                        component_level.insert(Ustr::from(k), lvl);
                    }
                }
            }
        });

        Self {
            stdout_level,
            fileout_level,
            component_level,
            is_colored,
            print_config,
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        match env::var("NAUTILUS_LOG") {
            Ok(spec) => Self::from_spec(&spec),
            Err(e) => panic!("Error parsing `LoggerConfig` spec: {e}"),
        }
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
    /// The timestamp of when the log event occurred.
    timestamp: String,
    /// The ID of the trader associated with this log event.
    trader_id: Ustr,
}

impl LogLineWrapper {
    /// Creates a new [`LogLineWrapper`] instance.
    #[must_use]
    pub fn new(line: LogLine, trader_id: Ustr, timestamp: UnixNanos) -> Self {
        Self {
            line,
            cache: None,
            colored: None,
            timestamp: unix_nanos_to_iso8601(timestamp),
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
                self.timestamp,
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
                self.timestamp,
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
        json_obj.insert("timestamp".to_string(), self.timestamp.clone());
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
            let key_values = record.key_values();
            let color = key_values
                .get("color".into())
                .and_then(|v| v.to_u64().map(|v| (v as u8).into()))
                .unwrap_or(LogColor::Normal);
            let component = key_values.get("component".into()).map_or_else(
                || Ustr::from(record.metadata().target()),
                |v| Ustr::from(&v.to_string()),
            );

            let line = LogLine {
                level: record.level(),
                color,
                component,
                message: format!("{}", record.args()),
            };
            if let Err(SendError(LogEvent::Log(line))) = self.tx.send(LogEvent::Log(line)) {
                eprintln!("Error sending log event: {line}");
            }
        }
    }

    fn flush(&self) {
        if let Err(e) = self.tx.send(LogEvent::Flush) {
            eprintln!("Error sending flush log event: {e}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
impl Logger {
    #[must_use]
    pub fn init_with_env(
        trader_id: TraderId,
        instance_id: UUID4,
        file_config: FileWriterConfig,
    ) -> LogGuard {
        let config = LoggerConfig::from_env();
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
    #[must_use]
    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
        file_config: FileWriterConfig,
    ) -> LogGuard {
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

        let mut handle: Option<std::thread::JoinHandle<()>> = None;
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
                        })
                        .expect("Error spawning thread '{LOGGING}'"),
                );

                let max_level = log::LevelFilter::Trace;
                set_max_level(max_level);
                if print_config {
                    println!("Logger set as `log` implementation with max level {max_level}");
                }
            }
            Err(e) => {
                eprintln!("Cannot set logger because of error: {e}");
            }
        }

        LogGuard::new(handle)
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
        let mut file_writer_opt = if fileout_level != LevelFilter::Off {
            FileWriter::new(trader_id, instance_id, file_config, fileout_level)
        } else {
            None
        };

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            match event {
                LogEvent::Flush => {
                    break;
                }
                LogEvent::Log(line) => {
                    let timestamp = match LOGGING_REALTIME.load(Ordering::Relaxed) {
                        true => get_atomic_clock_realtime().get_time_ns(),
                        false => get_atomic_clock_static().get_time_ns(),
                    };

                    let component_level = component_level.get(&line.component);

                    // Check if the component exists in level_filters,
                    // and if its level is greater than event.level.
                    if let Some(&filter_level) = component_level {
                        if line.level > filter_level {
                            continue;
                        }
                    }

                    let mut wrapper = LogLineWrapper::new(line, trader_id_cache, timestamp);

                    if stderr_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stderr_writer.write(wrapper.get_colored());
                        } else {
                            stderr_writer.write(wrapper.get_string());
                        }
                        // TODO: remove flushes once log guard is implemented
                        stderr_writer.flush();
                    }

                    if stdout_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stdout_writer.write(wrapper.get_colored());
                        } else {
                            stdout_writer.write(wrapper.get_string());
                        }
                        stdout_writer.flush();
                    }

                    if let Some(ref mut writer) = file_writer_opt {
                        if writer.enabled(&wrapper.line) {
                            if writer.json_format {
                                writer.write(&wrapper.get_json());
                            } else {
                                writer.write(wrapper.get_string());
                            }
                            writer.flush();
                        }
                    }
                }
            }
        }
    }
}

pub fn log(level: LogLevel, color: LogColor, component: Ustr, message: &str) {
    let color = Value::from(color as u8);

    match level {
        LogLevel::Off => {}
        LogLevel::Trace => {
            log::trace!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Debug => {
            log::debug!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Info => {
            log::info!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Warning => {
            log::warn!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Error => {
            log::error!(component = component.to_value(), color = color; "{}", message);
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
    use std::{collections::HashMap, time::Duration};

    use log::LevelFilter;
    use nautilus_core::uuid::UUID4;
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
            LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Error");
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
        let config = LoggerConfig::from_spec("stdout=Warn;print_config;fileout=Error;");
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

        drop(log_guard); // Ensure log buffers are flushed

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INFO] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[rstest]
    fn test_log_component_level_filtering() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error");

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

        drop(log_guard); // Ensure log buffers are flushed

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
            LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Info");

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

        drop(log_guard); // Ensure log buffers are flushed

        assert_eq!(
        log_contents,
        "{\"timestamp\":\"1970-01-20T02:20:00.000000000Z\",\"trader_id\":\"TRADER-001\",\"level\":\"INFO\",\"color\":\"NORMAL\",\"component\":\"RiskEngine\",\"message\":\"This is a test.\"}\n"
    );
    }
}

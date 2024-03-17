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
    env, fmt,
    str::FromStr,
    sync::{
        atomic::Ordering,
        mpsc::{channel, Receiver, SendError, Sender},
    },
    thread,
};

use indexmap::IndexMap;
use log::{
    debug, error, info,
    kv::{ToValue, Value},
    set_boxed_logger, set_max_level, warn, Level, LevelFilter, Log, STATIC_MAX_LEVEL,
};
use nautilus_core::{
    datetime::unix_nanos_to_iso8601,
    time::{get_atomic_clock_realtime, get_atomic_clock_static, UnixNanos},
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use serde::{Deserialize, Serialize, Serializer};
use ustr::Ustr;

use super::{LOGGING_BYPASSED, LOGGING_REALTIME};
use crate::{
    enums::{LogColor, LogLevel},
    logging::writer::{FileWriter, FileWriterConfig, LogWriter, StderrWriter, StdoutWriter},
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerConfig {
    /// Maximum log level to write to stdout.
    pub stdout_level: LevelFilter,
    /// Maximum log level to write to file.
    pub fileout_level: LevelFilter,
    /// Maximum log level to write for a given component.
    component_level: HashMap<Ustr, LevelFilter>,
    /// If logger is using ANSI color codes.
    pub is_colored: bool,
    /// If the configuration should be printed to stdout at initialization.
    pub print_config: bool,
}

impl Default for LoggerConfig {
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
    pub fn new(
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

    pub fn from_env() -> Self {
        match env::var("NAUTILUS_LOG") {
            Ok(spec) => LoggerConfig::from_spec(&spec),
            Err(e) => panic!("Error parsing `LoggerConfig` spec: {e}"),
        }
    }
}

/// Initialize tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level only using
/// by passing a configuration using the `RUST_LOG` environment variable.

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives [`LogEvent`] structs over the
/// channel.
#[derive(Debug)]
pub struct Logger {
    /// Configure maximum levels for components and IO.
    pub config: LoggerConfig,
    /// Send log events to a different thread.
    tx: Sender<LogEvent>,
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

impl fmt::Display for LogLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.level, self.component, self.message)
    }
}

pub struct LogLineWrapper {
    line: LogLine,
    cache: Option<String>,
    colored: Option<String>,
    timestamp: String,
    trader_id: Ustr,
}

impl LogLineWrapper {
    pub fn new(line: LogLine, trader_id: Ustr, timestamp: UnixNanos) -> Self {
        LogLineWrapper {
            line,
            cache: None,
            colored: None,
            timestamp: unix_nanos_to_iso8601(timestamp),
            trader_id,
        }
    }

    pub fn get_string(&mut self) -> &str {
        self.cache.get_or_insert_with(|| {
            format!(
                "{} [{}] {}.{}: {}\n",
                self.timestamp,
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message
            )
        })
    }

    pub fn get_colored(&mut self) -> &str {
        self.colored.get_or_insert_with(|| {
            format!(
                "\x1b[1m{}\x1b[0m {}[{}] {}.{}: {}\x1b[0m\n",
                self.timestamp,
                &self.line.color.as_ansi(),
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message
            )
        })
    }

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
            let component = key_values
                .get("component".into())
                .map(|v| Ustr::from(&v.to_string()))
                .unwrap_or_else(|| Ustr::from(record.metadata().target()));

            let line = LogLine {
                level: record.level(),
                color,
                component,
                message: format!("{}", record.args()).to_string(),
            };
            if let Err(SendError(LogEvent::Log(line))) = self.tx.send(LogEvent::Log(line)) {
                eprintln!("Error sending log event: {line}");
            }
        }
    }

    fn flush(&self) {
        self.tx.send(LogEvent::Flush).unwrap();
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
        Logger::init_with_config(trader_id, instance_id, config, file_config)
    }

    #[must_use]
    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
        file_config: FileWriterConfig,
    ) -> LogGuard {
        let (tx, rx) = channel::<LogEvent>();

        let logger = Self {
            tx,
            config: config.clone(),
        };

        let print_config = config.print_config;
        if print_config {
            println!("STATIC_MAX_LEVEL={STATIC_MAX_LEVEL}");
            println!("Logger initialized with {:?} {:?}", config, file_config);
        }

        match set_boxed_logger(Box::new(logger)) {
            Ok(_) => {
                let _join_handle = thread::Builder::new()
                    .name("logging".to_string())
                    .spawn(move || {
                        Self::handle_messages(
                            trader_id.to_string(),
                            instance_id.to_string(),
                            config,
                            file_config,
                            rx,
                        );
                    })
                    .expect("Error spawning `logging` thread");

                let max_level = log::LevelFilter::Debug;
                set_max_level(max_level);
                if print_config {
                    println!("Logger set as `log` implementation with max level {max_level}");
                }
            }
            Err(e) => {
                eprintln!("Cannot set logger because of error: {e}")
            }
        }

        LogGuard::new()
    }

    fn handle_messages(
        trader_id: String,
        instance_id: String,
        config: LoggerConfig,
        file_config: FileWriterConfig,
        rx: Receiver<LogEvent>,
    ) {
        if config.print_config {
            println!("Logger thread `handle_messages` initialized")
        }

        let LoggerConfig {
            stdout_level,
            fileout_level,
            ref component_level,
            is_colored,
            print_config: _,
        } = config;

        let trader_id_cache = Ustr::from(&trader_id);

        // Setup std I/O buffers
        let mut stdout_writer = StdoutWriter::new(stdout_level, is_colored);
        let mut stderr_writer = StderrWriter::new(is_colored);

        // Conditionally create file writer based on fileout_level
        let mut file_writer_opt = if fileout_level != LevelFilter::Off {
            FileWriter::new(trader_id.clone(), instance_id, file_config, fileout_level)
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
        LogLevel::Debug => {
            debug!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Info => {
            info!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Warning => {
            warn!(component = component.to_value(), color = color; "{}", message);
        }
        LogLevel::Error => {
            error!(component = component.to_value(), color = color; "{}", message);
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug)]
pub struct LogGuard {}

impl LogGuard {
    pub fn new() -> Self {
        LogGuard {}
    }
}

impl Default for LogGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        log::logger().flush();
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use log::{info, LevelFilter};
    use nautilus_core::uuid::UUID4;
    use nautilus_model::identifiers::trader_id::TraderId;
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
        )
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
        )
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

        info!(
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

        info!(
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

        info!(
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

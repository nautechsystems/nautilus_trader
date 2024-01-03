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
    env, fmt,
    fs::{create_dir_all, File},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::{sync_channel, Receiver, SendError, SyncSender},
    thread,
};

use chrono::{prelude::*, Utc};
use log::{set_boxed_logger, Level, LevelFilter, Log, STATIC_MAX_LEVEL};
use nautilus_core::{
    datetime::unix_nanos_to_iso8601,
    time::{get_atomic_clock, AtomicTime, UnixNanos},
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::enums::LogColor;

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerConfig {
    /// Maximum log level to write to stdout.
    stdout_level: LevelFilter,
    /// Maximum log level to write to file.
    fileout_level: LevelFilter,
    /// Maximum log level to write for a given component.
    component_level: HashMap<Ustr, LevelFilter>,
    /// If logger is using ANSI color codes.
    pub is_colored: bool,
    /// If logging is bypassed.
    pub is_bypassed: bool,
    /// If the Rust logging config should be printed to stdout at initialization.
    pub print_config: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            stdout_level: LevelFilter::Info,
            fileout_level: LevelFilter::Off,
            component_level: HashMap::new(),
            is_colored: true,
            is_bypassed: false,
            print_config: false,
        }
    }
}

impl LoggerConfig {
    pub fn from_spec(spec: &str) -> Self {
        let Self {
            mut stdout_level,
            mut fileout_level,
            mut component_level,
            mut is_colored,
            mut is_bypassed,
            mut print_config,
        } = Self::default();
        spec.split(';').for_each(|kv| {
            if kv == "is_colored" {
                is_colored = true;
            } else if kv == "is_bypassed" {
                is_bypassed = true;
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
            is_bypassed,
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

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, Default)]
pub struct FileWriterConfig {
    directory: Option<String>,
    file_name: Option<String>,
    file_format: Option<String>,
}

impl FileWriterConfig {
    pub fn new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
    ) -> Self {
        Self {
            directory,
            file_name,
            file_format,
        }
    }
}

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives [`LogEvent`] structs over the
/// channel.
pub struct Logger {
    /// Reference to the atomic clock used by the engine.
    clock: &'static AtomicTime,
    /// Send log events to a different thread.
    tx: SyncSender<LogEvent>,
    /// Configure maximum levels for components and IO.
    pub config: LoggerConfig,
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
    /// The UNIX nanoseconds timestamp when the log event occurred.
    timestamp: UnixNanos,
    /// The log level for the event.
    level: Level,
    /// The color for the log message content.
    color: LogColor,
    /// The Nautilus system component the log event originated from.
    component: Ustr,
    /// The log message content.
    message: String,
}

impl fmt::Display for LogLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] {}: {}",
            self.timestamp, self.level, self.component, self.message
        )
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !self.config.is_bypassed
            && (metadata.level() == Level::Error
                || metadata.level() >= self.config.stdout_level
                || metadata.level() >= self.config.fileout_level)
    }

    fn log(&self, record: &log::Record) {
        // TODO remove unwraps
        if self.enabled(record.metadata()) {
            let timestamp = self.clock.get_time_ns();
            let color = record
                .key_values()
                .get("color".into())
                .and_then(|v| v.to_u64().map(|v| (v as u8).into()))
                .unwrap_or(LogColor::Normal);
            let component = record
                .key_values()
                .get("component".into())
                .map(|v| Ustr::from(&v.to_string()))
                .unwrap_or_else(|| Ustr::from(record.metadata().target()));
            let line = LogLine {
                timestamp,
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
    pub fn init_with_env(
        trader_id: TraderId,
        instance_id: UUID4,
        file_writer_config: FileWriterConfig,
        clock: Option<&'static AtomicTime>,
    ) {
        let config = LoggerConfig::from_env();
        Logger::init_with_config(trader_id, instance_id, file_writer_config, config, clock);
    }

    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        file_writer_config: FileWriterConfig,
        config: LoggerConfig,
        clock: Option<&'static AtomicTime>,
    ) {
        let (tx, rx) = sync_channel::<LogEvent>(0);

        let trader_id_clone = trader_id.value.to_string();
        let instance_id_clone = instance_id.to_string();

        let logger = Self {
            clock: clock.unwrap_or(get_atomic_clock()),
            tx,
            config: config.clone(),
        };

        if config.print_config {
            println!("Initializing logger with {:?}", config);
            println!("STATIC_MAX_LEVEL={}", STATIC_MAX_LEVEL);
        }

        match set_boxed_logger(Box::new(logger)) {
            Ok(_) => {
                thread::spawn(move || {
                    Self::handle_messages(
                        &trader_id_clone,
                        &instance_id_clone,
                        file_writer_config,
                        config,
                        rx,
                    );
                });
            }
            Err(e) => {
                eprintln!("Cannot set logger because of error: {}", e)
            }
        }
    }

    #[allow(unused_variables)] // `is_bypassed` is unused
    #[allow(clippy::useless_format)] // Format is not actually useless as we escape braces
    fn handle_messages(
        trader_id: &str,
        instance_id: &str,
        file_writer_config: FileWriterConfig,
        config: LoggerConfig,
        rx: Receiver<LogEvent>,
    ) {
        if config.print_config {
            println!("Logging `handle_messages` thread initialized")
        }

        let LoggerConfig {
            stdout_level,
            fileout_level,
            component_level,
            is_colored,
            is_bypassed,
            print_config: _,
        } = config;

        // Setup std I/O buffers
        let mut out_buf = BufWriter::new(io::stdout());
        let mut err_buf = BufWriter::new(io::stderr());

        // Setup log file
        let is_json_format = match file_writer_config
            .file_format
            .as_ref()
            .map(|s| s.to_lowercase())
        {
            Some(ref format) if format == "json" => true,
            None => false,
            Some(ref unrecognized) => {
                eprintln!(
                    "Unrecognized log file format: {unrecognized}. Using plain text format as default."
                );
                false
            }
        };

        let file_path = PathBuf::new();
        let file = if fileout_level > LevelFilter::Off {
            let file_path = Self::create_log_file_path(
                &file_writer_config,
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
        let template_console = match is_colored {
            true => format!("\x1b[1m{{ts}}\x1b[0m {{color}}[{{level}}] {{trader_id}}.{{component}}: {{message}}\x1b[0m\n"),
            false => format!("{{ts}} [{{level}}] {{trader_id}}.{{component}}: {{message}}\n")
        };

        let template_file = String::from("{ts} [{level}] {trader_id}.{component}: {message}\n");

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            match event {
                LogEvent::Flush => {
                    Self::flush_stderr(&mut err_buf);
                    Self::flush_stdout(&mut out_buf);
                    file_buf.as_mut().map(Self::flush_file);
                }
                LogEvent::Log(line) => {
                    let component_level = component_level.get(&line.component);

                    // Check if the component exists in level_filters and if its level is greater than event.level
                    if let Some(&filter_level) = component_level {
                        if line.level > filter_level {
                            continue;
                        }
                    }

                    if line.level == LevelFilter::Error {
                        let line = Self::format_colored_log(&line, trader_id, &template_console);
                        Self::write_stderr(&mut err_buf, &line);
                        Self::flush_stderr(&mut err_buf);
                    } else if line.level <= stdout_level {
                        let line = Self::format_colored_log(&line, trader_id, &template_console);
                        Self::write_stdout(&mut out_buf, &line);
                        Self::flush_stdout(&mut out_buf);
                    }

                    if fileout_level != LevelFilter::Off {
                        if Self::should_rotate_file(&file_path) {
                            // Ensure previous file buffer flushed
                            if let Some(file_buf) = file_buf.as_mut() {
                                Self::flush_file(file_buf);
                            };

                            let file_path = Self::create_log_file_path(
                                &file_writer_config,
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

                        if line.level <= fileout_level {
                            if let Some(file_buf) = file_buf.as_mut() {
                                let line = Self::format_line(
                                    &line,
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
        format!("{trader_id}_{current_date_utc}_{instance_id}")
    }

    fn create_log_file_path(
        file_writer_config: &FileWriterConfig,
        trader_id: &str,
        instance_id: &str,
        is_json_format: bool,
    ) -> PathBuf {
        let basename = if let Some(file_name) = file_writer_config.file_name.as_ref() {
            file_name.clone()
        } else {
            Self::default_log_file_basename(trader_id, instance_id)
        };

        let suffix = if is_json_format { "json" } else { "log" };
        let mut file_path = PathBuf::new();

        if let Some(directory) = file_writer_config.directory.as_ref() {
            file_path.push(directory);
            create_dir_all(&file_path).expect("Failed to create directories for log file");
        }

        file_path.push(basename);
        file_path.set_extension(suffix);
        file_path
    }

    fn format_colored_log(event: &LogLine, trader_id: &str, _template: &str) -> String {
        format!(
            "\x1b[1m{ts}\x1b[0m {color}[{level}] {trader_id}.{component}: {message}\x1b[0m\n",
            ts = unix_nanos_to_iso8601(event.timestamp),
            color = &event.color.to_string(),
            level = event.level,
            trader_id = trader_id,
            component = &event.component,
            message = &event.message
        )
    }

    fn format_line(
        event: &LogLine,
        trader_id: &str,
        _template: &str,
        is_json_format: bool,
    ) -> String {
        if is_json_format {
            let json_string =
                serde_json::to_string(event).expect("Error serializing log event to string");
            format!("{json_string}\n")
        } else {
            format!(
                "{ts} [{level}] {trader_id}.{component}: {message}\n",
                ts = &unix_nanos_to_iso8601(event.timestamp),
                level = &event.level.to_string(),
                trader_id = trader_id,
                component = &event.component,
                message = &event.message,
            )
        }
    }

    fn write_stdout(out_buf: &mut BufWriter<Stdout>, line: &str) {
        match out_buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stdout: {e:?}"),
        }
    }

    fn flush_stdout(out_buf: &mut BufWriter<Stdout>) {
        match out_buf.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error flushing stdout: {e:?}"),
        }
    }

    fn write_stderr(err_buf: &mut BufWriter<Stderr>, line: &str) {
        match err_buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stderr: {e:?}"),
        }
    }

    fn flush_stderr(err_buf: &mut BufWriter<Stderr>) {
        match err_buf.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error flushing stderr: {e:?}"),
        }
    }

    fn write_file(file_buf: &mut BufWriter<File>, line: &str) {
        match file_buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to file: {e:?}"),
        }
    }

    fn flush_file(file_buf: &mut BufWriter<File>) {
        match file_buf.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to file: {e:?}"),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use log::{info, LevelFilter};
    use nautilus_core::{time::set_atomic_clock_static, uuid::UUID4};
    use nautilus_model::identifiers::trader_id::TraderId;
    use rstest::*;
    use serde_json::Value;
    use tempfile::tempdir;
    use ustr::Ustr;

    use super::FileWriterConfig;
    use crate::{
        enums::LogColor,
        logging::{LogLine, Logger, LoggerConfig},
        testing::wait_until,
    };

    #[rstest]
    fn log_message_serialization() {
        let log_message = LogLine {
            timestamp: 1_000_000_000,
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Portfolio"),
            message: "This is a log message".to_string(),
        };

        let serialized_json = serde_json::to_string(&log_message).unwrap();
        let deserialized_value: Value = serde_json::from_str(&serialized_json).unwrap();

        assert_eq!(deserialized_value["timestamp"], 1_000_000_000);
        assert_eq!(deserialized_value["level"], "INFO");
        assert_eq!(deserialized_value["component"], "Portfolio");
        assert_eq!(deserialized_value["message"], "This is a log message");
    }

    #[rstest]
    fn log_config_parsing() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error");
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
                is_bypassed: false,
                print_config: false,
            }
        )
    }

    #[ignore] // TODO: Ignore pre nextest setup
    #[rstest]
    fn test_logging_to_file() {
        let config = LoggerConfig {
            fileout_level: LevelFilter::Debug,
            ..Default::default()
        };

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_writer_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        set_atomic_clock_static(1_650_000_000_000_000);

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            file_writer_config,
            config,
            None,
        );

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

        assert_eq!(
            log_contents,
            "1970-01-20T02:20:00.000000000Z [INFO] TRADER-001.RiskEngine: This is a test.\n"
        );
    }

    #[ignore] // TODO: Ignore pre nextest setup
    #[rstest]
    fn test_log_component_level_filtering() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error");

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_writer_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        set_atomic_clock_static(1_650_000_000_000_000);

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            file_writer_config,
            config,
            None,
        );

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

        assert!(
            std::fs::read_dir(&temp_dir)
                .expect("Failed to read directory")
                .filter_map(Result::ok)
                .any(|entry| entry.path().is_file()),
            "Log file exists"
        );
    }

    #[ignore] // TODO: Ignore pre nextest setup
    #[rstest]
    fn test_logging_to_file_in_json_format() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Info");

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_writer_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            file_format: Some("json".to_string()),
            ..Default::default()
        };

        set_atomic_clock_static(1_650_000_000_000_000);

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            file_writer_config,
            config,
            None,
        );

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

        assert_eq!(
        log_contents,
        "{\"timestamp\":1650000000000000,\"level\":\"INFO\",\"color\":\"Normal\",\"component\":\"RiskEngine\",\"message\":\"This is a test.\"}\n"
    );
    }
}

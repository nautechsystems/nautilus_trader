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
    borrow::Cow,
    collections::HashMap,
    env, fmt,
    fs::{create_dir_all, File},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, SendError, Sender},
    },
    thread,
};

use chrono::{prelude::*, Utc};
use human_bytes::human_bytes;
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
use serde::{Deserialize, Serialize};
use sysinfo::System;
use tracing_subscriber::EnvFilter;
use ustr::Ustr;

use crate::{
    enums::{LogColor, LogLevel},
    python::versioning::{get_python_package_version, get_python_version},
};

static LOGGING_INITIALIZED: AtomicBool = AtomicBool::new(false);
static LOGGING_BYPASSED: AtomicBool = AtomicBool::new(false);
static LOGGING_REALTIME: AtomicBool = AtomicBool::new(true);
static LOGGING_COLORED: AtomicBool = AtomicBool::new(true);

/// Returns whether the core logger is enabled.
#[no_mangle]
pub extern "C" fn logging_is_initialized() -> u8 {
    LOGGING_INITIALIZED.load(Ordering::Relaxed) as u8
}

/// Sets the logging system to bypass mode.
#[no_mangle]
pub extern "C" fn logging_set_bypass() {
    LOGGING_BYPASSED.store(true, Ordering::Relaxed)
}

/// Shuts down the logging system.
#[no_mangle]
pub extern "C" fn logging_shutdown() {
    todo!()
}

/// Returns whether the core logger is using ANSI colors.
#[no_mangle]
pub extern "C" fn logging_is_colored() -> u8 {
    LOGGING_COLORED.load(Ordering::Relaxed) as u8
}

/// Sets the global logging clock to real-time mode.
#[no_mangle]
pub extern "C" fn logging_clock_set_realtime() {
    LOGGING_REALTIME.store(true, Ordering::Relaxed);
}

/// Sets the global logging clock to static mode with the given UNIX time (nanoseconds).
#[no_mangle]
pub extern "C" fn logging_clock_set_static(time_ns: u64) {
    LOGGING_REALTIME.store(false, Ordering::Relaxed);
    let clock = get_atomic_clock_static();
    clock.set_time(time_ns);
}

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

pub fn map_log_level_to_filter(log_level: LogLevel) -> LevelFilter {
    match log_level {
        LogLevel::Off => LevelFilter::Off,
        LogLevel::Debug => LevelFilter::Debug,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Warning => LevelFilter::Warn,
        LogLevel::Error => LevelFilter::Error,
    }
}

pub fn parse_level_filter_str(s: &str) -> LevelFilter {
    let mut log_level_str = s.to_string().to_uppercase();
    if log_level_str == "WARNING" {
        log_level_str = "WARN".to_string()
    }
    LevelFilter::from_str(&log_level_str)
        .unwrap_or_else(|_| panic!("Invalid `LevelFilter` string, was {log_level_str}"))
}

pub fn parse_component_levels(
    original_map: Option<HashMap<String, serde_json::Value>>,
) -> HashMap<Ustr, LevelFilter> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let value = parse_level_filter_str(value.as_str().unwrap());
                new_map.insert(ustr_key, value);
            }
            new_map
        }
        None => HashMap::new(),
    }
}

/// Initialize tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level only using
/// by passing a configuration using the `RUST_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
pub fn init_tracing() {
    // Skip tracing initialization if `RUST_LOG` is not set
    if let Ok(v) = env::var("RUST_LOG") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(v.clone()))
            .try_init()
            .unwrap_or_else(|e| eprintln!("Cannot set tracing subscriber because of error: {e}"));
        println!("Initialized tracing logs with RUST_LOG={v}");
    }
}

/// Initialize logging.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the main `nautilus_trader` package.
/// Logging can be configured to filter components and write up to a specific level only
/// by passing a configuration using the `NAUTILUS_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
pub fn init_logging(
    trader_id: TraderId,
    instance_id: UUID4,
    config: LoggerConfig,
    file_config: FileWriterConfig,
) {
    LOGGING_INITIALIZED.store(true, Ordering::Relaxed);
    LOGGING_COLORED.store(config.is_colored, Ordering::Relaxed);
    Logger::init_with_config(trader_id, instance_id, config, file_config);
}

/// Provides a high-performance logger utilizing a MPSC channel under the hood.
///
/// A separate thead is spawned at initialization which receives [`LogEvent`] structs over the
/// channel.
#[derive(Debug)]
pub struct Logger {
    /// Send log events to a different thread.
    tx: Sender<LogEvent>,
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
        write!(f, "[{}] {}: {}", self.level, self.component, self.message)
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !LOGGING_BYPASSED.load(Ordering::Relaxed)
            && (metadata.level() >= self.config.stdout_level
                || metadata.level() >= self.config.fileout_level)
    }

    fn log(&self, record: &log::Record) {
        // TODO remove unwraps
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
    pub fn init_with_env(trader_id: TraderId, instance_id: UUID4, file_config: FileWriterConfig) {
        let config = LoggerConfig::from_env();
        Logger::init_with_config(trader_id, instance_id, config, file_config);
    }

    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
        file_config: FileWriterConfig,
    ) {
        let (tx, rx) = channel::<LogEvent>();

        let trader_id_clone = trader_id.value.to_string();
        let instance_id_clone = instance_id.to_string();

        let logger = Self {
            tx,
            config: config.clone(),
        };

        let print_config = config.print_config;
        if print_config {
            println!("STATIC_MAX_LEVEL={STATIC_MAX_LEVEL}");
            println!("Logger initialized with {:?}", config);
        }

        match set_boxed_logger(Box::new(logger)) {
            Ok(_) => {
                thread::spawn(move || {
                    Self::handle_messages(
                        &trader_id_clone,
                        &instance_id_clone,
                        config,
                        file_config,
                        rx,
                    );
                });

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
    }

    fn handle_messages(
        trader_id: &str,
        instance_id: &str,
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
            component_level,
            is_colored,
            print_config: _,
        } = config;

        // Setup std I/O buffers
        let mut out_buf = BufWriter::new(io::stdout());
        let mut err_buf = BufWriter::new(io::stderr());

        // Setup log file
        let is_json_format = match file_config.file_format.as_ref().map(|s| s.to_lowercase()) {
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
            let file_path =
                Self::create_log_file_path(&file_config, trader_id, instance_id, is_json_format);

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

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            let timestamp = match LOGGING_REALTIME.load(Ordering::Relaxed) {
                true => get_atomic_clock_realtime().get_time_ns(),
                false => get_atomic_clock_static().get_time_ns(),
            };

            match event {
                LogEvent::Flush => {
                    Self::flush_stderr(&mut err_buf);
                    Self::flush_stdout(&mut out_buf);
                    file_buf.as_mut().map(Self::flush_file);
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

                    if line.level == LevelFilter::Error {
                        let line =
                            Self::format_console_log(timestamp, &line, trader_id, is_colored);
                        Self::write_stderr(&mut err_buf, &line);
                        Self::flush_stderr(&mut err_buf);
                    } else if line.level <= stdout_level {
                        let line =
                            Self::format_console_log(timestamp, &line, trader_id, is_colored);
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
                                &file_config,
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
                                let line = Self::format_file_log(
                                    timestamp,
                                    &line,
                                    trader_id,
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
        file_config: &FileWriterConfig,
        trader_id: &str,
        instance_id: &str,
        is_json_format: bool,
    ) -> PathBuf {
        let basename = if let Some(file_name) = file_config.file_name.as_ref() {
            file_name.clone()
        } else {
            Self::default_log_file_basename(trader_id, instance_id)
        };

        let suffix = if is_json_format { "json" } else { "log" };
        let mut file_path = PathBuf::new();

        if let Some(directory) = file_config.directory.as_ref() {
            file_path.push(directory);
            create_dir_all(&file_path).expect("Failed to create directories for log file");
        }

        file_path.push(basename);
        file_path.set_extension(suffix);
        file_path
    }

    fn format_console_log(
        timestamp: UnixNanos,
        event: &LogLine,
        trader_id: &str,
        is_colored: bool,
    ) -> String {
        match is_colored {
            true => format!(
                "\x1b[1m{}\x1b[0m {}[{}] {}.{}: {}\x1b[0m\n",
                unix_nanos_to_iso8601(timestamp),
                &event.color.to_string(),
                event.level,
                &trader_id,
                &event.component,
                &event.message
            ),
            false => format!(
                "{} [{}] {}.{}: {}\n",
                unix_nanos_to_iso8601(timestamp),
                event.level,
                &trader_id,
                &event.component,
                &event.message
            ),
        }
    }

    fn format_file_log(
        timestamp: UnixNanos,
        event: &LogLine,
        trader_id: &str,
        is_json_format: bool,
    ) -> String {
        if is_json_format {
            let json_string =
                serde_json::to_string(event).expect("Error serializing log event to string");
            format!("{json_string}\n")
        } else {
            format!(
                "{} [{}] {}.{}: {}\n",
                &unix_nanos_to_iso8601(timestamp),
                event.level,
                trader_id,
                &event.component,
                &event.message,
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

pub fn log(level: LogLevel, color: LogColor, component: Ustr, message: Cow<'_, str>) {
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

pub fn log_header(trader_id: TraderId, machine_id: &str, instance_id: UUID4, component: Ustr) {
    let mut sys = System::new_all();
    sys.refresh_all();

    let kernel_version = match System::kernel_version() {
        Some(v) => format!("Linux-{v} "),
        None => "".to_string(),
    };
    let os_version = match System::long_os_version() {
        Some(v) => v.replace("Linux ", ""),
        None => "".to_string(),
    };

    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" NAUTILUS TRADER - Automated Algorithmic Trading Platform"),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" by Nautech Systems Pty Ltd."),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" Copyright (C) 2015-2024. All rights reserved."),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(""),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣴⣶⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣾⣿⣿⣿⠀⢸⣿⣿⣿⣿⣶⣶⣤⣀⠀⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⠀⢀⣴⡇⢀⣾⣿⣿⣿⣿⣿⠀⣾⣿⣿⣿⣿⣿⣿⣿⠿⠓⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⣰⣿⣿⡀⢸⣿⣿⣿⣿⣿⣿⠀⣿⣿⣿⣿⣿⣿⠟⠁⣠⣄⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⢠⣿⣿⣿⣇⠀⢿⣿⣿⣿⣿⣿⠀⢻⣿⣿⣿⡿⢃⣠⣾⣿⣿⣧⡀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⢸⣿⣿⣿⣿⣆⠘⢿⣿⡿⠛⢉⠀⠀⠉⠙⠛⣠⣿⣿⣿⣿⣿⣿⣷⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠠⣾⣿⣿⣿⣿⣿⣧⠈⠋⢀⣴⣧⠀⣿⡏⢠⡀⢸⣿⣿⣿⣿⣿⣿⣿⡇⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⣀⠙⢿⣿⣿⣿⣿⣿⠇⢠⣿⣿⣿⡄⠹⠃⠼⠃⠈⠉⠛⠛⠛⠛⠛⠻⠇⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⢸⡟⢠⣤⠉⠛⠿⢿⣿⠀⢸⣿⡿⠋⣠⣤⣄⠀⣾⣿⣿⣶⣶⣶⣦⡄⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠸⠀⣾⠏⣸⣷⠂⣠⣤⠀⠘⢁⣴⣾⣿⣿⣿⡆⠘⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠛⠀⣿⡟⠀⢻⣿⡄⠸⣿⣿⣿⣿⣿⣿⣿⡀⠘⣿⣿⣿⣿⠟⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⠀⣿⠇⠀⠀⢻⡿⠀⠈⠻⣿⣿⣿⣿⣿⡇⠀⢹⣿⠿⠋⠀⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed("⠀⠀⠀⠀⠀⠀⠋⠀⠀⠀⡘⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠁⠀⠀⠀⠀⠀⠀⠀"),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(""),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" SYSTEM SPECIFICATION"),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("CPU architecture: {}", sys.cpus()[0].brand())),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "CPU(s): {} @ {} Mhz",
            sys.cpus().len(),
            sys.cpus()[0].frequency()
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("OS: {}{}", kernel_version, os_version)),
    );

    log_sysinfo(component);

    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" IDENTIFIERS"),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("trader_id: {trader_id}")),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("machine_id: {machine_id}")),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("instance_id: {instance_id}")),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" VERSIONING"),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    let package = "nautilus_trader";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("python: {}", get_python_version())),
    );
    let package = "numpy";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    let package = "pandas";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    let package = "msgspec";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    let package = "pyarrow";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    let package = "pytz";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    let package = "uvloop";
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "{package}: {}",
            get_python_package_version(package)
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
}

pub fn log_sysinfo(component: Ustr) {
    let mut sys = System::new_all();
    sys.refresh_all();

    let ram_total = sys.total_memory();
    let ram_used = sys.used_memory();
    let ram_avail = ram_total - ram_used;

    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();
    let swap_avail = swap_total - swap_used;

    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed(" MEMORY USAGE"),
    );
    log(
        LogLevel::Info,
        LogColor::Cyan,
        component,
        Cow::Borrowed("================================================================="),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("RAM-Total: {}", human_bytes(ram_total as f64),)),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "RAM-Used: {} ({:.2}%)",
            human_bytes(ram_used as f64),
            (ram_used as f64 / ram_total as f64) * 100.0,
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "RAM-Avail: {} ({:.2}%)",
            human_bytes(ram_avail as f64),
            (ram_avail as f64 / ram_total as f64) * 100.0,
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!("Swap-Total: {}", human_bytes(swap_total as f64))),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "Swap-Used: {} ({:.2}%)",
            human_bytes(swap_used as f64),
            (swap_used as f64 / swap_total as f64) * 100.0
        )),
    );
    log(
        LogLevel::Info,
        LogColor::Normal,
        component,
        Cow::Borrowed(&format!(
            "Swap-Avail: {} ({:.2}%)",
            human_bytes(swap_avail as f64),
            (swap_avail as f64 / swap_total as f64) * 100.0
        )),
    );
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
        logging::{LogLine, Logger, LoggerConfig},
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

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static(1_650_000_000_000_000);

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

    #[rstest]
    fn test_log_component_level_filtering() {
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error");

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static(1_650_000_000_000_000);

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

        Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        );

        logging_clock_set_static(1_650_000_000_000_000);

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
        "{\"level\":\"INFO\",\"color\":\"Normal\",\"component\":\"RiskEngine\",\"message\":\"This is a test.\"}\n"
    );
    }
}

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

//! The logging framework for Nautilus systems.
//!
//! This module implements a high-performance logging subsystem that operates in a separate thread
//! using an MPSC channel for log message delivery. The system uses reference counting to track
//! active `LogGuard` instances, ensuring the logging thread completes all pending writes before
//! termination.
//!
//! # LogGuard Reference Counting
//!
//! The logging system maintains a global count of active `LogGuard` instances using an atomic
//! counter (`LOGGING_GUARDS_ACTIVE`). When a `LogGuard` is created, the counter is incremented,
//! and when dropped, it's decremented. When the last `LogGuard` is dropped (counter reaches zero),
//! the logging thread is properly joined to ensure all buffered log messages are written to their
//! destinations before the process terminates.
//!
//! The system supports a maximum of 255 concurrent `LogGuard` instances. Attempting to create
//! more will cause a panic.

pub mod headers;
pub mod logger;
pub mod macros;
pub mod writer;

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use log::LevelFilter;
// Re-exports
pub use macros::{log_debug, log_error, log_info, log_trace, log_warn};
use nautilus_core::{UUID4, time::get_atomic_clock_static};
use nautilus_model::identifiers::TraderId;
use tracing_subscriber::EnvFilter;
use ustr::Ustr;

use self::{
    logger::{LogGuard, Logger, LoggerConfig},
    writer::FileWriterConfig,
};
use crate::enums::LogLevel;

pub const RECV: &str = "<--";
pub const SEND: &str = "-->";
pub const CMD: &str = "[CMD]";
pub const EVT: &str = "[EVT]";
pub const DOC: &str = "[DOC]";
pub const RPT: &str = "[RPT]";
pub const REQ: &str = "[REQ]";
pub const RES: &str = "[RES]";

static LOGGING_INITIALIZED: AtomicBool = AtomicBool::new(false);
static LOGGING_BYPASSED: AtomicBool = AtomicBool::new(false);
static LOGGING_REALTIME: AtomicBool = AtomicBool::new(true);
static LOGGING_COLORED: AtomicBool = AtomicBool::new(true);
static LOGGING_GUARDS_ACTIVE: AtomicU8 = AtomicU8::new(0);

/// Returns whether the core logger is enabled.
pub fn logging_is_initialized() -> bool {
    LOGGING_INITIALIZED.load(Ordering::Relaxed)
}

/// Sets the logging subsystem to bypass mode.
pub fn logging_set_bypass() {
    LOGGING_BYPASSED.store(true, Ordering::Relaxed);
}

/// Shuts down the logging subsystem.
pub fn logging_shutdown() {
    // Perform a graceful shutdown: prevent new logs, signal Close, drain and join.
    // Delegates to logger implementation which has access to the internals.
    crate::logging::logger::shutdown_graceful();
}

/// Returns whether the core logger is using ANSI colors.
pub fn logging_is_colored() -> bool {
    LOGGING_COLORED.load(Ordering::Relaxed)
}

/// Sets the global logging clock to real-time mode.
pub fn logging_clock_set_realtime_mode() {
    LOGGING_REALTIME.store(true, Ordering::Relaxed);
}

/// Sets the global logging clock to static mode.
pub fn logging_clock_set_static_mode() {
    LOGGING_REALTIME.store(false, Ordering::Relaxed);
}

/// Sets the global logging clock static time with the given UNIX timestamp (nanoseconds).
pub fn logging_clock_set_static_time(time_ns: u64) {
    let clock = get_atomic_clock_static();
    clock.set_time(time_ns.into());
}

/// Initialize tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level by passing
/// a configuration using the `RUST_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
///
/// # Errors
///
/// Returns an error if tracing subscriber fails to initialize.
pub fn init_tracing() -> anyhow::Result<()> {
    // Skip tracing initialization if `RUST_LOG` is not set
    if let Ok(v) = env::var("RUST_LOG") {
        let env_filter = EnvFilter::new(v.clone());

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to initialize tracing subscriber: {e}"))?;

        println!("Initialized tracing logs with RUST_LOG={v}");
    }
    Ok(())
}

/// Initialize logging.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the [nautilus_trader](https://pypi.org/project/nautilus_trader) package.
/// Logging can be configured to filter components and write up to a specific level only
/// by passing a configuration using the `NAUTILUS_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the `nautilus_trader` package.
/// Logging can be configured via the `NAUTILUS_LOG` environment variable.
///
/// # Errors
///
/// Returns an error if the logging subsystem fails to initialize.
pub fn init_logging(
    trader_id: TraderId,
    instance_id: UUID4,
    config: LoggerConfig,
    file_config: FileWriterConfig,
) -> anyhow::Result<LogGuard> {
    // Only set these after successful initialization
    let is_colored = config.is_colored;
    let guard = Logger::init_with_config(trader_id, instance_id, config, file_config)?;

    // Set flags only after successful initialization
    LOGGING_INITIALIZED.store(true, Ordering::Relaxed);
    LOGGING_COLORED.store(is_colored, Ordering::Relaxed);

    Ok(guard)
}

#[must_use]
pub const fn map_log_level_to_filter(log_level: LogLevel) -> LevelFilter {
    match log_level {
        LogLevel::Off => LevelFilter::Off,
        LogLevel::Trace => LevelFilter::Trace,
        LogLevel::Debug => LevelFilter::Debug,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Warning => LevelFilter::Warn,
        LogLevel::Error => LevelFilter::Error,
    }
}

/// Parses a string into a [`LevelFilter`].
///
/// # Errors
///
/// Returns an error if the provided string is not a valid `LevelFilter`.
pub fn parse_level_filter_str(s: &str) -> anyhow::Result<LevelFilter> {
    let mut log_level_str = s.to_string().to_uppercase();
    if log_level_str == "WARNING" {
        log_level_str = "WARN".to_string();
    }
    LevelFilter::from_str(&log_level_str)
        .map_err(|_| anyhow::anyhow!("Invalid log level string: '{s}'"))
}

/// Parses component-specific log levels from a JSON value map.
///
/// # Errors
///
/// Returns an error if a JSON value in the map is not a string or is not a valid log level.
pub fn parse_component_levels(
    original_map: Option<HashMap<String, serde_json::Value>>,
) -> anyhow::Result<HashMap<Ustr, LevelFilter>> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let s = value.as_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Component log level for '{key}' must be a string, got: {value}"
                    )
                })?;
                let lvl = parse_level_filter_str(s)?;
                new_map.insert(ustr_key, lvl);
            }
            Ok(new_map)
        }
        None => Ok(HashMap::new()),
    }
}

/// Logs that a task has started using `tracing::debug!`.
pub fn log_task_started(task_name: &str) {
    tracing::debug!("Started task '{task_name}'");
}

/// Logs that a task has stopped using `tracing::debug!`.
pub fn log_task_stopped(task_name: &str) {
    tracing::debug!("Stopped task '{task_name}'");
}

/// Logs that a task is being awaited using `tracing::debug!`.
pub fn log_task_awaiting(task_name: &str) {
    tracing::debug!("Awaiting task '{task_name}'");
}

/// Logs that a task was aborted using `tracing::debug!`.
pub fn log_task_aborted(task_name: &str) {
    tracing::debug!("Aborted task '{task_name}'");
}

/// Logs that there was an error in a task `tracing::error!`.
pub fn log_task_error(task_name: &str, e: &anyhow::Error) {
    tracing::error!("Error in task '{task_name}': {e}");
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("DEBUG", LevelFilter::Debug)]
    #[case("debug", LevelFilter::Debug)]
    #[case("Debug", LevelFilter::Debug)]
    #[case("DeBuG", LevelFilter::Debug)]
    #[case("INFO", LevelFilter::Info)]
    #[case("info", LevelFilter::Info)]
    #[case("WARNING", LevelFilter::Warn)]
    #[case("warning", LevelFilter::Warn)]
    #[case("WARN", LevelFilter::Warn)]
    #[case("warn", LevelFilter::Warn)]
    #[case("ERROR", LevelFilter::Error)]
    #[case("error", LevelFilter::Error)]
    #[case("OFF", LevelFilter::Off)]
    #[case("off", LevelFilter::Off)]
    #[case("TRACE", LevelFilter::Trace)]
    #[case("trace", LevelFilter::Trace)]
    fn test_parse_level_filter_str_case_insensitive(
        #[case] input: &str,
        #[case] expected: LevelFilter,
    ) {
        let result = parse_level_filter_str(input).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("INVALID")]
    #[case("DEBG")]
    #[case("WARNINGG")]
    #[case("")]
    #[case("INFO123")]
    fn test_parse_level_filter_str_invalid_returns_error(#[case] invalid_input: &str) {
        let result = parse_level_filter_str(invalid_input);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid log level")
        );
    }

    #[rstest]
    fn test_parse_component_levels_valid() {
        let mut map = HashMap::new();
        map.insert(
            "Strategy1".to_string(),
            serde_json::Value::String("DEBUG".to_string()),
        );
        map.insert(
            "Strategy2".to_string(),
            serde_json::Value::String("info".to_string()),
        );

        let result = parse_component_levels(Some(map)).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[&Ustr::from("Strategy1")], LevelFilter::Debug);
        assert_eq!(result[&Ustr::from("Strategy2")], LevelFilter::Info);
    }

    #[rstest]
    fn test_parse_component_levels_non_string_value_returns_error() {
        let mut map = HashMap::new();
        map.insert(
            "Strategy1".to_string(),
            serde_json::Value::Number(123.into()),
        );

        let result = parse_component_levels(Some(map));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be a string"));
    }

    #[rstest]
    fn test_parse_component_levels_invalid_level_returns_error() {
        let mut map = HashMap::new();
        map.insert(
            "Strategy1".to_string(),
            serde_json::Value::String("INVALID_LEVEL".to_string()),
        );

        let result = parse_component_levels(Some(map));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid log level")
        );
    }

    #[rstest]
    fn test_parse_component_levels_none_returns_empty() {
        let result = parse_component_levels(None).unwrap();
        assert_eq!(result.len(), 0);
    }
}

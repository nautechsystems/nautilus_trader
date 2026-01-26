// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod config;
pub mod headers;
pub mod logger;
pub mod macros;
pub mod writer;

#[cfg(feature = "tracing-bridge")]
pub mod bridge;

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use ahash::AHashMap;
use log::LevelFilter;
// Re-exports
pub use macros::{log_debug, log_error, log_info, log_trace, log_warn};
use nautilus_core::{UUID4, time::get_atomic_clock_static};
use nautilus_model::identifiers::TraderId;
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
static LAZY_GUARD: OnceLock<Option<LogGuard>> = OnceLock::new();

/// Returns whether the core logger is enabled.
pub fn logging_is_initialized() -> bool {
    LOGGING_INITIALIZED.load(Ordering::Relaxed)
}

/// Ensures logging is initialized on first use.
///
/// If `NAUTILUS_LOG` is set, initializes the logger with the specified config.
/// Otherwise, initializes with INFO level to stdout. This enables lazy
/// initialization for Rust-only binaries that don't go through the Python
/// kernel initialization.
///
/// Returns `true` if logging is available (either already initialized or
/// successfully lazy-initialized), `false` otherwise.
pub fn ensure_logging_initialized() -> bool {
    if LOGGING_INITIALIZED.load(Ordering::SeqCst) {
        return true;
    }

    LAZY_GUARD.get_or_init(|| {
        let config = env::var("NAUTILUS_LOG")
            .ok()
            .and_then(|spec| LoggerConfig::from_spec(&spec).ok())
            .unwrap_or_default();

        Logger::init_with_config(
            TraderId::default(),
            UUID4::default(),
            config,
            FileWriterConfig::default(),
        )
        .ok()
    });

    LOGGING_INITIALIZED.load(Ordering::SeqCst)
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
    Logger::init_with_config(trader_id, instance_id, config, file_config)
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
) -> anyhow::Result<AHashMap<Ustr, LevelFilter>> {
    match original_map {
        Some(map) => {
            let mut new_map = AHashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                let s = value.as_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Component log level for '{key}' must be a string, was: {value}"
                    )
                })?;
                let lvl = parse_level_filter_str(s)?;
                new_map.insert(ustr_key, lvl);
            }
            Ok(new_map)
        }
        None => Ok(AHashMap::new()),
    }
}

/// Logs that a task has started.
pub fn log_task_started(task_name: &str) {
    log::debug!("Started task '{task_name}'");
}

/// Logs that a task has stopped.
pub fn log_task_stopped(task_name: &str) {
    log::debug!("Stopped task '{task_name}'");
}

/// Logs that a task is being awaited.
pub fn log_task_awaiting(task_name: &str) {
    log::debug!("Awaiting task '{task_name}'");
}

/// Logs that a task was aborted.
pub fn log_task_aborted(task_name: &str) {
    log::debug!("Aborted task '{task_name}'");
}

/// Logs that there was an error in a task.
pub fn log_task_error(task_name: &str, e: &anyhow::Error) {
    log::error!("Error in task '{task_name}': {e}");
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

    #[rstest]
    fn test_logging_clock_set_static_mode() {
        logging_clock_set_static_mode();
        assert!(!LOGGING_REALTIME.load(Ordering::Relaxed));
    }

    #[rstest]
    fn test_logging_clock_set_realtime_mode() {
        logging_clock_set_realtime_mode();
        assert!(LOGGING_REALTIME.load(Ordering::Relaxed));
    }

    #[rstest]
    fn test_logging_clock_set_static_time() {
        let test_time: u64 = 1_700_000_000_000_000_000;
        logging_clock_set_static_time(test_time);
        let clock = get_atomic_clock_static();
        assert_eq!(clock.get_time_ns(), test_time);
    }

    #[rstest]
    fn test_logging_set_bypass() {
        logging_set_bypass();
        assert!(LOGGING_BYPASSED.load(Ordering::Relaxed));
    }

    #[rstest]
    fn test_map_log_level_to_filter() {
        assert_eq!(map_log_level_to_filter(LogLevel::Off), LevelFilter::Off);
        assert_eq!(map_log_level_to_filter(LogLevel::Trace), LevelFilter::Trace);
        assert_eq!(map_log_level_to_filter(LogLevel::Debug), LevelFilter::Debug);
        assert_eq!(map_log_level_to_filter(LogLevel::Info), LevelFilter::Info);
        assert_eq!(
            map_log_level_to_filter(LogLevel::Warning),
            LevelFilter::Warn
        );
        assert_eq!(map_log_level_to_filter(LogLevel::Error), LevelFilter::Error);
    }

    #[rstest]
    fn test_ensure_logging_initialized_returns_consistent_value() {
        // This test verifies ensure_logging_initialized() can be called safely.
        // Due to Once semantics, we can only test one code path per process.
        //
        // With nextest (process isolation per test):
        // - If NAUTILUS_LOG is unset, this returns false.
        // - If NAUTILUS_LOG is set externally, it may return true.
        //
        // The key invariant: multiple calls return the same value.
        let first_call = ensure_logging_initialized();
        let second_call = ensure_logging_initialized();

        assert_eq!(
            first_call, second_call,
            "ensure_logging_initialized must be idempotent"
        );
        assert_eq!(
            first_call,
            logging_is_initialized(),
            "ensure_logging_initialized return value must match logging_is_initialized()"
        );
    }

    #[rstest]
    fn test_ensure_logging_initialized_fast_path() {
        // If logging is already initialized, the fast path returns true immediately.
        // This test documents the expected behavior.
        if logging_is_initialized() {
            assert!(
                ensure_logging_initialized(),
                "Fast path should return true when already initialized"
            );
        }
        // If not initialized, we can't test the initialization path here
        // without side effects that affect other tests.
    }
}

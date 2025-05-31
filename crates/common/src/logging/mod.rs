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

pub mod headers;
pub mod logger;
pub mod writer;

use std::{
    collections::HashMap,
    env,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

use log::LevelFilter;
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

/// Returns whether the core logger is enabled.
pub fn logging_is_initialized() -> bool {
    LOGGING_INITIALIZED.load(Ordering::Relaxed)
}

/// Sets the logging system to bypass mode.
pub fn logging_set_bypass() {
    LOGGING_BYPASSED.store(true, Ordering::Relaxed);
}

/// Shuts down the logging system.
pub fn logging_shutdown() {
    // Flush any buffered logs and mark logging as uninitialized
    log::logger().flush();
    LOGGING_INITIALIZED.store(false, Ordering::Relaxed);
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
/// Initialize logging.
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
    LOGGING_INITIALIZED.store(true, Ordering::Relaxed);
    LOGGING_COLORED.store(config.is_colored, Ordering::Relaxed);
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
/// # Panics
///
/// Panics if the provided string is not a valid `LevelFilter`.
#[must_use]
pub fn parse_level_filter_str(s: &str) -> LevelFilter {
    let mut log_level_str = s.to_string().to_uppercase();
    if log_level_str == "WARNING" {
        log_level_str = "WARN".to_string();
    }
    LevelFilter::from_str(&log_level_str)
        .unwrap_or_else(|_| panic!("Invalid `LevelFilter` string, was {log_level_str}"))
}

#[must_use]
/// Parses component-specific log levels from a JSON value map.
///
/// # Panics
///
/// Panics if a JSON value in the map is not a string representing a log level.
pub fn parse_component_levels(
    original_map: Option<HashMap<String, serde_json::Value>>,
) -> HashMap<Ustr, LevelFilter> {
    match original_map {
        Some(map) => {
            let mut new_map = HashMap::new();
            for (key, value) in map {
                let ustr_key = Ustr::from(&key);
                // Expect the JSON value to be a string representing a log level
                let s = value
                    .as_str()
                    .expect("Invalid component log level: expected string");
                let lvl = parse_level_filter_str(s);
                new_map.insert(ustr_key, lvl);
            }
            new_map
        }
        None => HashMap::new(),
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

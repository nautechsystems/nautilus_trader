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
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

use log::LevelFilter;
use nautilus_core::{time::get_atomic_clock_static, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use tracing_subscriber::EnvFilter;
use ustr::Ustr;

use self::{
    logger::{LogGuard, Logger, LoggerConfig},
    writer::FileWriterConfig,
};
use crate::enums::LogLevel;

pub mod headers;
pub mod logger;
pub mod writer;

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
pub extern "C" fn logging_clock_set_realtime_mode() {
    LOGGING_REALTIME.store(true, Ordering::Relaxed);
}

/// Sets the global logging clock to static mode.
#[no_mangle]
pub extern "C" fn logging_clock_set_static_mode() {
    LOGGING_REALTIME.store(false, Ordering::Relaxed);
}

/// Sets the global logging clock static time with the given UNIX time (nanoseconds).
#[no_mangle]
pub extern "C" fn logging_clock_set_static_time(time_ns: u64) {
    let clock = get_atomic_clock_static();
    clock.set_time(time_ns);
}

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
) -> LogGuard {
    LOGGING_INITIALIZED.store(true, Ordering::Relaxed);
    LOGGING_COLORED.store(config.is_colored, Ordering::Relaxed);
    Logger::init_with_config(trader_id, instance_id, config, file_config)
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

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

use std::str::FromStr;

use pyo3::{prelude::*, types::PyDict};
use tracing::Level;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{fmt::Layer, prelude::*, EnvFilter, Registry};

/// Guards the log collector and flushes it when dropped
///
/// This struct must be dropped when the application has completed operation
/// it ensures that the any pending log lines are flushed before the application
/// closes.
#[pyclass]
pub struct LogGuard {
    #[allow(dead_code)]
    guards: Vec<WorkerGuard>,
}

/// Sets the global log collector
///
/// stdout_level: Set the level for the stdout writer
/// stderr_level: Set the level for the stderr writer
/// file_level: Set the level, the directory and the prefix for the file writer
///
/// It also configures a top level filter based on module/component name.
/// The format for the string is component1=info,component2=debug.
/// For e.g. network=error,kernel=info
///
/// # Safety
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
pub fn set_global_log_collector(
    stdout_level: Option<String>,
    stderr_level: Option<String>,
    file_level: Option<(String, String, String)>,
) -> LogGuard {
    let mut guards = Vec::new();
    let stdout_sub_builder = stdout_level.map(|stdout_level| {
        let stdout_level = Level::from_str(&stdout_level).unwrap();
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        guards.push(guard);
        Layer::default().with_writer(non_blocking.with_max_level(stdout_level))
    });
    let stderr_sub_builder = stderr_level.map(|stderr_level| {
        let stderr_level = Level::from_str(&stderr_level).unwrap();
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        guards.push(guard);
        Layer::default().with_writer(non_blocking.with_max_level(stderr_level))
    });
    let file_sub_builder = file_level.map(|(dir_path, file_prefix, file_level)| {
        let file_level = Level::from_str(&file_level).unwrap();
        let rolling_log = RollingFileAppender::new(Rotation::NEVER, dir_path, file_prefix);
        let (non_blocking, guard) = tracing_appender::non_blocking(rolling_log);
        guards.push(guard);
        Layer::default()
            .with_ansi(false) // turn off unicode colors when writing to file
            .with_writer(non_blocking.with_max_level(file_level))
    });

    if let Err(err) = Registry::default()
        .with(stderr_sub_builder)
        .with(stdout_sub_builder)
        .with(file_sub_builder)
        .with(EnvFilter::from_default_env())
        .try_init()
    {
        println!(
            "Failed to set global default dispatcher because of error: {}",
            err
        );
    };

    LogGuard { guards }
}

/// Need to modify sys modules so that submodule can be loaded directly as
/// import supermodule.submodule
///
/// refer: https://github.com/PyO3/pyo3/issues/2644
#[pymodule]
fn nautilus_pyo3(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let sys = PyModule::import(py, "sys")?;
    let sys_modules: &PyDict = sys.getattr("modules")?.downcast()?;

    // Indicators
    let submodule = pyo3::wrap_pymodule!(nautilus_indicators::indicators);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(
        "nautilus_trader.core.nautilus_pyo3.indicators",
        m.getattr("indicators")?,
    )?;

    // Model
    let submodule = pyo3::wrap_pymodule!(nautilus_model::model);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(
        "nautilus_trader.core.nautilus_pyo3.model",
        m.getattr("model")?,
    )?;

    // Network
    let submodule = pyo3::wrap_pymodule!(nautilus_network::network);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(
        "nautilus_trader.core.nautilus_pyo3.network",
        m.getattr("network")?,
    )?;

    // Persistence
    let submodule = pyo3::wrap_pymodule!(nautilus_persistence::persistence);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(
        "nautilus_trader.core.nautilus_pyo3.persistence",
        m.getattr("persistence")?,
    )?;

    m.add_class::<LogGuard>()?;
    m.add_function(wrap_pyfunction!(set_global_log_collector, m)?)?;

    Ok(())
}

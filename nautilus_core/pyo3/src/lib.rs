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

use pyo3::{
    prelude::*,
    types::{PyDict, PyString},
};
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
#[must_use]
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

    if let Err(e) = Registry::default()
        .with(stderr_sub_builder)
        .with(stdout_sub_builder)
        .with(file_sub_builder)
        .with(EnvFilter::from_default_env())
        .try_init()
    {
        println!("Failed to set global default dispatcher because of error: {e}");
    };

    LogGuard { guards }
}

/// Need to modify sys modules so that submodule can be loaded directly as
/// import supermodule.submodule
///
/// Also re-exports all submodule attributes so they can be imported directly from `nautilus_pyo3`.
/// refer: https://github.com/PyO3/pyo3/issues/2644
#[pymodule]
fn nautilus_pyo3(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let sys = PyModule::import(py, "sys")?;
    let sys_modules: &PyDict = sys.getattr("modules")?.downcast()?;
    let module_name = "nautilus_trader.core.nautilus_pyo3";

    // Set pyo3_nautilus to be recognized as a subpackage
    sys_modules.set_item(module_name, m)?;

    m.add_class::<LogGuard>()?;
    m.add_function(wrap_pyfunction!(set_global_log_collector, m)?)?;

    // Core
    let n = "core";
    let submodule = pyo3::wrap_pymodule!(nautilus_core::python::core);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // TODO: Currently experiencing the following issue when trying to add `common`
    // error[E0631]: type mismatch in closure arguments
    // = note: expected closure signature `fn(pyo3::Python<'_>) -> _`
    //         found closure signature `fn(pyo3::marker::Python<'_>) -> _`

    // Common
    // let n = "common";
    // let submodule = pyo3::wrap_pymodule!(nautilus_common::python::common);
    // m.add_wrapped(submodule)?;
    // sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    // re_export_module_attributes(m, n)?;

    // Model
    let n = "model";
    let submodule = pyo3::wrap_pymodule!(nautilus_model::python::model);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // Indicators
    let n = "indicators";
    let submodule = pyo3::wrap_pymodule!(nautilus_indicators::python::indicators);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // Infrastructure
    let n = "infrastructure";
    let submodule = pyo3::wrap_pymodule!(nautilus_infrastructure::python::infrastructure);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // Network
    let n = "network";
    let submodule = pyo3::wrap_pymodule!(nautilus_network::python::network);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // Persistence
    let n = "persistence";
    let submodule = pyo3::wrap_pymodule!(nautilus_persistence::python::persistence);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    Ok(())
}

fn re_export_module_attributes(parent_module: &PyModule, submodule_name: &str) -> PyResult<()> {
    let submodule = parent_module.getattr(submodule_name)?;
    for item in submodule.dir() {
        let item_name: &PyString = item.extract()?;
        if let Ok(attr) = submodule.getattr(item_name) {
            parent_module.add(item_name.to_str()?, attr)?;
        }
    }

    Ok(())
}

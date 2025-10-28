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

//! Python bindings aggregator crate for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-pyo3` crate collects the Python bindings generated across the NautilusTrader workspace
//! and re-exports them through a single shared library that can be included in binary wheels.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
//! highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.
//!
//! # Feature flags
//!
//! This crate is primarily intended to be built for Python via
//! [maturin](https://github.com/PyO3/maturin) and therefore provides a broad set of feature flags
//! to toggle bindings and optional dependencies:
//!
//! - `extension-module`: Builds the crate as a Python extension module (automatically enabled by `maturin`).
//! - `ffi`: Enables the C foreign function interface (FFI) support in dependent crates.
//! - `high-precision`: Uses 128-bit value types throughout the workspace.
//! - `cython-compat`: Adjusts the module name so it can be imported from Cython generated code.
//! - `postgres`: Enables PostgreSQL (sqlx) back-ends in dependent crates.
//! - `redis`: Enables Redis based infrastructure in dependent crates.
//! - `hypersync`: Enables hypersync support (fast parallel hash maps) where available.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::{path::Path, time::Duration};

use pyo3::{prelude::*, pyfunction};

const RUNTIME_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

#[pyfunction]
fn _shutdown_nautilus_runtime() -> PyResult<()> {
    nautilus_common::runtime::shutdown_runtime(Duration::from_secs(RUNTIME_SHUTDOWN_TIMEOUT_SECS));
    Ok(())
}

/// We modify sys modules so that submodule can be loaded directly as
/// import supermodule.submodule
///
/// Also re-exports all submodule attributes so they can be imported directly from `nautilus_pyo3`
/// refer: <https://github.com/PyO3/pyo3/issues/2644>
#[pymodule] // The name of the function must match `lib.name` in `Cargo.toml`
#[cfg_attr(feature = "cython-compat", pyo3(name = "nautilus_pyo3"))]
fn _libnautilus(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    let sys = PyModule::import(py, "sys")?;
    let modules = sys.getattr("modules")?;
    let sys_modules: &Bound<'_, PyAny> = modules.downcast()?;

    #[cfg(feature = "cython-compat")]
    let module_name = "nautilus_trader.core.nautilus_pyo3";

    #[cfg(not(feature = "cython-compat"))]
    let module_name = "nautilus_trader._libnautilus";

    // Set pyo3_nautilus to be recognized as a subpackage
    sys_modules.set_item(module_name, m)?;

    let n = "analysis";
    let submodule = pyo3::wrap_pymodule!(nautilus_analysis::python::analysis);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "core";
    let submodule = pyo3::wrap_pymodule!(nautilus_core::python::core);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "common";
    let submodule = pyo3::wrap_pymodule!(nautilus_common::python::common);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "cryptography";
    let submodule = pyo3::wrap_pymodule!(nautilus_cryptography::python::cryptography);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "indicators";
    let submodule = pyo3::wrap_pymodule!(nautilus_indicators::python::indicators);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "infrastructure";
    let submodule = pyo3::wrap_pymodule!(nautilus_infrastructure::python::infrastructure);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "live";
    let submodule = pyo3::wrap_pymodule!(nautilus_live::python::live);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "model";
    let submodule = pyo3::wrap_pymodule!(nautilus_model::python::model);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "network";
    let submodule = pyo3::wrap_pymodule!(nautilus_network::python::network);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "persistence";
    let submodule = pyo3::wrap_pymodule!(nautilus_persistence::python::persistence);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "serialization";
    let submodule = pyo3::wrap_pymodule!(nautilus_serialization::python::serialization);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "testkit";
    let submodule = pyo3::wrap_pymodule!(nautilus_testkit::python::testkit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "trading";
    let submodule = pyo3::wrap_pymodule!(nautilus_trading::python::trading);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    ////////////////////////////////////////////////////////////////////////////////
    // Adapters
    ////////////////////////////////////////////////////////////////////////////////

    let n = "bitmex";
    let submodule = pyo3::wrap_pymodule!(nautilus_bitmex::python::bitmex);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "bybit";
    let submodule = pyo3::wrap_pymodule!(nautilus_bybit::python::bybit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "coinbase_intx";
    let submodule = pyo3::wrap_pymodule!(nautilus_coinbase_intx::python::coinbase_intx);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "databento";
    let submodule = pyo3::wrap_pymodule!(nautilus_databento::python::databento);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "hyperliquid";
    let submodule = pyo3::wrap_pymodule!(nautilus_hyperliquid::python::hyperliquid);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "okx";
    let submodule = pyo3::wrap_pymodule!(nautilus_okx::python::okx);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    let n = "tardis";
    let submodule = pyo3::wrap_pymodule!(nautilus_tardis::python::tardis);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;

    #[cfg(feature = "defi")]
    {
        let n = "blockchain";
        let submodule = pyo3::wrap_pymodule!(nautilus_blockchain::python::blockchain);
        m.add_wrapped(submodule)?;
        sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
        #[cfg(feature = "cython-compat")]
        re_export_module_attributes(m, n)?;
    }

    // Register a lightweight shutdown hook so the interpreter waits for the Tokio
    // runtime to yield once before `Py_Finalize` tears it down.
    m.add_function(pyo3::wrap_pyfunction!(_shutdown_nautilus_runtime, m)?)?;
    let shutdown_callable = m.getattr("_shutdown_nautilus_runtime")?;
    let atexit = PyModule::import(py, "atexit")?;
    atexit.call_method1("register", (shutdown_callable,))?;

    Ok(())
}

#[cfg(feature = "cython-compat")]
fn re_export_module_attributes(
    parent_module: &Bound<'_, PyModule>,
    submodule_name: &str,
) -> PyResult<()> {
    let submodule = parent_module.getattr(submodule_name)?;
    for item_name in submodule.dir()? {
        let item_name_str: &str = item_name.extract()?;
        if let Ok(attr) = submodule.getattr(item_name_str) {
            parent_module.add(item_name_str, attr)?;
        }
    }

    Ok(())
}

/// Generate Python type stub info for PyO3 bindings.
///
/// Assumes the pyproject.toml is located in the python/ directory relative to the workspace root.
///
/// # Panics
///
/// Panics if the path locating the pyproject.toml is incorrect.
///
/// # Errors
///
/// Returns an error if stub information generation fails.
///
/// # Reference
///
/// - <https://pyo3.rs/latest/python-typing-hints>
/// - <https://crates.io/crates/pyo3-stub-gen>
/// - <https://github.com/Jij-Inc/pyo3-stub-gen>
pub fn stub_info() -> pyo3_stub_gen::Result<pyo3_stub_gen::StubInfo> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let pyproject_path = workspace_root.join("python").join("pyproject.toml");

    pyo3_stub_gen::StubInfo::from_pyproject_toml(&pyproject_path)
}

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

//! Python bindings aggregator crate for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-pyo3` crate collects the Python bindings generated across the NautilusTrader workspace
//! and re-exports them through a single shared library that can be included in binary wheels.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
//! engine for multi-asset, multi-venue trading systems.
//!
//! The system spans research, deterministic simulation, and live execution within a single
//! event-driven architecture, providing research-to-live semantic parity.
//!
//! # Feature Flags
//!
//! This crate is primarily intended to be built for Python via
//! [maturin](https://github.com/PyO3/maturin) and therefore provides a broad set of feature flags
//! to toggle bindings and optional dependencies:
//!
//! - `arrow`: Enables Apache Arrow support in dependent crates.
//! - `betfair`: Enables the Betfair adapter and its Python bindings.
//! - `defi`: Enables DeFi (Decentralized Finance) support, including blockchain adapters.
//! - `extension-module`: Builds as a Python extension module and is automatically enabled by
//!   `maturin`.
//! - `high-precision`: Uses 128-bit value types throughout the workspace.
//! - `hypersync`: Enables [`hypersync-client`](https://crates.io/crates/hypersync-client)
//!   support for the blockchain adapter.
//! - `mimalloc`: Sets [mimalloc](https://crates.io/crates/mimalloc) as Rust's global allocator.
//! - `postgres`: Enables PostgreSQL (sqlx) back-ends in dependent crates.
//! - `redis`: Enables Redis based infrastructure in dependent crates.
//! - `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::{path::Path, time::Duration};

#[cfg(feature = "mimalloc")]
use mimalloc::MiMalloc;
use nautilus_common::live::runtime::shutdown_runtime;
use nautilus_system::{config::StreamingConfig, python::controller::PyController};
use pyo3::{prelude::*, pyfunction};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const RUNTIME_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

#[pyfunction]
fn _shutdown_nautilus_runtime() {
    shutdown_runtime(Duration::from_secs(RUNTIME_SHUTDOWN_TIMEOUT_SECS));
}

/// Adds each wrapped module to `sys.modules` so Python can import it as a submodule.
///
/// See <https://github.com/PyO3/pyo3/issues/2644>.
#[pymodule] // The name of the function must match `lib.name` in `Cargo.toml`
fn _libnautilus(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(feature = "mimalloc")]
    nautilus_common::logging::headers::register_allocator_mimalloc();

    let sys = PyModule::import(py, "sys")?;
    let modules = sys.getattr("modules")?;
    let sys_modules: &Bound<'_, PyAny> = modules.cast()?;

    let module_name = "nautilus_trader._libnautilus";

    // Set pyo3_nautilus to be recognized as a subpackage
    sys_modules.set_item(module_name, m)?;

    // nautilus-import-ok: wrap_pymodule! requires fully qualified paths
    let n = "analysis";
    let submodule = pyo3::wrap_pymodule!(nautilus_analysis::python::analysis);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "core";
    let submodule = pyo3::wrap_pymodule!(nautilus_core::python::core);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "common";
    let submodule = pyo3::wrap_pymodule!(nautilus_common::python::common);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "cryptography";
    let submodule = pyo3::wrap_pymodule!(nautilus_cryptography::python::cryptography);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "data";
    let submodule = pyo3::wrap_pymodule!(nautilus_data::python::data);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "execution";
    let submodule = pyo3::wrap_pymodule!(nautilus_execution::python::execution);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "indicators";
    let submodule = pyo3::wrap_pymodule!(nautilus_indicators::python::indicators);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "infrastructure";
    let submodule = pyo3::wrap_pymodule!(nautilus_infrastructure::python::infrastructure);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "live";
    let submodule = pyo3::wrap_pymodule!(nautilus_live::python::live);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "model";
    let submodule = pyo3::wrap_pymodule!(nautilus_model::python::model);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "network";
    let submodule = pyo3::wrap_pymodule!(nautilus_network::python::network);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "persistence";
    let submodule = pyo3::wrap_pymodule!(nautilus_persistence::python::persistence);
    m.add_wrapped(submodule)?;
    m.getattr(n)?
        .cast::<PyModule>()?
        .add_class::<StreamingConfig>()?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "portfolio";
    let submodule = pyo3::wrap_pymodule!(nautilus_portfolio::python::portfolio);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "risk";
    let submodule = pyo3::wrap_pymodule!(nautilus_risk::python::risk);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "serialization";
    let submodule = pyo3::wrap_pymodule!(nautilus_serialization::python::serialization);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "testkit";
    let submodule = pyo3::wrap_pymodule!(nautilus_testkit::python::testkit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "trading";
    let submodule = pyo3::wrap_pymodule!(nautilus_trading::python::trading);
    m.add_wrapped(submodule)?;

    // `Controller` drives the trader, so it lives in nautilus-system which depends on
    // nautilus-trading and therefore cannot register itself from the trading module
    m.getattr(n)?
        .cast::<PyModule>()?
        .add_class::<PyController>()?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "backtest";
    let submodule = pyo3::wrap_pymodule!(nautilus_backtest::python::backtest);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    ////////////////////////////////////////////////////////////////////////////////
    // Adapters
    ////////////////////////////////////////////////////////////////////////////////

    let n = "architect_ax";
    let submodule = pyo3::wrap_pymodule!(nautilus_architect_ax::python::architect_ax);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    #[cfg(feature = "betfair")]
    {
        let n = "betfair";
        let submodule = pyo3::wrap_pymodule!(nautilus_betfair::python::betfair);
        m.add_wrapped(submodule)?;
        sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    }

    let n = "binance";
    let submodule = pyo3::wrap_pymodule!(nautilus_binance::python::binance);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "bitmex";
    let submodule = pyo3::wrap_pymodule!(nautilus_bitmex::python::bitmex);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "bybit";
    let submodule = pyo3::wrap_pymodule!(nautilus_bybit::python::bybit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "coinbase";
    let submodule = pyo3::wrap_pymodule!(nautilus_coinbase::python::coinbase);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "databento";
    let submodule = pyo3::wrap_pymodule!(nautilus_databento::python::databento);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "deribit";
    let submodule = pyo3::wrap_pymodule!(nautilus_deribit::python::deribit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "derive";
    let submodule = pyo3::wrap_pymodule!(nautilus_derive::python::derive);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "dydx";
    let submodule = pyo3::wrap_pymodule!(nautilus_dydx::python::dydx);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "hyperliquid";
    let submodule = pyo3::wrap_pymodule!(nautilus_hyperliquid::python::hyperliquid);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "kraken";
    let submodule = pyo3::wrap_pymodule!(nautilus_kraken::python::kraken);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "lighter";
    let submodule = pyo3::wrap_pymodule!(nautilus_lighter::python::lighter);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "interactive_brokers";
    let submodule = pyo3::wrap_pymodule!(nautilus_interactive_brokers::python::interactive_brokers);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "okx";
    let submodule = pyo3::wrap_pymodule!(nautilus_okx::python::okx);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "polymarket";
    let submodule = pyo3::wrap_pymodule!(nautilus_polymarket::python::polymarket);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "sandbox";
    let submodule = pyo3::wrap_pymodule!(nautilus_sandbox::python::sandbox);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    let n = "tardis";
    let submodule = pyo3::wrap_pymodule!(nautilus_tardis::python::tardis);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;

    #[cfg(feature = "defi")]
    {
        // nautilus-import-ok: wrap_pymodule! requires fully qualified paths
        let n = "blockchain";
        let submodule = pyo3::wrap_pymodule!(nautilus_blockchain::python::blockchain);
        m.add_wrapped(submodule)?;
        sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    }

    // Register a lightweight shutdown hook so the interpreter waits for the Tokio
    // runtime to yield once before `Py_Finalize` tears it down.
    m.add_function(pyo3::wrap_pyfunction!(_shutdown_nautilus_runtime, m)?)?;
    let shutdown_callable = m.getattr("_shutdown_nautilus_runtime")?;
    let atexit = PyModule::import(py, "atexit")?;
    atexit.call_method1("register", (shutdown_callable,))?;

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

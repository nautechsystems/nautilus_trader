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

//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(clippy::missing_errors_doc)]

use pyo3::prelude::*;

/// We modify sys modules so that submodule can be loaded directly as
/// import supermodule.submodule
///
/// Also re-exports all submodule attributes so they can be imported directly from `nautilus_pyo3`
/// refer: https://github.com/PyO3/pyo3/issues/2644
#[pymodule]
fn nautilus_pyo3(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    let sys = PyModule::import(py, "sys")?;
    let modules = sys.getattr("modules")?;
    let sys_modules: &Bound<'_, PyAny> = modules.downcast()?;
    let module_name = "nautilus_trader.core.nautilus_pyo3";

    // Set pyo3_nautilus to be recognized as a subpackage
    sys_modules.set_item(module_name, m)?;

    let n = "core";
    let submodule = pyo3::wrap_pymodule!(nautilus_core::python::core);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "common";
    let submodule = pyo3::wrap_pymodule!(nautilus_common::python::common);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "cryptography";
    let submodule = pyo3::wrap_pymodule!(nautilus_cryptography::python::cryptography);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "execution";
    let submodule = pyo3::wrap_pymodule!(nautilus_execution::python::execution);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "model";
    let submodule = pyo3::wrap_pymodule!(nautilus_model::python::model);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "indicators";
    let submodule = pyo3::wrap_pymodule!(nautilus_indicators::python::indicators);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "infrastructure";
    let submodule = pyo3::wrap_pymodule!(nautilus_infrastructure::python::infrastructure);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "network";
    let submodule = pyo3::wrap_pymodule!(nautilus_network::python::network);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "persistence";
    let submodule = pyo3::wrap_pymodule!(nautilus_persistence::python::persistence);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "serialization";
    let submodule = pyo3::wrap_pymodule!(nautilus_serialization::python::serialization);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "test_kit";
    let submodule = pyo3::wrap_pymodule!(nautilus_test_kit::python::test_kit);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "trading";
    let submodule = pyo3::wrap_pymodule!(nautilus_trading::python::trading);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    // Adapters

    let n = "coinbase_intx";
    let submodule = pyo3::wrap_pymodule!(nautilus_coinbase_intx::python::coinbase_intx);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "databento";
    let submodule = pyo3::wrap_pymodule!(nautilus_databento::python::databento);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    let n = "tardis";
    let submodule = pyo3::wrap_pymodule!(nautilus_tardis::python::tardis);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    re_export_module_attributes(m, n)?;

    Ok(())
}

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

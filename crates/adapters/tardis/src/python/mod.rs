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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod config;
pub mod csv;
pub mod enums;
pub mod http;
pub mod machine;

use pyo3::prelude::*;

/// Loaded as nautilus_pyo3.tardis
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn tardis(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<super::machine::types::InstrumentMiniInfo>()?;
    m.add_class::<super::machine::types::ReplayNormalizedRequestOptions>()?;
    m.add_class::<super::machine::types::StreamNormalizedRequestOptions>()?;
    m.add_class::<super::machine::TardisMachineClient>()?;
    m.add_class::<super::http::client::TardisHttpClient>()?;
    m.add_function(wrap_pyfunction!(
        enums::py_tardis_exchange_from_venue_str,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        config::py_bar_spec_to_tardis_trade_bar_string,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(machine::py_run_tardis_machine_replay, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_tardis_deltas, m)?)?;
    m.add_function(wrap_pyfunction!(
        csv::py_load_tardis_depth10_from_snapshot5,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        csv::py_load_tardis_depth10_from_snapshot25,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_tardis_quotes, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_tardis_trades, m)?)?;

    Ok(())
}

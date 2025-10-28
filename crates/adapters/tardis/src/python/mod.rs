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

use nautilus_core::python::enums::parse_enum;
use pyo3::prelude::*;
use ustr::Ustr;

use super::enums::{TardisExchange, TardisInstrumentType};
use crate::parse::normalize_symbol_str;

/// Normalize a symbol string for Tardis, returning a suffix-modified symbol.
///
/// # Errors
///
/// Returns a `PyErr` if the `exchange` or `instrument_type` cannot be parsed.
#[pyfunction(name = "tardis_normalize_symbol_str")]
#[pyo3(signature = (symbol, exchange, instrument_type, is_inverse=None))]
pub fn py_tardis_normalize_symbol_str(
    symbol: String,
    exchange: String,
    instrument_type: String,
    is_inverse: Option<bool>,
) -> PyResult<String> {
    let symbol = Ustr::from(&symbol);
    let exchange: TardisExchange = parse_enum(&exchange, stringify!(exchange))?;
    let instrument_type: TardisInstrumentType =
        parse_enum(&instrument_type, stringify!(instrument_type))?;

    Ok(normalize_symbol_str(symbol, &exchange, &instrument_type, is_inverse).to_string())
}

/// Loaded as `nautilus_pyo3.tardis`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn tardis(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<super::machine::types::TardisInstrumentMiniInfo>()?;
    m.add_class::<super::machine::types::ReplayNormalizedRequestOptions>()?;
    m.add_class::<super::machine::types::StreamNormalizedRequestOptions>()?;
    m.add_class::<super::machine::TardisMachineClient>()?;
    m.add_class::<super::http::client::TardisHttpClient>()?;
    m.add_function(wrap_pyfunction!(py_tardis_normalize_symbol_str, m)?)?;
    m.add_function(wrap_pyfunction!(
        enums::py_tardis_exchange_from_venue_str,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(enums::py_tardis_exchange_to_venue_str, m)?)?;
    m.add_function(wrap_pyfunction!(
        enums::py_tardis_exchange_is_option_exchange,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(enums::py_tardis_exchanges, m)?)?;
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
    m.add_function(wrap_pyfunction!(csv::py_stream_tardis_deltas, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_tardis_batched_deltas, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_tardis_quotes, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_tardis_trades, m)?)?;
    m.add_function(wrap_pyfunction!(
        csv::py_stream_tardis_depth10_from_snapshot5,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        csv::py_stream_tardis_depth10_from_snapshot25,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_tardis_funding_rates, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_tardis_funding_rates, m)?)?;

    Ok(())
}

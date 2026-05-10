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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{data::BarSpecification, identifiers::InstrumentId};
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{
    common::{enums::TardisExchange, parse::bar_spec_to_tardis_trade_bar_string},
    config::TardisDataClientConfig,
    machine::types::{
        ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions, TardisInstrumentMiniInfo,
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl TardisInstrumentMiniInfo {
    /// Instrument definition information necessary for stream parsing.
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        raw_symbol: &str,
        exchange: &str,
        price_precision: u8,
        size_precision: u8,
    ) -> Self {
        let exchange: TardisExchange = exchange
            .parse()
            .expect("`exchange` should be Tardis convention");
        Self::new(
            instrument_id,
            Some(Ustr::from(raw_symbol)),
            exchange,
            price_precision,
            size_precision,
        )
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    const fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "raw_symbol")]
    fn py_raw_symbol(&self) -> String {
        self.raw_symbol.to_string()
    }

    #[getter]
    #[pyo3(name = "exchange")]
    fn py_exchange(&self) -> String {
        self.exchange.to_string()
    }

    #[getter]
    #[pyo3(name = "price_precision")]
    const fn py_price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    #[pyo3(name = "size_precision")]
    const fn py_size_precision(&self) -> u8 {
        self.size_precision
    }
}

/// Converts a Nautilus `BarSpecification` to the Tardis trade bar string convention.
///
/// # Errors
///
/// Returns an error if the bar aggregation kind is unsupported.
#[pyfunction(name = "bar_spec_to_tardis_trade_bar_string")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.tardis")]
pub fn py_bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> PyResult<String> {
    bar_spec_to_tardis_trade_bar_string(bar_spec).map_err(to_pyvalue_err)
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl TardisDataClientConfig {
    /// Configuration for the Tardis data client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        tardis_ws_url = None,
        proxy_url = None,
        normalize_symbols = None,
        options = None,
        stream_options = None,
    ))]
    fn py_new(
        api_key: Option<String>,
        tardis_ws_url: Option<String>,
        proxy_url: Option<String>,
        normalize_symbols: Option<bool>,
        options: Option<Vec<ReplayNormalizedRequestOptions>>,
        stream_options: Option<Vec<StreamNormalizedRequestOptions>>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            tardis_ws_url,
            proxy_url,
            normalize_symbols: normalize_symbols.unwrap_or(defaults.normalize_symbols),
            book_snapshot_output: defaults.book_snapshot_output,
            options: options.unwrap_or_default(),
            stream_options: stream_options.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

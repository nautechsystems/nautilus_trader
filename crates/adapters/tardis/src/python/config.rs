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

use nautilus_model::{data::BarSpecification, identifiers::InstrumentId};
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{
    enums::Exchange, machine::types::InstrumentMiniInfo, parse::bar_spec_to_tardis_trade_bar_string,
};

#[pymethods]
impl InstrumentMiniInfo {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        raw_symbol: String,
        exchange: String,
        price_precision: u8,
        size_precision: u8,
    ) -> PyResult<Self> {
        let exchange: Exchange = exchange
            .parse()
            .expect("`exchange` should be Tardis convention");
        Ok(Self::new(
            instrument_id,
            Some(Ustr::from(&raw_symbol)),
            exchange,
            price_precision,
            size_precision,
        ))
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

#[must_use]
#[pyfunction(name = "bar_spec_to_tardis_trade_bar_string")]
pub fn py_bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> String {
    bar_spec_to_tardis_trade_bar_string(bar_spec)
}

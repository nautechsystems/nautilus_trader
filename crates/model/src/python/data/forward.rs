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

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{data::forward::ForwardPrice, identifiers::InstrumentId};

#[pymethods]
impl ForwardPrice {
    #[new]
    #[pyo3(signature = (instrument_id, forward_price, underlying_index=None, ts_event=0, ts_init=0))]
    fn py_new(
        instrument_id: InstrumentId,
        forward_price: String,
        underlying_index: Option<String>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        let price = forward_price.parse::<Decimal>().map_err(to_pyvalue_err)?;
        Ok(Self {
            instrument_id,
            forward_price: price,
            underlying_index,
            ts_event: UnixNanos::from(ts_event),
            ts_init: UnixNanos::from(ts_init),
        })
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "forward_price")]
    fn py_forward_price(&self) -> String {
        self.forward_price.to_string()
    }

    #[getter]
    #[pyo3(name = "underlying_index")]
    fn py_underlying_index(&self) -> Option<String> {
        self.underlying_index.clone()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    fn __repr__(&self) -> String {
        format!(
            "ForwardPrice({}, price={}, index={:?})",
            self.instrument_id, self.forward_price, self.underlying_index
        )
    }

    fn __str__(&self) -> String {
        format!(
            "ForwardPrice({}, {})",
            self.instrument_id, self.forward_price
        )
    }
}

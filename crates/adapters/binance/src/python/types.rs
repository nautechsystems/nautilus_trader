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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3};
use nautilus_model::{
    data::bar::BarType,
    types::{Price, Quantity},
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::Decimal;

use crate::common::bar::BinanceBar;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceBar {
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.bar_type.hash(&mut hasher);
        self.ts_event.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(bar_type={}, open={}, high={}, low={}, close={}, volume={}, quote_volume={}, count={}, taker_buy_base_volume={}, taker_buy_quote_volume={}, ts_event={}, ts_init={})",
            stringify!(BinanceBar),
            self.bar_type,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.quote_volume,
            self.count,
            self.taker_buy_base_volume,
            self.taker_buy_quote_volume,
            self.ts_event,
            self.ts_init,
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    #[getter]
    #[pyo3(name = "bar_type")]
    fn py_bar_type(&self) -> BarType {
        self.bar_type
    }

    #[getter]
    #[pyo3(name = "open")]
    const fn py_open(&self) -> Price {
        self.open
    }

    #[getter]
    #[pyo3(name = "high")]
    const fn py_high(&self) -> Price {
        self.high
    }

    #[getter]
    #[pyo3(name = "low")]
    const fn py_low(&self) -> Price {
        self.low
    }

    #[getter]
    #[pyo3(name = "close")]
    const fn py_close(&self) -> Price {
        self.close
    }

    #[getter]
    #[pyo3(name = "volume")]
    const fn py_volume(&self) -> Quantity {
        self.volume
    }

    #[getter]
    #[pyo3(name = "quote_volume")]
    fn py_quote_volume(&self) -> Decimal {
        self.quote_volume
    }

    #[getter]
    #[pyo3(name = "count")]
    const fn py_count(&self) -> u64 {
        self.count
    }

    #[getter]
    #[pyo3(name = "taker_buy_base_volume")]
    fn py_taker_buy_base_volume(&self) -> Decimal {
        self.taker_buy_base_volume
    }

    #[getter]
    #[pyo3(name = "taker_buy_quote_volume")]
    fn py_taker_buy_quote_volume(&self) -> Decimal {
        self.taker_buy_quote_volume
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    const fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    /// # Errors
    ///
    /// Returns a `PyErr` if generating the Python dictionary fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(BinanceBar))?;
        dict.set_item("bar_type", self.bar_type.to_string())?;
        dict.set_item("open", self.open.to_string())?;
        dict.set_item("high", self.high.to_string())?;
        dict.set_item("low", self.low.to_string())?;
        dict.set_item("close", self.close.to_string())?;
        dict.set_item("volume", self.volume.to_string())?;
        dict.set_item("quote_volume", self.quote_volume.to_string())?;
        dict.set_item("count", self.count)?;
        dict.set_item(
            "taker_buy_base_volume",
            self.taker_buy_base_volume.to_string(),
        )?;
        dict.set_item(
            "taker_buy_quote_volume",
            self.taker_buy_quote_volume.to_string(),
        )?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.into())
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    time::UnixNanos,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use ustr::Ustr;

use crate::{
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    instruments::equity::Equity,
    types::{currency::Currency, price::Price, quantity::Quantity},
};

#[pymethods]
impl Equity {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        id: InstrumentId,
        raw_symbol: Symbol,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        isin: Option<String>,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> PyResult<Self> {
        Self::new(
            id,
            raw_symbol,
            isin.map(|x| Ustr::from(&x)),
            currency,
            price_precision,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .map_err(to_pyvalue_err)
    }

    #[getter]
    fn instrument_type(&self) -> &str {
        "Equity"
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            _ => panic!("Not implemented"),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    #[getter]
    fn isin(&self) -> Option<&str> {
        match self.isin {
            Some(isin) => Some(isin.as_str()),
            None => None,
        }
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(Equity))?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("raw_symbol", self.raw_symbol.to_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("price_precision", self.price_precision)?;
        dict.set_item("price_increment", self.price_increment.to_string())?;
        dict.set_item("ts_event", self.ts_event)?;
        dict.set_item("ts_init", self.ts_init)?;
        match &self.isin {
            Some(value) => dict.set_item("isin", value.to_string())?,
            None => dict.set_item("isin", py.None())?,
        }
        match self.lot_size {
            Some(value) => dict.set_item("lot_size", value.to_string())?,
            None => dict.set_item("lot_size", py.None())?,
        }
        match self.max_quantity {
            Some(value) => dict.set_item("max_quantity", value.to_string())?,
            None => dict.set_item("max_quantity", py.None())?,
        }
        match self.min_quantity {
            Some(value) => dict.set_item("min_quantity", value.to_string())?,
            None => dict.set_item("min_quantity", py.None())?,
        }
        match self.max_price {
            Some(value) => dict.set_item("max_price", value.to_string())?,
            None => dict.set_item("max_price", py.None())?,
        }
        match self.min_price {
            Some(value) => dict.set_item("min_price", value.to_string())?,
            None => dict.set_item("min_price", py.None())?,
        }
        Ok(dict.into())
    }
}

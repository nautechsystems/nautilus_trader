// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::python::{serialization::from_dict_pyo3, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::{
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    instruments::crypto_perpetual::CryptoPerpetual,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[pymethods]
impl CryptoPerpetual {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        id: InstrumentId,
        symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        margin_init: Decimal,
        margin_maint: Decimal,
        maker_fee: Decimal,
        taker_fee: Decimal,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> PyResult<Self> {
        Self::new(
            id,
            symbol,
            base_currency,
            quote_currency,
            settlement_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
        )
        .map_err(to_pyvalue_err)
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

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(CryptoPerpetual))?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("raw_symbol", self.raw_symbol.to_string())?;
        dict.set_item("base_currency", self.base_currency.code.to_string())?;
        dict.set_item("quote_currency", self.quote_currency.code.to_string())?;
        dict.set_item(
            "settlement_currency",
            self.settlement_currency.code.to_string(),
        )?;
        dict.set_item("price_precision", self.price_precision)?;
        dict.set_item("size_precision", self.size_precision)?;
        dict.set_item("price_increment", self.price_increment.to_string())?;
        dict.set_item("size_increment", self.size_increment.to_string())?;
        dict.set_item("margin_init", self.margin_init.to_f64())?;
        dict.set_item("margin_maint", self.margin_maint.to_f64())?;
        dict.set_item("maker_fee", self.margin_init.to_f64())?;
        dict.set_item("taker_fee", self.margin_init.to_f64())?;
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
        match self.max_notional {
            Some(value) => dict.set_item("max_notional", value.to_string())?,
            None => dict.set_item("max_notional", py.None())?,
        }
        match self.min_notional {
            Some(value) => dict.set_item("min_notional", value.to_string())?,
            None => dict.set_item("min_notional", py.None())?,
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

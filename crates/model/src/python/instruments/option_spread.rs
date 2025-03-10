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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::{
    IntoPyObjectNautilusExt, serialization::from_dict_pyo3, to_pyvalue_err,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::OptionSpread,
    types::{Currency, Price, Quantity},
};

#[pymethods]
impl OptionSpread {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (id, raw_symbol, asset_class, underlying, strategy_type, activation_ns, expiration_ns, currency, price_precision, price_increment, multiplier, lot_size, ts_event, ts_init, max_quantity=None, min_quantity=None, max_price=None, min_price=None, margin_init=None, margin_maint=None, maker_fee=None, taker_fee=None, exchange=None))]
    fn py_new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: String,
        strategy_type: String,
        activation_ns: u64,
        expiration_ns: u64,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        ts_event: u64,
        ts_init: u64,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        exchange: Option<String>,
    ) -> PyResult<Self> {
        Self::new_checked(
            id,
            raw_symbol,
            asset_class,
            exchange.map(|x| Ustr::from(&x)),
            underlying.into(),
            strategy_type.into(),
            activation_ns.into(),
            expiration_ns.into(),
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    #[getter]
    fn type_str(&self) -> &str {
        stringify!(OptionSpread)
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> InstrumentId {
        self.id
    }

    #[getter]
    #[pyo3(name = "raw_symbol")]
    fn py_raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    #[getter]
    #[pyo3(name = "asset_class")]
    fn py_asset_class(&self) -> AssetClass {
        self.asset_class
    }

    #[getter]
    #[pyo3(name = "exchange")]
    fn py_exchange(&self) -> Option<String> {
        self.exchange.map(|e| e.to_string())
    }

    #[getter]
    #[pyo3(name = "underlying")]
    fn py_underlying(&self) -> &str {
        self.underlying.as_str()
    }

    #[getter]
    #[pyo3(name = "strategy_type")]
    fn py_option_kind(&self) -> &str {
        self.strategy_type.as_str()
    }

    #[getter]
    #[pyo3(name = "activation_ns")]
    fn py_activation_ns(&self) -> u64 {
        self.activation_ns.as_u64()
    }

    #[getter]
    #[pyo3(name = "expiration_ns")]
    fn py_expiration_ns(&self) -> u64 {
        self.expiration_ns.as_u64()
    }

    #[getter]
    #[pyo3(name = "currency")]
    fn py_currency(&self) -> Currency {
        self.currency
    }

    #[getter]
    #[pyo3(name = "price_precision")]
    fn py_price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    #[pyo3(name = "price_increment")]
    fn py_price_increment(&self) -> Price {
        self.price_increment
    }

    #[getter]
    #[pyo3(name = "size_increment")]
    fn py_size_increment(&self) -> Quantity {
        self.size_increment
    }

    #[getter]
    #[pyo3(name = "size_precision")]
    fn py_size_precision(&self) -> u8 {
        self.size_precision
    }

    #[getter]
    #[pyo3(name = "multiplier")]
    fn py_multiplier(&self) -> Quantity {
        self.multiplier
    }

    #[getter]
    #[pyo3(name = "lot_size")]
    fn py_lot_size(&self) -> Quantity {
        self.lot_size
    }

    #[getter]
    #[pyo3(name = "max_quantity")]
    fn py_max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    #[getter]
    #[pyo3(name = "min_quantity")]
    fn py_min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
    }

    #[getter]
    #[pyo3(name = "max_price")]
    fn py_max_price(&self) -> Option<Price> {
        self.max_price
    }

    #[getter]
    #[pyo3(name = "min_price")]
    fn py_min_price(&self) -> Option<Price> {
        self.min_price
    }

    #[getter]
    #[pyo3(name = "margin_init")]
    fn py_margin_init(&self) -> Decimal {
        self.margin_init
    }

    #[getter]
    #[pyo3(name = "margin_maint")]
    fn py_margin_maint(&self) -> Decimal {
        self.margin_maint
    }

    #[getter]
    #[pyo3(name = "info")]
    fn py_info(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(PyDict::new(py).into())
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

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(OptionSpread))?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("raw_symbol", self.raw_symbol.to_string())?;
        dict.set_item("asset_class", self.asset_class.to_string())?;
        dict.set_item("underlying", self.underlying.to_string())?;
        dict.set_item("strategy_type", self.strategy_type.to_string())?;
        dict.set_item("activation_ns", self.activation_ns.as_u64())?;
        dict.set_item("expiration_ns", self.expiration_ns.as_u64())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("price_precision", self.price_precision)?;
        dict.set_item("price_increment", self.price_increment.to_string())?;
        dict.set_item("size_increment", self.size_increment.to_string())?;
        dict.set_item("size_precision", self.size_precision)?;
        dict.set_item("multiplier", self.multiplier.to_string())?;
        dict.set_item("lot_size", self.lot_size.to_string())?;
        dict.set_item("margin_init", self.margin_init.to_string())?;
        dict.set_item("margin_maint", self.margin_maint.to_string())?;
        dict.set_item("maker_fee", self.maker_fee.to_string())?;
        dict.set_item("taker_fee", self.taker_fee.to_string())?;
        dict.set_item("info", PyDict::new(py))?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
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
        match self.exchange {
            Some(value) => dict.set_item("exchange", value.to_string())?,
            None => dict.set_item("exchange", py.None())?,
        }
        Ok(dict.into())
    }
}

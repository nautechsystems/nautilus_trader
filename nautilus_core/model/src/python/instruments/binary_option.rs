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

use nautilus_core::python::{serialization::from_dict_pyo3, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::binary_option::BinaryOption,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[pymethods]
impl BinaryOption {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (id, raw_symbol, asset_class, currency, activation_ns, expiration_ns, price_precision, size_precision, price_increment, size_increment, maker_fee, taker_fee, ts_event, ts_init, outcome=None, description=None, margin_init=None, margin_maint=None, max_quantity=None, min_quantity=None, max_notional=None, min_notional=None, max_price=None, min_price=None))]
    fn py_new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        activation_ns: u64,
        expiration_ns: u64,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        ts_event: u64,
        ts_init: u64,
        outcome: Option<String>,
        description: Option<String>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> PyResult<Self> {
        Self::new_checked(
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns.into(),
            expiration_ns.into(),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            maker_fee,
            taker_fee,
            outcome.map(|x| Ustr::from(&x)),
            description.map(|x| Ustr::from(&x)),
            margin_init,
            margin_maint,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            _ => panic!("Not implemented"),
        }
    }

    #[getter]
    fn type_str(&self) -> &str {
        stringify!(BinaryOption)
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
    #[pyo3(name = "currency")]
    fn py_quote_currency(&self) -> Currency {
        self.currency
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
    #[pyo3(name = "price_precision")]
    fn py_price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    #[pyo3(name = "size_precision")]
    fn py_size_precision(&self) -> u8 {
        self.size_precision
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
    #[pyo3(name = "outcome")]
    fn py_outcome(&self) -> Option<&str> {
        match self.outcome {
            Some(outcome) => Some(outcome.as_str()),
            None => None,
        }
    }

    #[getter]
    #[pyo3(name = "description")]
    fn py_description(&self) -> Option<&str> {
        match self.description {
            Some(description) => Some(description.as_str()),
            None => None,
        }
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
    #[pyo3(name = "max_notional")]
    fn py_max_notional(&self) -> Option<Money> {
        self.max_notional
    }

    #[getter]
    #[pyo3(name = "min_notional")]
    fn py_min_notional(&self) -> Option<Money> {
        self.min_notional
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
    fn py_margin_init(&self) -> Option<Decimal> {
        self.margin_init
    }

    #[getter]
    #[pyo3(name = "margin_maint")]
    fn py_margin_maint(&self) -> Option<Decimal> {
        self.margin_maint
    }

    #[getter]
    #[pyo3(name = "maker_fee")]
    fn py_maker_fee(&self) -> Decimal {
        self.maker_fee
    }

    #[getter]
    #[pyo3(name = "taker_fee")]
    fn py_taker_fee(&self) -> Decimal {
        self.taker_fee
    }

    #[getter]
    #[pyo3(name = "info")]
    fn py_info(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(PyDict::new_bound(py).into())
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
        let dict = PyDict::new_bound(py);
        dict.set_item("type", stringify!(BinaryOption))?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("raw_symbol", self.raw_symbol.to_string())?;
        dict.set_item("asset_class", self.asset_class.to_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("activation_ns", self.activation_ns.as_u64())?;
        dict.set_item("expiration_ns", self.expiration_ns.as_u64())?;
        dict.set_item("price_precision", self.price_precision)?;
        dict.set_item("size_precision", self.size_precision)?;
        dict.set_item("price_increment", self.price_increment.to_string())?;
        dict.set_item("size_increment", self.size_increment.to_string())?;
        dict.set_item("info", PyDict::new_bound(py))?;
        dict.set_item("maker_fee", self.maker_fee.to_string())?;
        dict.set_item("taker_fee", self.taker_fee.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        match &self.outcome {
            Some(value) => dict.set_item("outcome", value.to_string())?,
            None => dict.set_item("outcome", py.None())?,
        }
        match &self.description {
            Some(value) => dict.set_item("description", value.to_string())?,
            None => dict.set_item("description", py.None())?,
        }
        match self.margin_init {
            Some(value) => dict.set_item("margin_init", value.to_string())?,
            None => dict.set_item("margin_init", py.None())?,
        }
        match self.margin_maint {
            Some(value) => dict.set_item("margin_maint", value.to_string())?,
            None => dict.set_item("margin_maint", py.None())?,
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, prepare_freethreaded_python, types::PyDict};
    use rstest::rstest;

    use crate::instruments::{binary_option::BinaryOption, stubs::*};

    #[rstest]
    fn test_dict_round_trip(binary_option: BinaryOption) {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let values = binary_option.py_to_dict(py).unwrap();
            let values: Py<PyDict> = values.extract(py).unwrap();
            let new_binary_option = BinaryOption::py_from_dict(py, values).unwrap();
            assert_eq!(binary_option, new_binary_option);
        })
    }
}

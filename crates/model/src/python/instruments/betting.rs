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
    enums::{AssetClass, InstrumentClass},
    identifiers::{InstrumentId, Symbol},
    instruments::BettingInstrument,
    types::{Currency, Money, Price, Quantity},
};

#[pymethods]
impl BettingInstrument {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (id, raw_symbol, event_type_id, event_type_name, competition_id, competition_name, event_id, event_name, event_country_code, event_open_date, betting_type, market_id, market_name, market_type, market_start_time, selection_id, selection_name, selection_handicap, currency, price_precision, size_precision, price_increment, size_increment, ts_event, ts_init, max_quantity=None, min_quantity=None, max_notional=None, min_notional=None, max_price=None, min_price=None, margin_init=None, margin_maint=None, maker_fee=None, taker_fee=None))]
    fn py_new(
        id: InstrumentId,
        raw_symbol: Symbol,
        event_type_id: u64,
        event_type_name: String,
        competition_id: u64,
        competition_name: String,
        event_id: u64,
        event_name: String,
        event_country_code: String,
        event_open_date: u64,
        betting_type: String,
        market_id: String,
        market_name: String,
        market_type: String,
        market_start_time: u64,
        selection_id: u64,
        selection_name: String,
        selection_handicap: f64,
        currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: u64,
        ts_init: u64,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> PyResult<Self> {
        Self::new_checked(
            id,
            raw_symbol,
            event_type_id,
            Ustr::from(&event_type_name),
            competition_id,
            Ustr::from(&competition_name),
            event_id,
            Ustr::from(&event_name),
            Ustr::from(&event_country_code),
            event_open_date.into(),
            Ustr::from(&betting_type),
            Ustr::from(&market_id),
            Ustr::from(&market_name),
            Ustr::from(&market_type),
            market_start_time.into(),
            selection_id,
            Ustr::from(&selection_name),
            selection_handicap,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
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

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    #[getter]
    fn type_str(&self) -> &str {
        stringify!(BettingInstrument)
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
        AssetClass::Alternative
    }

    #[getter]
    #[pyo3(name = "instrument_class")]
    fn py_instrument_class(&self) -> InstrumentClass {
        InstrumentClass::SportsBetting
    }

    #[getter]
    #[pyo3(name = "event_type_id")]
    fn py_event_type_id(&self) -> u64 {
        self.event_type_id
    }

    #[getter]
    #[pyo3(name = "event_type_name")]
    fn py_event_type_name(&self) -> &str {
        self.event_type_name.as_str()
    }

    #[getter]
    #[pyo3(name = "competition_id")]
    fn py_competition_id(&self) -> u64 {
        self.competition_id
    }

    #[getter]
    #[pyo3(name = "competition_name")]
    fn py_competition_name(&self) -> &str {
        self.competition_name.as_str()
    }

    #[getter]
    #[pyo3(name = "event_id")]
    fn py_event_id(&self) -> u64 {
        self.event_id
    }

    #[getter]
    #[pyo3(name = "event_name")]
    fn py_event_name(&self) -> &str {
        self.event_name.as_str()
    }

    #[getter]
    #[pyo3(name = "event_country_code")]
    fn py_event_country_code(&self) -> &str {
        self.event_country_code.as_str()
    }

    #[getter]
    #[pyo3(name = "event_open_date")]
    fn py_event_open_date(&self) -> u64 {
        self.event_open_date.as_u64()
    }

    #[getter]
    #[pyo3(name = "betting_type")]
    fn py_betting_type(&self) -> &str {
        self.betting_type.as_str()
    }

    #[getter]
    #[pyo3(name = "market_id")]
    fn py_market_id(&self) -> &str {
        self.market_id.as_str()
    }

    #[getter]
    #[pyo3(name = "market_name")]
    fn py_market_name(&self) -> &str {
        self.market_name.as_str()
    }

    #[getter]
    #[pyo3(name = "market_type")]
    fn py_market_type(&self) -> &str {
        self.market_type.as_str()
    }

    #[getter]
    #[pyo3(name = "market_start_time")]
    fn py_market_start_time(&self) -> u64 {
        self.market_start_time.as_u64()
    }

    #[getter]
    #[pyo3(name = "selection_id")]
    fn py_selection_id(&self) -> u64 {
        self.selection_id
    }

    #[getter]
    #[pyo3(name = "selection_name")]
    fn py_selection_name(&self) -> &str {
        self.selection_name.as_str()
    }

    #[getter]
    #[pyo3(name = "selection_name")]
    fn py_selection_handicap(&self) -> f64 {
        self.selection_handicap
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
        dict.set_item("type", stringify!(BettingInstrument))?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("raw_symbol", self.raw_symbol.to_string())?;
        dict.set_item("event_type_id", self.event_type_id)?;
        dict.set_item("event_type_name", self.event_type_name.to_string())?;
        dict.set_item("competition_id", self.competition_id)?;
        dict.set_item("competition_name", self.competition_name.to_string())?;
        dict.set_item("event_id", self.event_id)?;
        dict.set_item("event_name", self.event_name.to_string())?;
        dict.set_item("event_country_code", self.event_country_code.to_string())?;
        dict.set_item("event_open_date", self.event_open_date.as_u64())?;
        dict.set_item("betting_type", self.betting_type.to_string())?;
        dict.set_item("market_id", self.market_id.to_string())?;
        dict.set_item("market_name", self.market_name.to_string())?;
        dict.set_item("market_type", self.market_type.to_string())?;
        dict.set_item("market_start_time", self.market_start_time.as_u64())?;
        dict.set_item("selection_id", self.selection_id)?;
        dict.set_item("selection_name", self.selection_name.to_string())?;
        dict.set_item("selection_handicap", self.selection_handicap)?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("price_precision", self.price_precision)?;
        dict.set_item("size_precision", self.size_precision)?;
        dict.set_item("price_increment", self.price_increment.to_string())?;
        dict.set_item("size_increment", self.size_increment.to_string())?;
        dict.set_item("margin_init", self.margin_init.to_string())?;
        dict.set_item("margin_maint", self.margin_maint.to_string())?;
        dict.set_item("maker_fee", self.maker_fee.to_string())?;
        dict.set_item("taker_fee", self.taker_fee.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("info", PyDict::new(py))?;
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

    use crate::instruments::{BettingInstrument, stubs::*};

    #[rstest]
    fn test_dict_round_trip(betting: BettingInstrument) {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let values = betting.py_to_dict(py).unwrap();
            let values: Py<PyDict> = values.extract(py).unwrap();
            let new_betting = BettingInstrument::py_from_dict(py, values).unwrap();
            assert_eq!(betting, new_betting);
        })
    }
}

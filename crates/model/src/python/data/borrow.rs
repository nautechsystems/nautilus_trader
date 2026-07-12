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

//! Python bindings for borrow rate data types.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pykey_err, to_pyvalue_err},
    serialization::{
        Serializable,
        msgpack::{FromMsgPack, ToMsgPack},
    },
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};
use rust_decimal::Decimal;

use crate::{
    data::BorrowRate,
    identifiers::Venue,
    python::common::PY_MODULE_MODEL,
    types::{Currency, Money},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BorrowRate {
    /// Represents a borrow rate for a margin-eligible currency at a venue.
    #[new]
    #[pyo3(signature = (currency, venue, rate, accrual_interval, ts_event, ts_init, next_accrual_ns=None, borrow_limit=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        currency: Currency,
        venue: Venue,
        rate: Decimal,
        accrual_interval: u16,
        ts_event: u64,
        ts_init: u64,
        next_accrual_ns: Option<u64>,
        borrow_limit: Option<Money>,
    ) -> Self {
        Self::new(
            currency,
            venue,
            rate,
            accrual_interval,
            next_accrual_ns.map(UnixNanos::from),
            borrow_limit,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self}")
    }

    fn __hash__(&self) -> isize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Hash::hash(self, &mut hasher);
        Hasher::finish(&hasher) as isize
    }

    #[getter]
    #[pyo3(name = "currency")]
    fn py_currency(&self) -> Currency {
        self.currency
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    #[pyo3(name = "rate")]
    fn py_rate(&self) -> Decimal {
        self.rate
    }

    #[getter]
    #[pyo3(name = "accrual_interval")]
    fn py_accrual_interval(&self) -> u16 {
        self.accrual_interval
    }

    #[getter]
    #[pyo3(name = "next_accrual_ns")]
    fn py_next_accrual_ns(&self) -> Option<u64> {
        self.next_accrual_ns.map(|ts| ts.as_u64())
    }

    #[getter]
    #[pyo3(name = "borrow_limit")]
    fn py_borrow_limit(&self) -> Option<Money> {
        self.borrow_limit
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
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(BorrowRate))
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(currency: &Currency, venue: &Venue) -> HashMap<String, String> {
        Self::get_metadata(currency, venue)
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[staticmethod]
    #[pyo3(name = "get_fields")]
    fn py_get_fields() -> HashMap<String, String> {
        Self::get_fields().into_iter().collect()
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> Py<PyAny> {
        let mut dict = HashMap::new();
        dict.insert("type".to_string(), "BorrowRate".into_py_any_unwrap(py));
        dict.insert(
            "currency".to_string(),
            self.currency.code.to_string().into_py_any_unwrap(py),
        );
        dict.insert(
            "venue".to_string(),
            self.venue.to_string().into_py_any_unwrap(py),
        );
        dict.insert(
            "rate".to_string(),
            self.rate.to_string().into_py_any_unwrap(py),
        );
        dict.insert(
            "accrual_interval".to_string(),
            self.accrual_interval.into_py_any_unwrap(py),
        );

        if let Some(next_accrual_ns) = self.next_accrual_ns {
            dict.insert(
                "next_accrual_ns".to_string(),
                next_accrual_ns.as_u64().into_py_any_unwrap(py),
            );
        }

        if let Some(borrow_limit) = self.borrow_limit {
            dict.insert(
                "borrow_limit".to_string(),
                borrow_limit.to_string().into_py_any_unwrap(py),
            );
        }
        dict.insert(
            "ts_event".to_string(),
            self.ts_event.as_u64().into_py_any_unwrap(py),
        );
        dict.insert(
            "ts_init".to_string(),
            self.ts_init.as_u64().into_py_any_unwrap(py),
        );
        dict.into_py_any_unwrap(py)
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_from_dict(py: Python<'_>, values: Py<PyAny>) -> PyResult<Self> {
        let dict = values.cast_bound::<pyo3::types::PyDict>(py)?;

        let currency_str: String = dict
            .get_item("currency")?
            .ok_or_else(|| to_pykey_err("Missing 'currency' field"))?
            .extract()?;
        let currency = Currency::from_str(&currency_str).map_err(to_pyvalue_err)?;

        let venue_str: String = dict
            .get_item("venue")?
            .ok_or_else(|| to_pykey_err("Missing 'venue' field"))?
            .extract()?;
        let venue = Venue::new(venue_str);

        let rate_str: String = dict
            .get_item("rate")?
            .ok_or_else(|| to_pykey_err("Missing 'rate' field"))?
            .extract()?;
        let rate = Decimal::from_str(&rate_str).map_err(to_pyvalue_err)?;

        let accrual_interval: u16 = dict
            .get_item("accrual_interval")?
            .ok_or_else(|| to_pykey_err("Missing 'accrual_interval' field"))?
            .extract()?;

        let next_accrual_ns: Option<u64> = dict
            .get_item("next_accrual_ns")
            .ok()
            .flatten()
            .and_then(|v| v.extract().ok());

        let borrow_limit: Option<Money> = dict
            .get_item("borrow_limit")?
            .and_then(|v| v.extract::<String>().ok())
            .map(|s| Money::from_str(&s))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let ts_event: u64 = dict
            .get_item("ts_event")?
            .ok_or_else(|| to_pykey_err("Missing 'ts_event' field"))?
            .extract()?;

        let ts_init: u64 = dict
            .get_item("ts_init")?
            .ok_or_else(|| to_pykey_err("Missing 'ts_init' field"))?
            .extract()?;

        Ok(Self::new(
            currency,
            venue,
            rate,
            accrual_interval,
            next_accrual_ns.map(UnixNanos::from),
            borrow_limit,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        ))
    }

    #[pyo3(name = "to_json")]
    fn py_to_json(&self) -> PyResult<Vec<u8>> {
        self.to_json_bytes()
            .map(|b| b.to_vec())
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "to_msgpack")]
    fn py_to_msgpack(&self) -> PyResult<Vec<u8>> {
        self.to_msgpack_bytes()
            .map(|b| b.to_vec())
            .map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.cast::<PyTuple>()?;

        let currency_str: String = py_tuple.get_item(0)?.cast::<PyString>()?.extract()?;
        let venue_str: String = py_tuple.get_item(1)?.cast::<PyString>()?.extract()?;
        let rate_str: String = py_tuple.get_item(2)?.cast::<PyString>()?.extract()?;
        let accrual_interval: u16 = py_tuple.get_item(3)?.extract()?;
        let next_accrual_ns: Option<u64> = py_tuple.get_item(4).ok().and_then(|item| {
            if item.is_none() {
                None
            } else {
                item.extract().ok()
            }
        });
        let borrow_limit: Option<String> = py_tuple.get_item(5).ok().and_then(|item| {
            if item.is_none() {
                None
            } else {
                item.extract().ok()
            }
        });
        let ts_event: u64 = py_tuple.get_item(6)?.extract()?;
        let ts_init: u64 = py_tuple.get_item(7)?.extract()?;

        self.currency = Currency::from_str(&currency_str).map_err(to_pyvalue_err)?;
        self.venue = Venue::new(venue_str);
        self.rate = Decimal::from_str(&rate_str).map_err(to_pyvalue_err)?;
        self.accrual_interval = accrual_interval;
        self.next_accrual_ns = next_accrual_ns.map(UnixNanos::from);
        self.borrow_limit = borrow_limit
            .map(|s| Money::from_str(&s))
            .transpose()
            .map_err(to_pyvalue_err)?;
        self.ts_event = UnixNanos::from(ts_event);
        self.ts_init = UnixNanos::from(ts_init);

        Ok(())
    }

    fn __getstate__(&self, py: Python) -> Py<PyAny> {
        (
            self.currency.code.to_string(),
            self.venue.to_string(),
            self.rate.to_string(),
            self.accrual_interval,
            self.next_accrual_ns.map(|ts| ts.as_u64()),
            self.borrow_limit.map(|m| m.to_string()),
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .into_py_any_unwrap(py)
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py);
        Ok((safe_constructor, PyTuple::empty(py), state).into_py_any_unwrap(py))
    }

    #[staticmethod]
    #[pyo3(name = "_safe_constructor")]
    fn py_safe_constructor() -> Self {
        let currency = Currency::USD();
        Self::new(
            currency,
            Venue::new("NULL"),
            Decimal::ZERO,
            0,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }
}

#[pymethods]
impl BorrowRate {
    #[pyo3(name = "from_json")]
    #[staticmethod]
    fn py_from_json(data: &[u8]) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "from_msgpack")]
    #[staticmethod]
    fn py_from_msgpack(data: &[u8]) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(to_pyvalue_err)
    }
}

impl BorrowRate {
    /// Creates a new [`BorrowRate`] from a Python object.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if extracting any attribute or converting types fails.
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let currency: Currency = obj.getattr("currency")?.extract()?;
        let venue: Venue = obj.getattr("venue")?.extract()?;
        let rate: Decimal = obj.getattr("rate")?.extract()?;
        let accrual_interval: u16 = obj.getattr("accrual_interval")?.extract()?;
        let next_accrual_ns: Option<u64> = obj
            .getattr("next_accrual_ns")
            .ok()
            .and_then(|x| x.extract().ok());
        let borrow_limit: Option<Money> = obj
            .getattr("borrow_limit")
            .ok()
            .and_then(|x| x.extract().ok());
        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        Ok(Self::new(
            currency,
            venue,
            rate,
            accrual_interval,
            next_accrual_ns.map(UnixNanos::from),
            borrow_limit,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        ))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_py_borrow_rate_new() {
        Python::initialize();
        Python::attach(|_py| {
            let currency = Currency::USD();
            let venue = Venue::new("BYBIT");
            let rate = Decimal::new(876, 3); // 0.876
            let borrow_limit = Some(Money::new(1_000_000.0, currency));
            let ts_event = UnixNanos::from(1_640_000_000_000_000_000_u64);
            let ts_init = UnixNanos::from(1_640_000_000_000_000_000_u64);

            let borrow_rate = BorrowRate::py_new(
                currency,
                venue,
                rate,
                60,
                ts_event.as_u64(),
                ts_init.as_u64(),
                None,
                borrow_limit,
            );

            assert_eq!(borrow_rate.currency, currency);
            assert_eq!(borrow_rate.venue, venue);
            assert_eq!(borrow_rate.rate, rate);
            assert_eq!(borrow_rate.accrual_interval, 60);
            assert_eq!(borrow_rate.next_accrual_ns, None);
            assert_eq!(borrow_rate.borrow_limit, borrow_limit);
            assert_eq!(borrow_rate.ts_event, ts_event);
            assert_eq!(borrow_rate.ts_init, ts_init);
        });
    }
}

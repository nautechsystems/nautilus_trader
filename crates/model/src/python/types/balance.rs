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
    str::FromStr,
};

use nautilus_core::python::{
    parsing::{get_optional_parsed, get_required_parsed, get_required_string},
    to_pyvalue_err,
};
use pyo3::{prelude::*, types::PyDict};

use crate::{
    identifiers::InstrumentId,
    types::{AccountBalance, Currency, MarginBalance, Money},
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl AccountBalance {
    /// Represents an account balance denominated in a particular currency.
    #[new]
    fn py_new(total: Money, locked: Money, free: Money) -> PyResult<Self> {
        Self::new_checked(total, locked, free).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.total.raw().hash(&mut h);
        self.locked.raw().hash(&mut h);
        self.free.raw().hash(&mut h);
        self.currency.code.hash(&mut h);
        h.finish() as isize
    }

    #[getter]
    #[pyo3(name = "total")]
    fn py_total(&self) -> Money {
        self.total
    }

    #[getter]
    #[pyo3(name = "locked")]
    fn py_locked(&self) -> Money {
        self.locked
    }

    #[getter]
    #[pyo3(name = "free")]
    fn py_free(&self) -> Money {
        self.free
    }

    #[getter]
    #[pyo3(name = "currency")]
    fn py_currency(&self) -> Currency {
        self.currency
    }

    /// Returns a copy of this balance.
    #[pyo3(name = "copy")]
    fn py_copy(&self) -> Self {
        *self
    }

    /// Constructs an [`AccountBalance`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or conversion fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        let total = money_from_dict(values, "total", currency)?;
        let locked = money_from_dict(values, "locked", currency)?;
        let free = money_from_dict(values, "free", currency)?;
        Self::new_checked(total, locked, free).map_err(to_pyvalue_err)
    }

    /// Converts this [`AccountBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(AccountBalance))?;
        dict.set_item("total", self.total.to_decimal_string())?;
        dict.set_item("locked", self.locked.to_decimal_string())?;
        dict.set_item("free", self.free.to_decimal_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        Ok(dict.into())
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl MarginBalance {
    /// Represents a margin balance.
    ///
    /// Margin entries have two mutually exclusive scopes:
    ///
    /// - Per-instrument: `instrument_id = Some(id)`. Used for isolated margin and
    ///   for calculated margin in backtest mode where each instrument carries its
    ///   own reserve.
    /// - Account-wide (cross margin): `instrument_id = None`. Used for venues that
    ///   report a single aggregate margin per collateral currency (most derivatives
    ///   venues in cross-margin mode).
    #[new]
    #[pyo3(signature = (initial, maintenance, instrument_id=None))]
    fn py_new(
        initial: Money,
        maintenance: Money,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Self> {
        Self::new_checked(initial, maintenance, instrument_id).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.initial.raw().hash(&mut h);
        self.maintenance.raw().hash(&mut h);
        self.currency.code.hash(&mut h);
        self.instrument_id.hash(&mut h);
        h.finish() as isize
    }

    #[getter]
    #[pyo3(name = "initial")]
    fn py_initial(&self) -> Money {
        self.initial
    }

    #[getter]
    #[pyo3(name = "maintenance")]
    fn py_maintenance(&self) -> Money {
        self.maintenance
    }

    #[getter]
    #[pyo3(name = "currency")]
    fn py_currency(&self) -> Currency {
        self.currency
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    /// Returns a copy of this margin balance.
    #[pyo3(name = "copy")]
    fn py_copy(&self) -> Self {
        *self
    }

    /// Constructs a [`MarginBalance`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or conversion fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        let initial = money_from_dict(values, "initial", currency)?;
        let maintenance = money_from_dict(values, "maintenance", currency)?;

        let instrument_id = get_optional_parsed(values, "instrument_id", |s| {
            Ok::<InstrumentId, String>(InstrumentId::from(s))
        })?;

        Self::new_checked(initial, maintenance, instrument_id).map_err(to_pyvalue_err)
    }

    /// Converts this [`MarginBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(MarginBalance))?;
        dict.set_item("initial", self.initial.to_decimal_string())?;
        dict.set_item("maintenance", self.maintenance.to_decimal_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        match self.instrument_id {
            Some(id) => dict.set_item("instrument_id", id.to_string())?,
            None => dict.set_item("instrument_id", py.None())?,
        }
        Ok(dict.into())
    }
}

fn money_from_dict(values: &Bound<'_, PyDict>, field: &str, currency: Currency) -> PyResult<Money> {
    get_required_parsed(values, field, |value| {
        Money::from_decimal_str_exact(&value, currency)
    })
}

#[cfg(test)]
mod tests {
    use pyo3::{Python, types::PyDict};
    use rstest::rstest;

    use super::*;
    use crate::types::{
        fixed::FIXED_PRECISION,
        money::{MONEY_MAX, MONEY_MIN, MONEY_RAW_MAX, MONEY_RAW_MIN},
    };

    #[rstest]
    #[case(
        "total",
        "ValueError: Failed to parse 'total': Invalid decimal value 'not-a-number'"
    )]
    #[case(
        "free",
        "ValueError: Failed to parse 'free': Invalid decimal value 'not-a-number'"
    )]
    #[case(
        "locked",
        "ValueError: Failed to parse 'locked': Invalid decimal value 'not-a-number'"
    )]
    fn test_account_balance_from_dict_rejects_invalid_numeric_field(
        #[case] field: &str,
        #[case] expected: &str,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let values = PyDict::new(py);
            values.set_item("currency", "USD").unwrap();
            values.set_item("total", "1.00").unwrap();
            values.set_item("free", "1.00").unwrap();
            values.set_item("locked", "0.00").unwrap();
            values.set_item(field, "not-a-number").unwrap();

            let error = AccountBalance::py_from_dict(&values).unwrap_err();

            assert_eq!(error.to_string(), expected);
        });
    }

    #[rstest]
    fn test_account_balance_from_dict_rejects_out_of_range_money_value() {
        Python::initialize();
        Python::attach(|py| {
            let value = MONEY_MAX + 1.0;
            let values = PyDict::new(py);
            values.set_item("currency", "USD").unwrap();
            values.set_item("total", value.to_string()).unwrap();
            values.set_item("free", "1.00").unwrap();
            values.set_item("locked", "0.00").unwrap();

            let error = AccountBalance::py_from_dict(&values).unwrap_err();

            let raw = (value as i128) * 10_i128.pow(u32::from(FIXED_PRECISION));

            let detail = if cfg!(feature = "high-precision") {
                format!(
                    "`raw` value {raw} exceeded bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}] for Money"
                )
            } else {
                format!("Raw value {raw} exceeds MoneyRaw range for Money")
            };

            assert_eq!(
                error.to_string(),
                format!("ValueError: Failed to parse 'total': {detail}")
            );
        });
    }

    #[rstest]
    #[case(
        "initial",
        "ValueError: Failed to parse 'initial': Invalid decimal value 'not-a-number'"
    )]
    #[case(
        "maintenance",
        "ValueError: Failed to parse 'maintenance': Invalid decimal value 'not-a-number'"
    )]
    fn test_margin_balance_from_dict_rejects_invalid_numeric_field(
        #[case] field: &str,
        #[case] expected: &str,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let values = PyDict::new(py);
            values.set_item("currency", "USD").unwrap();
            values.set_item("initial", "1.00").unwrap();
            values.set_item("maintenance", "0.50").unwrap();
            values.set_item("instrument_id", py.None()).unwrap();
            values.set_item(field, "not-a-number").unwrap();

            let error = MarginBalance::py_from_dict(&values).unwrap_err();

            assert_eq!(error.to_string(), expected);
        });
    }

    #[rstest]
    fn test_margin_balance_from_dict_rejects_out_of_range_money_value() {
        Python::initialize();
        Python::attach(|py| {
            let value = MONEY_MIN - 1.0;
            let values = PyDict::new(py);
            values.set_item("currency", "USD").unwrap();
            values.set_item("initial", value.to_string()).unwrap();
            values.set_item("maintenance", "0.50").unwrap();
            values.set_item("instrument_id", py.None()).unwrap();

            let error = MarginBalance::py_from_dict(&values).unwrap_err();

            let raw = (value as i128) * 10_i128.pow(u32::from(FIXED_PRECISION));

            let detail = if cfg!(feature = "high-precision") {
                format!(
                    "`raw` value {raw} exceeded bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}] for Money"
                )
            } else {
                format!("Raw value {raw} exceeds MoneyRaw range for Money")
            };

            assert_eq!(
                error.to_string(),
                format!("ValueError: Failed to parse 'initial': {detail}")
            );
        });
    }
}

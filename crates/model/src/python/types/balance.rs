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
    parsing::{get_optional_parsed, get_required_string},
    to_pyvalue_err,
};
use pyo3::{prelude::*, types::PyDict};

use crate::{
    identifiers::InstrumentId,
    types::{AccountBalance, Currency, MarginBalance, Money},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
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
        self.total.raw.hash(&mut h);
        self.locked.raw.hash(&mut h);
        self.free.raw.hash(&mut h);
        self.currency.code.hash(&mut h);
        h.finish() as isize
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
    /// Returns a `PyErr` if parsing or conversion fails.
    ///
    /// # Panics
    ///
    /// Panics if parsing numeric values (`unwrap()`) fails due to invalid format.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let total_str = get_required_string(values, "total")?;
        let total: f64 = total_str.parse::<f64>().unwrap();
        let free_str = get_required_string(values, "free")?;
        let free: f64 = free_str.parse::<f64>().unwrap();
        let locked_str = get_required_string(values, "locked")?;
        let locked: f64 = locked_str.parse::<f64>().unwrap();
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        Self::new_checked(
            Money::new(total, currency),
            Money::new(locked, currency),
            Money::new(free, currency),
        )
        .map_err(to_pyvalue_err)
    }

    /// Converts this [`AccountBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(AccountBalance))?;
        dict.set_item(
            "total",
            format!(
                "{:.*}",
                self.total.currency.precision as usize,
                self.total.as_f64()
            ),
        )?;
        dict.set_item(
            "locked",
            format!(
                "{:.*}",
                self.locked.currency.precision as usize,
                self.locked.as_f64()
            ),
        )?;
        dict.set_item(
            "free",
            format!(
                "{:.*}",
                self.free.currency.precision as usize,
                self.free.as_f64()
            ),
        )?;
        dict.set_item("currency", self.currency.code.to_string())?;
        Ok(dict.into())
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
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
        self.initial.raw.hash(&mut h);
        self.maintenance.raw.hash(&mut h);
        self.currency.code.hash(&mut h);
        self.instrument_id.hash(&mut h);
        h.finish() as isize
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
    /// Returns a `PyErr` if parsing or conversion fails.
    ///
    /// # Panics
    ///
    /// Panics if parsing numeric values (`unwrap()`) fails due to invalid format.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let initial_str = get_required_string(values, "initial")?;
        let initial: f64 = initial_str.parse::<f64>().unwrap();
        let maintenance_str = get_required_string(values, "maintenance")?;
        let maintenance: f64 = maintenance_str.parse::<f64>().unwrap();
        let instrument_id = get_optional_parsed(values, "instrument_id", |s| {
            Ok::<InstrumentId, String>(InstrumentId::from(s))
        })?;
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        Self::new_checked(
            Money::new(initial, currency),
            Money::new(maintenance, currency),
            instrument_id,
        )
        .map_err(to_pyvalue_err)
    }

    /// Converts this [`MarginBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization fails.
    ///
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(MarginBalance))?;
        dict.set_item(
            "initial",
            format!(
                "{:.*}",
                self.initial.currency.precision as usize,
                self.initial.as_f64()
            ),
        )?;
        dict.set_item(
            "maintenance",
            format!(
                "{:.*}",
                self.maintenance.currency.precision as usize,
                self.maintenance.as_f64()
            ),
        )?;
        dict.set_item("currency", self.currency.code.to_string())?;
        match self.instrument_id {
            Some(id) => dict.set_item("instrument_id", id.to_string())?,
            None => dict.set_item("instrument_id", py.None())?,
        }
        Ok(dict.into())
    }
}

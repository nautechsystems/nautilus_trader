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

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::{prelude::*, pyclass::CompareOp};
use ustr::Ustr;

use crate::identifiers::{OptionSeriesId, Venue};

#[pymethods]
impl OptionSeriesId {
    #[new]
    fn py_new(
        venue: &str,
        underlying: &str,
        settlement_currency: &str,
        expiration_ns: u64,
    ) -> Self {
        Self {
            venue: Venue::new(venue),
            underlying: Ustr::from(underlying),
            settlement_currency: Ustr::from(settlement_currency),
            expiration_ns: UnixNanos::from(expiration_ns),
        }
    }

    /// Creates an `OptionSeriesId` from venue, underlying, settlement currency, and date string.
    #[staticmethod]
    #[pyo3(name = "from_expiry")]
    fn py_from_expiry(
        venue: &str,
        underlying: &str,
        settlement_currency: &str,
        date_str: &str,
    ) -> PyResult<Self> {
        Self::from_expiry(venue, underlying, settlement_currency, date_str).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    #[pyo3(name = "underlying")]
    fn py_underlying(&self) -> String {
        self.underlying.to_string()
    }

    #[getter]
    #[pyo3(name = "settlement_currency")]
    fn py_settlement_currency(&self) -> String {
        self.settlement_currency.to_string()
    }

    #[getter]
    #[pyo3(name = "expiration_ns")]
    fn py_expiration_ns(&self) -> u64 {
        self.expiration_ns.as_u64()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> String {
        self.to_string()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            CompareOp::Ge => self >= other,
            CompareOp::Gt => self > other,
            CompareOp::Le => self <= other,
            CompareOp::Lt => self < other,
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("OptionSeriesId('{self}')")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

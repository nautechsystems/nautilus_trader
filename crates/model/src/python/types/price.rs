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
    ops::Neg,
    str::FromStr,
};

use nautilus_core::python::{get_pytype_name, to_pytype_err, to_pyvalue_err};
use pyo3::{basic::CompareOp, conversion::IntoPyObjectExt, prelude::*, types::PyFloat};
use rust_decimal::{Decimal, RoundingStrategy};

#[cfg(not(feature = "high-precision"))]
use crate::types::fixed::fixed_i64_to_f64;
#[cfg(feature = "high-precision")]
use crate::types::fixed::fixed_i128_to_f64;
use crate::types::price::{Price, PriceRaw};

#[pymethods]
impl Price {
    #[new]
    fn py_new(value: f64, precision: u8) -> PyResult<Self> {
        Self::new_checked(value, precision).map_err(to_pyvalue_err)
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let from_raw = py.get_type::<Self>().getattr("from_raw")?;
        let args = (self.raw, self.precision).into_py_any(py)?;
        (from_raw, args).into_py_any(py)
    }

    fn __richcmp__(
        &self,
        other: &Bound<'_, PyAny>,
        op: CompareOp,
        py: Python<'_>,
    ) -> PyResult<Py<PyAny>> {
        if let Ok(other_price) = other.extract::<Self>() {
            let result = match op {
                CompareOp::Eq => self.eq(&other_price),
                CompareOp::Ne => self.ne(&other_price),
                CompareOp::Ge => self.ge(&other_price),
                CompareOp::Gt => self.gt(&other_price),
                CompareOp::Le => self.le(&other_price),
                CompareOp::Lt => self.lt(&other_price),
            };
            result.into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            let result = match op {
                CompareOp::Eq => self.as_decimal() == other_dec,
                CompareOp::Ne => self.as_decimal() != other_dec,
                CompareOp::Ge => self.as_decimal() >= other_dec,
                CompareOp::Gt => self.as_decimal() > other_dec,
                CompareOp::Le => self.as_decimal() <= other_dec,
                CompareOp::Lt => self.as_decimal() < other_dec,
            };
            result.into_py_any(py)
        } else {
            Ok(py.NotImplemented())
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __add__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() + other_float).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() + other_price.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() + other_dec).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __add__, was `{pytype_name}`"
            )))
        }
    }

    fn __radd__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float + self.as_f64()).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() + self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec + self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __radd__, was `{pytype_name}`"
            )))
        }
    }

    fn __sub__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() - other_float).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() - other_price.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() - other_dec).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __sub__, was `{pytype_name}`"
            )))
        }
    }

    fn __rsub__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float - self.as_f64()).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() - self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec - self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rsub__, was `{pytype_name}`"
            )))
        }
    }

    fn __mul__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() * other_float).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() * other_price.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() * other_dec).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __mul__, was `{pytype_name}`"
            )))
        }
    }

    fn __rmul__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float * self.as_f64()).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() * self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec * self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rmul__, was `{pytype_name}`"
            )))
        }
    }

    fn __truediv__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() / other_float).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() / other_price.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() / other_dec).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __truediv__, was `{pytype_name}`"
            )))
        }
    }

    fn __rtruediv__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float / self.as_f64()).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() / self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec / self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rtruediv__, was `{pytype_name}`"
            )))
        }
    }

    fn __floordiv__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() / other_float).floor().into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() / other_price.as_decimal())
                .floor()
                .into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() / other_dec).floor().into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __floordiv__, was `{pytype_name}`"
            )))
        }
    }

    fn __rfloordiv__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float / self.as_f64()).floor().into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() / self.as_decimal())
                .floor()
                .into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec / self.as_decimal()).floor().into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rfloordiv__, was `{pytype_name}`"
            )))
        }
    }

    fn __mod__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (self.as_f64() % other_float).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (self.as_decimal() % other_price.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (self.as_decimal() % other_dec).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __mod__, was `{pytype_name}`"
            )))
        }
    }

    fn __rmod__(&self, other: &Bound<'_, PyAny>, py: Python) -> PyResult<Py<PyAny>> {
        if other.is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract()?;
            (other_float % self.as_f64()).into_py_any(py)
        } else if let Ok(other_price) = other.extract::<Self>() {
            (other_price.as_decimal() % self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec % self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rmod__, was `{pytype_name}`"
            )))
        }
    }

    fn __neg__(&self) -> Decimal {
        self.as_decimal().neg()
    }

    fn __pos__(&self) -> Decimal {
        let mut value = self.as_decimal();
        value.set_sign_positive(true);
        value
    }

    fn __abs__(&self) -> Decimal {
        self.as_decimal().abs()
    }

    fn __int__(&self) -> i64 {
        self.as_f64() as i64
    }

    fn __float__(&self) -> f64 {
        self.as_f64()
    }

    #[pyo3(signature = (ndigits=None))]
    fn __round__(&self, ndigits: Option<u32>) -> Decimal {
        self.as_decimal()
            .round_dp_with_strategy(ndigits.unwrap_or(0), RoundingStrategy::MidpointNearestEven)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn raw(&self) -> PriceRaw {
        self.raw
    }

    #[getter]
    fn precision(&self) -> u8 {
        self.precision
    }

    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: PriceRaw, precision: u8) -> Self {
        Self::from_raw(raw, precision)
    }

    #[staticmethod]
    #[pyo3(name = "zero")]
    #[pyo3(signature = (precision = 0))]
    fn py_zero(precision: u8) -> PyResult<Self> {
        Self::new_checked(0.0, precision).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_int")]
    fn py_from_int(value: u64) -> PyResult<Self> {
        Self::new_checked(value as f64, 0).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_zero")]
    fn py_is_zero(&self) -> bool {
        self.is_zero()
    }

    #[pyo3(name = "is_positive")]
    fn py_is_positive(&self) -> bool {
        self.is_positive()
    }

    #[cfg(feature = "high-precision")]
    #[pyo3(name = "as_double")]
    fn py_as_double(&self) -> f64 {
        fixed_i128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    #[pyo3(name = "as_double")]
    fn py_as_double(&self) -> f64 {
        fixed_i64_to_f64(self.raw)
    }

    #[pyo3(name = "as_decimal")]
    fn py_as_decimal(&self) -> Decimal {
        self.as_decimal()
    }

    #[pyo3(name = "to_formatted_str")]
    fn py_to_formatted_str(&self) -> String {
        self.to_formatted_string()
    }
}

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

use nautilus_core::python::{get_pytype_name, to_pytype_err, to_pyvalue_err};
use pyo3::{IntoPyObjectExt, basic::CompareOp, prelude::*, types::PyFloat};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::types::{Currency, Money, money::MoneyRaw};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Money {
    /// Represents an amount of money in a specified currency denomination.
    ///
    /// - `MONEY_MAX` - Maximum representable money amount
    /// - `MONEY_MIN` - Minimum representable money amount
    #[new]
    fn py_new(amount: f64, currency: Currency) -> PyResult<Self> {
        Self::new_checked(amount, currency).map_err(to_pyvalue_err)
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let from_raw = py.get_type::<Self>().getattr("from_raw")?;
        let args = (self.raw, self.currency).into_py_any(py)?;
        (from_raw, args).into_py_any(py)
    }

    fn __richcmp__(
        &self,
        other: &Bound<'_, PyAny>,
        op: CompareOp,
        py: Python<'_>,
    ) -> PyResult<Py<PyAny>> {
        if let Ok(other_money) = other.extract::<Self>() {
            if self.currency != other_money.currency {
                return Err(to_pyvalue_err(format!(
                    "Cannot compare Money with different currencies: {} vs {}",
                    self.currency.code, other_money.currency.code
                )));
            }
            let result = match op {
                CompareOp::Eq => self.eq(&other_money),
                CompareOp::Ne => self.ne(&other_money),
                CompareOp::Ge => self.ge(&other_money),
                CompareOp::Gt => self.gt(&other_money),
                CompareOp::Le => self.le(&other_money),
                CompareOp::Lt => self.lt(&other_money),
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
            let other_float: f64 = other.extract::<f64>()?;
            (self.as_f64() + other_float).into_py_any(py)
        } else if let Ok(other_money) = other.extract::<Self>() {
            if self.currency != other_money.currency {
                return Err(to_pyvalue_err(format!(
                    "Currency mismatch: cannot add {} to {}",
                    other_money.currency.code, self.currency.code
                )));
            }
            (*self + other_money).into_py_any(py)
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
        } else if let Ok(other_money) = other.extract::<Self>() {
            if self.currency != other_money.currency {
                return Err(to_pyvalue_err(format!(
                    "Currency mismatch: cannot add {} to {}",
                    self.currency.code, other_money.currency.code
                )));
            }
            (other_money + *self).into_py_any(py)
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
        } else if let Ok(other_money) = other.extract::<Self>() {
            if self.currency != other_money.currency {
                return Err(to_pyvalue_err(format!(
                    "Currency mismatch: cannot subtract {} from {}",
                    other_money.currency.code, self.currency.code
                )));
            }
            (*self - other_money).into_py_any(py)
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
        } else if let Ok(other_money) = other.extract::<Self>() {
            if self.currency != other_money.currency {
                return Err(to_pyvalue_err(format!(
                    "Currency mismatch: cannot subtract {} from {}",
                    self.currency.code, other_money.currency.code
                )));
            }
            (other_money - *self).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (self.as_decimal() * other_qty.as_decimal()).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (other_qty.as_decimal() * self.as_decimal()).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (self.as_decimal() / other_qty.as_decimal()).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (other_qty.as_decimal() / self.as_decimal()).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (self.as_decimal() / other_qty.as_decimal())
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (other_qty.as_decimal() / self.as_decimal())
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (self.as_decimal() % other_qty.as_decimal()).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (other_qty.as_decimal() % self.as_decimal()).into_py_any(py)
        } else if let Ok(other_dec) = other.extract::<Decimal>() {
            (other_dec % self.as_decimal()).into_py_any(py)
        } else {
            let pytype_name = get_pytype_name(other)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rmod__, was `{pytype_name}`"
            )))
        }
    }

    fn __neg__(&self) -> Self {
        -*self
    }

    fn __pos__(&self) -> Self {
        *self
    }

    fn __abs__(&self) -> Self {
        if self.raw < 0 { -*self } else { *self }
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
    fn raw(&self) -> MoneyRaw {
        self.raw
    }

    #[getter]
    fn currency(&self) -> Currency {
        self.currency
    }

    /// Creates a new `Money` instance with a value of zero with the given `Currency`.
    #[staticmethod]
    #[pyo3(name = "zero")]
    fn py_zero(currency: Currency) -> Self {
        Self::new(0.0, currency)
    }

    /// Creates a new `Money` instance from the given `raw` fixed-point value and the specified `currency`.
    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: MoneyRaw, currency: Currency) -> Self {
        Self::from_raw(raw, currency)
    }

    /// Creates a new `Money` from a `Decimal` value with specified currency.
    ///
    /// This method provides more reliable parsing by using Decimal arithmetic
    /// to avoid floating-point precision issues during conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    #[staticmethod]
    #[pyo3(name = "from_decimal")]
    fn py_from_decimal(value: Decimal, currency: Currency) -> PyResult<Self> {
        Self::from_decimal(value, currency).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    /// Returns `true` if the value of this instance is zero.
    #[pyo3(name = "is_zero")]
    fn py_is_zero(&self) -> bool {
        self.is_zero()
    }

    /// Returns `true` if the value of this instance is positive (> 0).
    #[pyo3(name = "is_positive")]
    fn py_is_positive(&self) -> bool {
        self.is_positive()
    }

    /// Returns the value of this instance as a `Decimal`.
    #[pyo3(name = "as_decimal")]
    fn py_as_decimal(&self) -> Decimal {
        self.as_decimal()
    }

    #[pyo3(name = "as_double")]
    fn py_as_double(&self) -> f64 {
        self.as_f64()
    }

    #[pyo3(name = "to_formatted_str")]
    fn py_to_formatted_str(&self) -> String {
        self.to_formatted_string()
    }

    /// Performs a checked addition, returning `None` on raw integer overflow, when
    /// the result falls outside `[MONEY_RAW_MIN, MONEY_RAW_MAX]`, or when the operands
    /// have mixed raw scales (e.g. a wei-scaled `Money` and a `FIXED_SCALAR`-scaled
    /// `Money`, even if their currency codes match).
    #[pyo3(name = "checked_add")]
    fn py_checked_add(&self, other: Self) -> PyResult<Option<Self>> {
        if self.currency != other.currency {
            return Err(to_pyvalue_err(format!(
                "Currency mismatch: cannot add {} to {}",
                other.currency.code, self.currency.code
            )));
        }
        Ok(self.checked_add(other))
    }

    /// Performs a checked subtraction, returning `None` on raw integer underflow, when
    /// the result falls outside `[MONEY_RAW_MIN, MONEY_RAW_MAX]`, or when the operands
    /// have mixed raw scales (e.g. a wei-scaled `Money` and a `FIXED_SCALAR`-scaled
    /// `Money`, even if their currency codes match).
    #[pyo3(name = "checked_sub")]
    fn py_checked_sub(&self, other: Self) -> PyResult<Option<Self>> {
        if self.currency != other.currency {
            return Err(to_pyvalue_err(format!(
                "Currency mismatch: cannot subtract {} from {}",
                other.currency.code, self.currency.code
            )));
        }
        Ok(self.checked_sub(other))
    }
}

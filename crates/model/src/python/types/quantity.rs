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
    ops::Neg,
    str::FromStr,
};

use nautilus_core::python::{get_pytype_name, to_pytype_err, to_pyvalue_err};
use pyo3::{basic::CompareOp, conversion::IntoPyObjectExt, prelude::*, types::PyFloat};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::types::{Quantity, quantity::QuantityRaw};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Quantity {
    /// Represents a quantity with a non-negative value and specified precision.
    ///
    /// Capable of storing either a whole number (no decimal places) of 'contracts'
    /// or 'shares' (instruments denominated in whole units) or a decimal value
    /// containing decimal places for instruments denominated in fractional units.
    ///
    /// Handles up to `FIXED_PRECISION` decimals of precision.
    ///
    /// - `QUANTITY_MAX` - Maximum representable quantity value.
    /// - `QUANTITY_MIN` - 0 (non-negative values only).
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
        if let Ok(other_qty) = other.extract::<Self>() {
            let result = match op {
                CompareOp::Eq => self.eq(&other_qty),
                CompareOp::Ne => self.ne(&other_qty),
                CompareOp::Ge => self.ge(&other_qty),
                CompareOp::Gt => self.gt(&other_qty),
                CompareOp::Le => self.le(&other_qty),
                CompareOp::Lt => self.lt(&other_qty),
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (*self + other_qty).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            (other_qty + *self).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            if other_qty.raw > self.raw {
                return Err(to_pyvalue_err(format!(
                    "Quantity subtraction would result in negative value: {self} - {other_qty}"
                )));
            }
            (*self - other_qty).into_py_any(py)
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
        } else if let Ok(other_qty) = other.extract::<Self>() {
            if self.raw > other_qty.raw {
                return Err(to_pyvalue_err(format!(
                    "Quantity subtraction would result in negative value: {other_qty} - {self}"
                )));
            }
            (other_qty - *self).into_py_any(py)
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

    fn __neg__(&self) -> Decimal {
        self.as_decimal().neg()
    }

    fn __pos__(&self) -> Self {
        *self
    }

    fn __abs__(&self) -> Self {
        *self
    }

    fn __int__(&self) -> u64 {
        self.as_f64() as u64
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
    fn raw(&self) -> QuantityRaw {
        self.raw
    }

    #[getter]
    fn precision(&self) -> u8 {
        self.precision
    }

    /// Creates a new `Quantity` instance from the given `raw` fixed-point value and `precision`.
    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: QuantityRaw, precision: u8) -> Self {
        Self::from_raw(raw, precision)
    }

    /// Creates a new `Quantity` instance with a value of zero with the given `precision`.
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

    /// Creates a new `Quantity` from a `Decimal` value with precision inferred from the decimal's scale.
    ///
    /// The precision is determined by the scale of the decimal (number of decimal places).
    /// The value is rounded to the inferred precision using banker's rounding (round half to even).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The inferred precision exceeds `FIXED_PRECISION`.
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    #[staticmethod]
    #[pyo3(name = "from_decimal")]
    fn py_from_decimal(decimal: Decimal) -> PyResult<Self> {
        Self::from_decimal(decimal).map_err(to_pyvalue_err)
    }

    /// Creates a new `Quantity` from a `Decimal` value with specified precision.
    ///
    /// Uses pure integer arithmetic on the Decimal's mantissa and scale for fast conversion.
    /// The value is rounded to the specified precision using banker's rounding (round half to even).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `precision` exceeds `FIXED_PRECISION`.
    /// - The decimal value is negative.
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    #[staticmethod]
    #[pyo3(name = "from_decimal_dp")]
    fn py_from_decimal_dp(decimal: Decimal, precision: u8) -> PyResult<Self> {
        Self::from_decimal_dp(decimal, precision).map_err(to_pyvalue_err)
    }

    /// Creates a new `Quantity` from a mantissa/exponent pair using pure integer arithmetic.
    ///
    /// The value is `mantissa * 10^exponent`. This avoids all floating-point and Decimal
    /// operations, making it ideal for exchange data that arrives as mantissa/exponent pairs.
    #[staticmethod]
    #[pyo3(name = "from_mantissa_exponent")]
    fn py_from_mantissa_exponent(mantissa: u64, exponent: i8, precision: u8) -> Self {
        Self::from_mantissa_exponent(mantissa, exponent, precision)
    }

    /// Returns `true` if the value of this instance is zero.
    #[pyo3(name = "is_zero")]
    fn py_is_zero(&self) -> bool {
        self.is_zero()
    }

    /// Returns `true` if the value of this instance is position (> 0).
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

    /// Computes a saturating subtraction between two quantities, logging when clamped.
    ///
    /// When `rhs` is greater than `self`, the result is clamped to zero and a warning is logged.
    /// Precision follows the `Sub` implementation: uses the maximum precision of both operands.
    #[pyo3(name = "saturating_sub")]
    fn py_saturating_sub(&self, other: Self) -> Self {
        self.saturating_sub(other)
    }

    /// Performs a checked addition, returning `None` on raw integer overflow, when the
    /// result exceeds `QUANTITY_RAW_MAX`, when either operand is `QUANTITY_UNDEF`, or
    /// when the operands have mixed raw scales (one at `FIXED_PRECISION` scale, the
    /// other at a defi `WEI_PRECISION` scale).
    ///
    /// Precision follows the `Add` implementation: uses the maximum precision of both operands.
    #[pyo3(name = "checked_add")]
    fn py_checked_add(&self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }

    /// Performs a checked subtraction, returning `None` if `rhs` is greater than `self`,
    /// when either operand is `QUANTITY_UNDEF`, or when the operands have mixed raw
    /// scales (one at `FIXED_PRECISION` scale, the other at a defi `WEI_PRECISION` scale).
    ///
    /// Precision follows the `Sub` implementation: uses the maximum precision of both operands.
    #[pyo3(name = "checked_sub")]
    fn py_checked_sub(&self, other: Self) -> Option<Self> {
        self.checked_sub(other)
    }
}

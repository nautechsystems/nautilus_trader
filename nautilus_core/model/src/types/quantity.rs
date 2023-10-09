// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    cmp::Ordering,
    collections::hash_map::DefaultHasher,
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Deref, Mul, MulAssign, Neg, Sub, SubAssign},
    str::FromStr,
};

use anyhow::Result;
use nautilus_core::{
    correctness::check_f64_in_range_inclusive,
    parsing::precision_from_str,
    python::{get_pytype_name, to_pytype_err, to_pyvalue_err},
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyFloat, PyLong, PyTuple},
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

use super::fixed::{check_fixed_precision, FIXED_PRECISION, FIXED_SCALAR};
use crate::types::fixed::{f64_to_fixed_u64, fixed_u64_to_f64};

pub const QUANTITY_MAX: f64 = 18_446_744_073.0;
pub const QUANTITY_MIN: f64 = 0.0;

#[repr(C)]
#[derive(Copy, Clone, Eq, Default)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Quantity {
    pub raw: u64,
    pub precision: u8,
}

impl Quantity {
    pub fn new(value: f64, precision: u8) -> Result<Self> {
        check_f64_in_range_inclusive(value, QUANTITY_MIN, QUANTITY_MAX, "`Quantity` value")?;
        check_fixed_precision(precision)?;

        Ok(Self {
            raw: f64_to_fixed_u64(value, precision),
            precision,
        })
    }

    pub fn from_raw(raw: u64, precision: u8) -> Result<Self> {
        check_fixed_precision(precision)?;
        Ok(Self { raw, precision })
    }

    #[must_use]
    pub fn zero(precision: u8) -> Self {
        check_fixed_precision(precision).unwrap();
        Quantity::new(0.0, precision).unwrap()
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.raw > 0
    }

    #[must_use]
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }

    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let rescaled_raw = self.raw / u64::pow(10, (FIXED_PRECISION - self.precision) as u32);
        Decimal::from_i128_with_scale(rescaled_raw as i128, self.precision as u32)
    }

    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        format!("{self}").separate_with_underscores()
    }
}

impl From<Quantity> for f64 {
    fn from(qty: Quantity) -> Self {
        qty.as_f64()
    }
}

impl From<&Quantity> for f64 {
    fn from(qty: &Quantity) -> Self {
        qty.as_f64()
    }
}

impl FromStr for Quantity {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let float_from_input = input
            .parse::<f64>()
            .map_err(|e| format!("Cannot parse `input` string '{input}' as f64: {e}"))?;

        Self::new(float_from_input, precision_from_str(input))
            .map_err(|e: anyhow::Error| e.to_string())
    }
}

impl From<&str> for Quantity {
    fn from(input: &str) -> Self {
        Self::from_str(input).unwrap()
    }
}

impl From<i64> for Quantity {
    fn from(input: i64) -> Self {
        Self::new(input as f64, 0).unwrap()
    }
}

impl Hash for Quantity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state)
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Quantity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    fn lt(&self, other: &Self) -> bool {
        self.raw.lt(&other.raw)
    }

    fn le(&self, other: &Self) -> bool {
        self.raw.le(&other.raw)
    }

    fn gt(&self, other: &Self) -> bool {
        self.raw.gt(&other.raw)
    }

    fn ge(&self, other: &Self) -> bool {
        self.raw.ge(&other.raw)
    }
}

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Quantity {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Add for Quantity {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision,
        }
    }
}

impl Sub for Quantity {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision,
        }
    }
}

impl Mul for Quantity {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            raw: (self.raw * rhs.raw) / (FIXED_SCALAR as u64),
            precision: self.precision,
        }
    }
}

impl Mul<f64> for Quantity {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl From<Quantity> for u64 {
    fn from(value: Quantity) -> Self {
        value.raw
    }
}

impl From<&Quantity> for u64 {
    fn from(value: &Quantity) -> Self {
        value.raw
    }
}

impl<T: Into<u64>> AddAssign<T> for Quantity {
    fn add_assign(&mut self, other: T) {
        self.raw += other.into();
    }
}

impl<T: Into<u64>> SubAssign<T> for Quantity {
    fn sub_assign(&mut self, other: T) {
        self.raw -= other.into();
    }
}

impl<T: Into<u64>> MulAssign<T> for Quantity {
    fn mul_assign(&mut self, other: T) {
        self.raw *= other.into();
    }
}

impl Debug for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Serialize for Quantity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Quantity {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let qty_str: &str = Deserialize::deserialize(_deserializer)?;
        let qty: Quantity = qty_str.into();
        Ok(qty)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl Quantity {
    #[new]
    fn py_new(value: f64, precision: u8) -> PyResult<Self> {
        Quantity::new(value, precision).map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyLong, &PyLong) = state.extract(py)?;
        self.raw = tuple.0.extract()?;
        self.precision = tuple.1.extract::<u8>()?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((self.raw, self.precision).to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Quantity::zero(0)) // Safe default
    }

    fn __add__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() + other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() + other_qty.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() + other_dec).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __add__, was `{pytype_name}`"
            )))
        }
    }

    fn __radd__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float + self.as_f64()).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() + self.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec + self.as_decimal()).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __radd__, was `{pytype_name}`"
            )))
        }
    }

    fn __sub__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() - other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() - other_qty.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() - other_dec).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __sub__, was `{pytype_name}`"
            )))
        }
    }

    fn __rsub__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float - self.as_f64()).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() - self.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec - self.as_decimal()).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rsub__, was `{pytype_name}`"
            )))
        }
    }

    fn __mul__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() * other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() * other_qty.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() * other_dec).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __mul__, was `{pytype_name}`"
            )))
        }
    }

    fn __rmul__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float * self.as_f64()).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() * self.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec * self.as_decimal()).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rmul__, was `{pytype_name}`"
            )))
        }
    }

    fn __truediv__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() / other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() / other_qty.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() / other_dec).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __truediv__, was `{pytype_name}`"
            )))
        }
    }

    fn __rtruediv__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float / self.as_f64()).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() / self.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec / self.as_decimal()).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rtruediv__, was `{pytype_name}`"
            )))
        }
    }

    fn __floordiv__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() / other_float).floor().into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() / other_qty.as_decimal())
                .floor()
                .into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() / other_dec).floor().into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __floordiv__, was `{pytype_name}`"
            )))
        }
    }

    fn __rfloordiv__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float / self.as_f64()).floor().into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() / self.as_decimal())
                .floor()
                .into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec / self.as_decimal()).floor().into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __rfloordiv__, was `{pytype_name}`"
            )))
        }
    }

    fn __mod__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() % other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((self.as_decimal() % other_qty.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((self.as_decimal() % other_dec).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
            Err(to_pytype_err(format!(
                "Unsupported type for __mod__, was `{pytype_name}`"
            )))
        }
    }

    fn __rmod__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((other_float % self.as_f64()).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Quantity>(py) {
            Ok((other_qty.as_decimal() % self.as_decimal()).into_py(py))
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            Ok((other_dec % self.as_decimal()).into_py(py))
        } else {
            let pytype_name = get_pytype_name(&other, py)?;
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

    fn __int__(&self) -> u64 {
        self.as_f64() as u64
    }

    fn __float__(&self) -> f64 {
        self.as_f64()
    }

    fn __round__(&self, ndigits: Option<u32>) -> Decimal {
        self.as_decimal()
            .round_dp_with_strategy(ndigits.unwrap_or(0), RoundingStrategy::MidpointNearestEven)
    }

    fn __richcmp__(&self, other: PyObject, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        if let Ok(other_qty) = other.extract::<Quantity>(py) {
            match op {
                CompareOp::Eq => self.eq(&other_qty).into_py(py),
                CompareOp::Ne => self.ne(&other_qty).into_py(py),
                CompareOp::Ge => self.ge(&other_qty).into_py(py),
                CompareOp::Gt => self.gt(&other_qty).into_py(py),
                CompareOp::Le => self.le(&other_qty).into_py(py),
                CompareOp::Lt => self.lt(&other_qty).into_py(py),
            }
        } else if let Ok(other_dec) = other.extract::<Decimal>(py) {
            match op {
                CompareOp::Eq => (self.as_decimal() == other_dec).into_py(py),
                CompareOp::Ne => (self.as_decimal() != other_dec).into_py(py),
                CompareOp::Ge => (self.as_decimal() >= other_dec).into_py(py),
                CompareOp::Gt => (self.as_decimal() > other_dec).into_py(py),
                CompareOp::Le => (self.as_decimal() <= other_dec).into_py(py),
                CompareOp::Lt => (self.as_decimal() < other_dec).into_py(py),
            }
        } else {
            py.NotImplemented()
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Quantity('{self:?}')")
    }

    #[getter]
    fn raw(&self) -> u64 {
        self.raw
    }

    #[getter]
    fn precision(&self) -> u8 {
        self.precision
    }

    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: u64, precision: u8) -> PyResult<Quantity> {
        Quantity::from_raw(raw, precision).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "zero")]
    #[pyo3(signature = (precision = 0))]
    fn py_zero(precision: u8) -> PyResult<Quantity> {
        Quantity::new(0.0, precision).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_int")]
    fn py_from_int(value: u64) -> PyResult<Quantity> {
        Quantity::new(value as f64, 0).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Quantity> {
        Quantity::from_str(value).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_zero")]
    fn py_is_zero(&self) -> bool {
        self.is_zero()
    }

    #[pyo3(name = "is_positive")]
    fn py_is_positive(&self) -> bool {
        self.is_positive()
    }

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
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_new(value: f64, precision: u8) -> Quantity {
    // SAFETY: Assumes `value` and `precision` were properly validated
    Quantity::new(value, precision).unwrap()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_from_raw(raw: u64, precision: u8) -> Quantity {
    Quantity::from_raw(raw, precision).unwrap()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_as_f64(qty: &Quantity) -> f64 {
    qty.as_f64()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_add_assign(mut a: Quantity, b: Quantity) {
    a.add_assign(b);
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_add_assign_u64(mut a: Quantity, b: u64) {
    a.add_assign(b);
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_sub_assign(mut a: Quantity, b: Quantity) {
    a.sub_assign(b);
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn quantity_sub_assign_u64(mut a: Quantity, b: u64) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use float_cmp::approx_eq;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_new() {
        // Precision out of range for fixed
        let _ = Quantity::new(1.0, 10).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Quantity::from_raw(1, 10).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Quantity::zero(10);
    }

    #[rstest]
    fn test_new() {
        let qty = Quantity::new(0.00812, 8).unwrap();
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
        assert!(!qty.is_zero());
        assert!(qty.is_positive());
        assert_eq!(qty.as_decimal(), dec!(0.00812000));
        assert!(approx_eq!(f64, qty.as_f64(), 0.00812, epsilon = 0.000001));
    }

    #[rstest]
    fn test_zero() {
        let qty = Quantity::zero(8);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.precision, 8);
        assert!(qty.is_zero());
        assert!(!qty.is_positive());
    }

    #[rstest]
    fn test_from_i64() {
        let qty = Quantity::from(100_000);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 100_000_000_000_000);
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_with_maximum_value() {
        let qty = Quantity::new(QUANTITY_MAX, 0).unwrap();
        assert_eq!(qty.raw, 18_446_744_073_000_000_000);
        assert_eq!(qty.to_string(), "18446744073");
    }

    #[rstest]
    fn test_with_minimum_positive_value() {
        let qty = Quantity::new(0.000000001, 9).unwrap();
        assert_eq!(qty.raw, 1);
        assert_eq!(qty.to_string(), "0.000000001");
    }

    #[rstest]
    fn test_with_minimum_value() {
        let qty = Quantity::new(QUANTITY_MIN, 9).unwrap();
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.to_string(), "0.000000000");
    }

    #[rstest]
    fn test_is_zero() {
        let qty = Quantity::zero(8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.0);
        assert_eq!(qty.to_string(), "0.00000000");
        assert!(qty.is_zero());
    }

    #[rstest]
    fn test_precision() {
        let qty = Quantity::new(1.001, 2).unwrap();
        assert_eq!(qty.raw, 1_000_000_000);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[rstest]
    fn test_new_from_str() {
        let qty = Quantity::from_str("0.00812000").unwrap();
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[rstest]
    #[case("0", 0)]
    #[case("1.1", 1)]
    #[case("1.123456789", 9)]
    fn test_from_str_valid_input(#[case] input: &str, #[case] expected_prec: u8) {
        let qty = Quantity::from_str(input).unwrap();
        assert_eq!(qty.precision, expected_prec);
        assert_eq!(qty.as_decimal(), Decimal::from_str(input).unwrap());
    }

    #[rstest]
    fn test_from_str_invalid_input() {
        let input = "invalid";
        let result = Quantity::from_str(input);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_add() {
        let quantity1 = Quantity::new(1.0, 0).unwrap();
        let quantity2 = Quantity::new(2.0, 0).unwrap();
        let quantity3 = quantity1 + quantity2;
        assert_eq!(quantity3.raw, 3_000_000_000);
    }

    #[rstest]
    fn test_sub() {
        let quantity1 = Quantity::new(3.0, 0).unwrap();
        let quantity2 = Quantity::new(2.0, 0).unwrap();
        let quantity3 = quantity1 - quantity2;
        assert_eq!(quantity3.raw, 1_000_000_000);
    }

    #[rstest]
    fn test_add_assign() {
        let mut quantity1 = Quantity::new(1.0, 0).unwrap();
        let quantity2 = Quantity::new(2.0, 0).unwrap();
        quantity1 += quantity2;
        assert_eq!(quantity1.raw, 3_000_000_000);
    }

    #[rstest]
    fn test_sub_assign() {
        let mut quantity1 = Quantity::new(3.0, 0).unwrap();
        let quantity2 = Quantity::new(2.0, 0).unwrap();
        quantity1 -= quantity2;
        assert_eq!(quantity1.raw, 1_000_000_000);
    }

    #[rstest]
    fn test_mul() {
        let quantity1 = Quantity::new(2.0, 1).unwrap();
        let quantity2 = Quantity::new(2.0, 1).unwrap();
        let quantity3 = quantity1 * quantity2;
        assert_eq!(quantity3.raw, 4_000_000_000);
    }

    #[rstest]
    fn test_equality() {
        assert_eq!(
            Quantity::new(1.0, 1).unwrap(),
            Quantity::new(1.0, 1).unwrap()
        );
        assert_eq!(
            Quantity::new(1.0, 1).unwrap(),
            Quantity::new(1.0, 2).unwrap()
        );
        assert_ne!(
            Quantity::new(1.1, 1).unwrap(),
            Quantity::new(1.0, 1).unwrap()
        );
        assert!(Quantity::new(1.0, 1).unwrap() <= Quantity::new(1.0, 2).unwrap());
        assert!(Quantity::new(1.1, 1).unwrap() > Quantity::new(1.0, 1).unwrap());
        assert!(Quantity::new(1.0, 1).unwrap() >= Quantity::new(1.0, 1).unwrap());
        assert!(Quantity::new(1.0, 1).unwrap() >= Quantity::new(1.0, 2).unwrap());
        assert!(Quantity::new(1.0, 1).unwrap() >= Quantity::new(1.0, 2).unwrap());
        assert!(Quantity::new(0.9, 1).unwrap() < Quantity::new(1.0, 1).unwrap());
        assert!(Quantity::new(0.9, 1).unwrap() <= Quantity::new(1.0, 2).unwrap());
        assert!(Quantity::new(0.9, 1).unwrap() <= Quantity::new(1.0, 1).unwrap());
    }

    #[rstest]
    fn test_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let qty = Quantity::from_str(input_string).unwrap();
        let mut res = String::new();
        write!(&mut res, "{qty}").unwrap();
        assert_eq!(res, input_string);
        assert_eq!(qty.to_string(), input_string);
    }
}

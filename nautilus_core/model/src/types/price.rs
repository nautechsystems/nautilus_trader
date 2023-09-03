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
    ops::{Add, AddAssign, Deref, Mul, Neg, Sub, SubAssign},
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

use super::fixed::{check_fixed_precision, FIXED_PRECISION, FIXED_SCALAR};
use crate::types::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};

pub const PRICE_MAX: f64 = 9_223_372_036.0;
pub const PRICE_MIN: f64 = -9_223_372_036.0;

/// Sentinel Price for errors.
pub const ERROR_PRICE: Price = Price {
    raw: i64::MAX,
    precision: 0,
};

#[repr(C)]
#[derive(Copy, Clone, Eq, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Price {
    pub raw: i64,
    pub precision: u8,
}

impl Price {
    pub fn new(value: f64, precision: u8) -> Result<Self> {
        check_f64_in_range_inclusive(value, PRICE_MIN, PRICE_MAX, "`Price` value")?;
        check_fixed_precision(precision)?;

        Ok(Self {
            raw: f64_to_fixed_i64(value, precision),
            precision,
        })
    }

    #[must_use]
    pub fn from_raw(raw: i64, precision: u8) -> Self {
        check_fixed_precision(precision).unwrap();
        Self { raw, precision }
    }

    #[must_use]
    pub fn max(precision: u8) -> Self {
        check_fixed_precision(precision).unwrap();
        Self {
            raw: (PRICE_MAX * FIXED_SCALAR) as i64,
            precision,
        }
    }

    #[must_use]
    pub fn min(precision: u8) -> Self {
        check_fixed_precision(precision).unwrap();
        Self {
            raw: (PRICE_MIN * FIXED_SCALAR) as i64,
            precision,
        }
    }

    #[must_use]
    pub fn zero(precision: u8) -> Self {
        check_fixed_precision(precision).unwrap();
        Self { raw: 0, precision }
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    #[must_use]
    pub fn as_f64(&self) -> f64 {
        fixed_i64_to_f64(self.raw)
    }

    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let rescaled_raw = self.raw / i64::pow(10, (FIXED_PRECISION - self.precision) as u32);
        Decimal::from_i128_with_scale(rescaled_raw as i128, self.precision as u32)
    }
}

impl FromStr for Price {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let float_from_input = input
            .parse::<f64>()
            .map_err(|err| format!("Cannot parse `input` string '{input}' as f64: {err}"))?;

        Self::new(float_from_input, precision_from_str(input))
            .map_err(|e: anyhow::Error| e.to_string())
    }
}

impl From<&str> for Price {
    fn from(input: &str) -> Self {
        Self::from_str(input).unwrap()
    }
}

impl From<Price> for f64 {
    fn from(price: Price) -> Self {
        price.as_f64()
    }
}

impl From<&Price> for f64 {
    fn from(price: &Price) -> Self {
        price.as_f64()
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state)
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Price {
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

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Price {
    type Target = i64;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Neg for Price {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            raw: -self.raw,
            precision: self.precision,
        }
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision,
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision,
        }
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        self.raw += other.raw;
    }
}

impl SubAssign for Price {
    fn sub_assign(&mut self, other: Self) {
        self.raw -= other.raw;
    }
}

impl Add<f64> for Price {
    type Output = f64;
    fn add(self, rhs: f64) -> Self::Output {
        self.as_f64() + rhs
    }
}

impl Sub<f64> for Price {
    type Output = f64;
    fn sub(self, rhs: f64) -> Self::Output {
        self.as_f64() - rhs
    }
}

impl Mul<f64> for Price {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl Debug for Price {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Serialize for Price {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Price {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let price_str: &str = Deserialize::deserialize(_deserializer)?;
        let price: Price = price_str.into();
        Ok(price)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl Price {
    #[new]
    fn py_new(value: f64, precision: u8) -> PyResult<Self> {
        Price::new(value, precision).map_err(to_pyvalue_err)
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

    fn __add__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() + other_float).into_py(py))
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() + other_price.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() + self.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() - other_price.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() - self.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() * other_price.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() * self.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() / other_price.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() / self.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() / other_price.as_decimal())
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() / self.as_decimal())
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((self.as_decimal() % other_price.as_decimal()).into_py(py))
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
        } else if let Ok(other_price) = other.extract::<Price>(py) {
            Ok((other_price.as_decimal() % self.as_decimal()).into_py(py))
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

    fn __int__(&self) -> i64 {
        self.as_f64() as i64
    }

    fn __float__(&self) -> f64 {
        self.as_f64()
    }

    fn __round__(&self, ndigits: Option<u32>) -> Decimal {
        self.as_decimal()
            .round_dp_with_strategy(ndigits.unwrap_or(0), RoundingStrategy::MidpointNearestEven)
    }

    fn __richcmp__(&self, other: PyObject, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        if let Ok(other_price) = other.extract::<Price>(py) {
            match op {
                CompareOp::Eq => self.eq(&other_price).into_py(py),
                CompareOp::Ne => self.ne(&other_price).into_py(py),
                CompareOp::Ge => self.ge(&other_price).into_py(py),
                CompareOp::Gt => self.gt(&other_price).into_py(py),
                CompareOp::Le => self.le(&other_price).into_py(py),
                CompareOp::Lt => self.lt(&other_price).into_py(py),
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
        format!("Price('{self:?}')")
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Price::zero(0)) // Safe default
    }

    #[getter]
    fn raw(&self) -> i64 {
        self.raw
    }

    #[getter]
    fn precision(&self) -> u8 {
        self.precision
    }

    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: i64, precision: u8) -> PyResult<Price> {
        check_fixed_precision(precision).map_err(to_pyvalue_err)?;
        Ok(Price::from_raw(raw, precision))
    }

    #[staticmethod]
    #[pyo3(name = "zero")]
    #[pyo3(signature = (precision = 0))]
    fn py_zero(precision: u8) -> PyResult<Price> {
        Price::new(0.0, precision).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_int")]
    fn py_from_int(value: u64) -> PyResult<Price> {
        Price::new(value as f64, 0).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Price> {
        Price::from_str(value).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_zero")]
    fn py_is_zero(&self) -> bool {
        self.is_zero()
    }

    #[pyo3(name = "is_positive")]
    fn py_is_positive(&self) -> bool {
        self.is_positive()
    }

    #[pyo3(name = "as_double")]
    fn py_as_double(&self) -> f64 {
        fixed_i64_to_f64(self.raw)
    }

    #[pyo3(name = "as_decimal")]
    fn py_as_decimal(&self) -> Decimal {
        self.as_decimal()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    // TODO: Document panic
    Price::new(value, precision).unwrap()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_from_raw(raw: i64, precision: u8) -> Price {
    Price::from_raw(raw, precision)
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_as_f64(price: &Price) -> f64 {
    price.as_f64()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_add_assign(mut a: Price, b: Price) {
    a.add_assign(b);
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_sub_assign(mut a: Price, b: Price) {
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
        let _ = Price::new(1.0, 10).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Price::from_raw(1, 10);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_max() {
        // Precision out of range for fixed
        let _ = Price::max(10);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_min() {
        // Precision out of range for fixed
        let _ = Price::min(10);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Price::zero(10);
    }

    #[rstest]
    fn test_new() {
        let price = Price::new(0.00812, 8).unwrap();
        assert_eq!(price, price);
        assert_eq!(price.raw, 8_120_000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
        assert!(!price.is_zero());
        assert_eq!(price.as_decimal(), dec!(0.00812000));
        assert!(approx_eq!(f64, price.as_f64(), 0.00812, epsilon = 0.000001));
    }

    #[rstest]
    fn test_with_maximum_value() {
        let price = Price::new(PRICE_MAX, 9).unwrap();
        assert_eq!(price.raw, 9_223_372_036_000_000_000);
        assert_eq!(price.to_string(), "9223372036.000000000");
    }

    #[rstest]
    fn test_with_minimum_positive_value() {
        let price = Price::new(0.000_000_001, 9).unwrap();
        assert_eq!(price.raw, 1);
        assert_eq!(price.to_string(), "0.000000001");
    }

    #[rstest]
    fn test_with_minimum_value() {
        let price = Price::new(PRICE_MIN, 9).unwrap();
        assert_eq!(price.raw, -9_223_372_036_000_000_000);
        assert_eq!(price.to_string(), "-9223372036.000000000");
    }

    #[rstest]
    fn test_max() {
        let price = Price::max(9);
        assert_eq!(price.raw, 9_223_372_036_000_000_000);
        assert_eq!(price.to_string(), "9223372036.000000000");
    }

    #[rstest]
    fn test_min() {
        let price = Price::min(9);
        assert_eq!(price.raw, -9_223_372_036_000_000_000);
        assert_eq!(price.to_string(), "-9223372036.000000000");
    }

    #[rstest]
    fn test_zero() {
        let price = Price::zero(0);
        assert_eq!(price.raw, 0);
        assert_eq!(price.to_string(), "0");
        assert!(price.is_zero());
    }

    #[rstest]
    fn test_is_zero() {
        let price = Price::new(0.0, 8).unwrap();
        assert_eq!(price, price);
        assert_eq!(price.raw, 0);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.0);
        assert_eq!(price.to_string(), "0.00000000");
        assert!(price.is_zero());
    }

    #[rstest]
    fn test_precision() {
        let price = Price::new(1.001, 2).unwrap();
        assert_eq!(price.raw, 1_000_000_000);
        assert_eq!(price.to_string(), "1.00");
    }

    #[rstest]
    fn test_new_from_str() {
        let price = Price::from_str("0.00812000").unwrap();
        assert_eq!(price, price);
        assert_eq!(price.raw, 8_120_000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[rstest]
    fn test_from_str_valid_input() {
        let input = "10.5";
        let expected_price = Price::new(10.5, precision_from_str(input)).unwrap();
        let result = Price::from_str(input).unwrap();
        assert_eq!(result, expected_price);
    }

    #[rstest]
    fn test_from_str_invalid_input() {
        let input = "invalid";
        let result = Price::from_str(input);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_equality() {
        assert_eq!(Price::from("1.0"), Price::from("1.0"));
        assert_eq!(Price::from("1.0"), Price::from("1.0"));
        assert_ne!(Price::from("1.1"), Price::from("1.0"));
        assert!(Price::from("1.0") <= Price::from("1.0"));
        assert!(Price::from("1.1") > Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("0.9") < Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
    }

    #[rstest]
    fn test_add() {
        let price1 = Price::new(1.000, 3).unwrap();
        let price2 = Price::new(1.011, 3).unwrap();
        let price3 = price1 + price2;
        assert_eq!(price3.raw, 2_011_000_000)
    }

    #[rstest]
    fn test_sub() {
        let price1 = Price::new(1.011, 3).unwrap();
        let price2 = Price::new(1.000, 3).unwrap();
        let price3 = price1 - price2;
        assert_eq!(price3.raw, 11_000_000);
    }

    #[rstest]
    fn test_add_assign() {
        let mut price = Price::new(1.000, 3).unwrap();
        price += Price::new(1.011, 3).unwrap();
        assert_eq!(price.raw, 2_011_000_000)
    }

    #[rstest]
    fn test_sub_assign() {
        let mut price = Price::new(1.000, 3).unwrap();
        price -= Price::new(0.011, 3).unwrap();
        assert_eq!(price.raw, 989_000_000)
    }

    #[rstest]
    fn test_mul() {
        let price1 = Price::new(1.000, 3).unwrap();
        let price2 = Price::new(1.011, 3).unwrap();
        let result = price1 * price2.into();
        assert!(approx_eq!(f64, result, 1.011, epsilon = 0.000001));
    }

    #[rstest]
    fn test_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let price = Price::from_str(input_string).unwrap();
        let mut res = String::new();
        write!(&mut res, "{price}").unwrap();
        assert_eq!(res, input_string);
    }
}

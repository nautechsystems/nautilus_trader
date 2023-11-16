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
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    ops::Neg,
    str::FromStr,
};

use nautilus_core::python::{get_pytype_name, to_pytype_err, to_pyvalue_err};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyFloat, PyLong, PyTuple},
};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::types::{fixed::fixed_i64_to_f64, price::Price};

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

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Price::zero(0)) // Safe default
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
        Price::from_raw(raw, precision).map_err(to_pyvalue_err)
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

    #[pyo3(name = "to_formatted_str")]
    fn py_to_formatted_str(&self) -> String {
        self.to_formatted_string()
    }
}

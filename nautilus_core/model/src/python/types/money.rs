// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    exceptions::PyValueError,
    prelude::*,
    pyclass::CompareOp,
    types::{PyFloat, PyLong, PyString, PyTuple},
};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::types::{currency::Currency, money::Money};

#[pymethods]
impl Money {
    #[new]
    fn py_new(value: f64, currency: Currency) -> PyResult<Self> {
        Self::new(value, currency).map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyLong, &PyString) = state.extract(py)?;
        self.raw = tuple.0.extract()?;
        let currency_code: &str = tuple.1.extract()?;
        self.currency = Currency::from_str(currency_code).map_err(to_pyvalue_err)?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((self.raw, self.currency.code.to_string()).to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Self::new(0.0, Currency::AUD()).unwrap()) // Safe default
    }

    fn __add__(&self, other: PyObject, py: Python) -> PyResult<PyObject> {
        if other.as_ref(py).is_instance_of::<PyFloat>() {
            let other_float: f64 = other.extract(py)?;
            Ok((self.as_f64() + other_float).into_py(py))
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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
        } else if let Ok(other_qty) = other.extract::<Self>(py) {
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

    fn __richcmp__(&self, other: PyObject, op: CompareOp, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if let Ok(other_money) = other.extract::<Self>(py) {
            if self.currency != other_money.currency {
                return Err(PyErr::new::<PyValueError, _>(
                    "Cannot compare `Money` with different currencies",
                ));
            }

            let result = match op {
                CompareOp::Eq => self.eq(&other_money),
                CompareOp::Ne => self.ne(&other_money),
                CompareOp::Ge => self.ge(&other_money),
                CompareOp::Gt => self.gt(&other_money),
                CompareOp::Le => self.le(&other_money),
                CompareOp::Lt => self.lt(&other_money),
            };
            Ok(result.into_py(py))
        } else {
            Ok(py.NotImplemented())
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
        let amount_str = format!("{:.*}", self.currency.precision as usize, self.as_f64());
        let code = self.currency.code.as_str();
        format!("Money('{amount_str}', {code})")
    }

    #[getter]
    fn raw(&self) -> i64 {
        self.raw
    }

    #[getter]
    fn currency(&self) -> Currency {
        self.currency
    }

    #[staticmethod]
    #[pyo3(name = "zero")]
    fn py_zero(currency: Currency) -> PyResult<Self> {
        Self::new(0.0, currency).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_raw")]
    fn py_from_raw(raw: i64, currency: Currency) -> PyResult<Self> {
        Ok(Self::from_raw(raw, currency))
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

impl ToPyObject for Money {
    fn to_object(&self, py: Python) -> PyObject {
        self.into_py(py)
    }
}

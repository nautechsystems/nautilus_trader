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

use std::str::FromStr;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyLong, PyString, PyTuple},
};
use ustr::Ustr;

use crate::{enums::CurrencyType, types::currency::Currency};

#[pymethods]
impl Currency {
    #[new]
    fn py_new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> PyResult<Self> {
        Self::new_checked(code, precision, iso4217, name, currency_type).map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.downcast::<PyTuple>()?;
        self.code = Ustr::from(
            py_tuple
                .get_item(0)?
                .downcast::<PyString>()?
                .extract::<&str>()?,
        );
        self.precision = py_tuple
            .get_item(1)?
            .downcast::<PyLong>()?
            .extract::<u8>()?;
        self.iso4217 = py_tuple
            .get_item(2)?
            .downcast::<PyLong>()?
            .extract::<u16>()?;
        self.name = Ustr::from(
            py_tuple
                .get_item(3)?
                .downcast::<PyString>()?
                .extract::<&str>()?,
        );
        self.currency_type = CurrencyType::from_str(
            py_tuple
                .get_item(4)?
                .downcast::<PyString>()?
                .extract::<&str>()?,
        )
        .map_err(to_pyvalue_err)?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((
            self.code.to_string(),
            self.precision,
            self.iso4217,
            self.name.to_string(),
            self.currency_type.to_string(),
        )
            .to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type_bound::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty_bound(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Self::AUD()) // Safe default
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        self.code.precomputed_hash() as isize
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> &'static str {
        self.code.as_str()
    }

    #[getter]
    #[pyo3(name = "code")]
    fn py_code(&self) -> &'static str {
        self.code.as_str()
    }

    #[getter]
    #[pyo3(name = "precision")]
    fn py_precision(&self) -> u8 {
        self.precision
    }

    #[getter]
    #[pyo3(name = "iso4217")]
    fn py_iso4217(&self) -> u16 {
        self.iso4217
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &'static str {
        self.name.as_str()
    }

    #[getter]
    #[pyo3(name = "currency_type")]
    fn py_currency_type(&self) -> CurrencyType {
        self.currency_type
    }

    #[staticmethod]
    #[pyo3(name = "is_fiat")]
    fn py_is_fiat(code: &str) -> PyResult<bool> {
        Self::is_fiat(code).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "is_crypto")]
    fn py_is_crypto(code: &str) -> PyResult<bool> {
        Self::is_crypto(code).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "is_commodity_backed")]
    fn py_is_commodidity_backed(code: &str) -> PyResult<bool> {
        Self::is_commodity_backed(code).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    #[pyo3(signature = (value, strict = false))]
    fn py_from_str(value: &str, strict: bool) -> PyResult<Self> {
        match Self::from_str(value) {
            Ok(currency) => Ok(currency),
            Err(e) => {
                if strict {
                    Err(to_pyvalue_err(e))
                } else {
                    Self::new_checked(value, 8, 0, value, CurrencyType::Crypto)
                        .map_err(to_pyvalue_err)
                }
            }
        }
    }

    #[staticmethod]
    #[pyo3(name = "register")]
    #[pyo3(signature = (currency, overwrite = false))]
    fn py_register(currency: Self, overwrite: bool) -> PyResult<()> {
        Self::register(currency, overwrite).map_err(to_pyruntime_err)
    }
}

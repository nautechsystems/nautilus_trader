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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::python::common::EnumIterator;
use pyo3::{prelude::*, types::PyType, PyTypeInfo};

use crate::enums::{LogColor, LogLevel};

#[pymethods]
impl LogLevel {
    #[new]
    fn py_new(py: Python<'_>, value: &PyAny) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(LogLevel),
            self.name(),
            self.value(),
        )
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &PyType, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &PyType, data: &PyAny) -> PyResult<Self> {
        let data_str: &str = data.str().and_then(|s| s.extract())?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "OFF")]
    fn py_off() -> Self {
        Self::Off
    }

    #[classattr]
    #[pyo3(name = "DEBUG")]
    fn py_debug() -> Self {
        Self::Debug
    }

    #[classattr]
    #[pyo3(name = "INFO")]
    fn py_info() -> Self {
        Self::Info
    }

    #[classattr]
    #[pyo3(name = "WARNING")]
    fn py_warning() -> Self {
        Self::Warning
    }

    #[classattr]
    #[pyo3(name = "ERROR")]
    fn py_error() -> Self {
        Self::Error
    }
}

#[pymethods]
impl LogColor {
    #[new]
    fn py_new(py: Python<'_>, value: &PyAny) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(LogColor),
            self.name(),
            self.value(),
        )
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &PyType, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &PyType, data: &PyAny) -> PyResult<Self> {
        let data_str: &str = data.str().and_then(|s| s.extract())?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NORMAL")]
    fn py_normal() -> Self {
        Self::Normal
    }

    #[classattr]
    #[pyo3(name = "GREEN")]
    fn py_green() -> Self {
        Self::Green
    }

    #[classattr]
    #[pyo3(name = "BLUE")]
    fn py_blue() -> Self {
        Self::Blue
    }

    #[classattr]
    #[pyo3(name = "MAGENTA")]
    fn py_magenta() -> Self {
        Self::Magenta
    }

    #[classattr]
    #[pyo3(name = "CYAN")]
    fn py_cyan() -> Self {
        Self::Cyan
    }

    #[classattr]
    #[pyo3(name = "YELLOW")]
    fn py_error() -> Self {
        Self::Yellow
    }

    #[classattr]
    #[pyo3(name = "RED")]
    fn py_red() -> Self {
        Self::Red
    }
}

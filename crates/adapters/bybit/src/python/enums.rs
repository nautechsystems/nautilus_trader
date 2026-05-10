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

//! Bybit enumerations Python bindings.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};
use strum::IntoEnumIterator;

use crate::common::enums::{
    BybitAccountType, BybitEnvironment, BybitMarginAction, BybitMarginMode, BybitPositionIdx,
    BybitPositionMode, BybitProductType,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitProductType {
    /// Product categories supported by the v5 API.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(BybitProductType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> String {
        self.to_string().to_lowercase()
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitEnvironment {
    /// Environments supported by the Bybit API stack.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: {}>",
            stringify!(BybitEnvironment),
            self.name(),
            *self as u8,
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> String {
        self.to_string().to_lowercase()
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitAccountType {
    /// Account type enumeration.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: {}>",
            stringify!(BybitAccountType),
            self.name(),
            *self as u8,
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> String {
        self.to_string().to_uppercase()
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitMarginMode {
    /// Margin mode used by Bybit when switching risk profiles.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(BybitMarginMode),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> &'static str {
        match self {
            Self::IsolatedMargin => "ISOLATED_MARGIN",
            Self::RegularMargin => "REGULAR_MARGIN",
            Self::PortfolioMargin => "PORTFOLIO_MARGIN",
        }
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitPositionMode {
    /// Position mode as returned by the v5 API.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: {}>",
            stringify!(BybitPositionMode),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> i32 {
        *self as i32
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        // Try to extract as integer first (for API payloads that send 0 or 3)
        if let Ok(int_val) = data.extract::<i32>() {
            return match int_val {
                0 => Ok(Self::MergedSingle),
                3 => Ok(Self::BothSides),
                _ => Err(to_pyvalue_err(anyhow::anyhow!(
                    "Invalid BybitPositionMode value: {int_val}"
                ))),
            };
        }

        // Fall back to string parsing for variant names
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitPositionIdx {
    /// Position index values used for hedge mode payloads.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: {}>",
            stringify!(BybitPositionIdx),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> i32 {
        *self as i32
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(int_val) = data.extract::<i32>() {
            return match int_val {
                0 => Ok(Self::OneWay),
                1 => Ok(Self::BuyHedge),
                2 => Ok(Self::SellHedge),
                _ => Err(to_pyvalue_err(anyhow::anyhow!(
                    "Invalid BybitPositionIdx value: {int_val}"
                ))),
            };
        }

        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitMarginAction {
    /// Margin actions for spot margin trading operations.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(BybitMarginAction),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> &str {
        self.as_ref()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> String {
        self.to_string()
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }
}

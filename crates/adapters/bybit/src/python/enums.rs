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

//! Bybit enumerations Python bindings.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};
use strum::IntoEnumIterator;

use crate::common::enums::{
    BybitAccountType, BybitEnvironment, BybitMarginAction, BybitMarginMode, BybitPositionMode,
    BybitProductType,
};

#[pymethods]
impl BybitProductType {
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

    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "LINEAR")]
    fn py_linear() -> Self {
        Self::Linear
    }

    #[classattr]
    #[pyo3(name = "INVERSE")]
    fn py_inverse() -> Self {
        Self::Inverse
    }

    #[classattr]
    #[pyo3(name = "OPTION")]
    fn py_option() -> Self {
        Self::Option
    }
}

#[pymethods]
impl BybitEnvironment {
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

    #[classattr]
    #[pyo3(name = "MAINNET")]
    fn py_mainnet() -> Self {
        Self::Mainnet
    }

    #[classattr]
    #[pyo3(name = "DEMO")]
    fn py_demo() -> Self {
        Self::Demo
    }

    #[classattr]
    #[pyo3(name = "TESTNET")]
    fn py_testnet() -> Self {
        Self::Testnet
    }
}

#[pymethods]
impl BybitAccountType {
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

    #[classattr]
    #[pyo3(name = "UNIFIED")]
    fn py_unified() -> Self {
        Self::Unified
    }
}

#[pymethods]
impl BybitMarginMode {
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
    pub fn value(&self) -> String {
        match self {
            Self::IsolatedMargin => "ISOLATED_MARGIN".to_string(),
            Self::RegularMargin => "REGULAR_MARGIN".to_string(),
            Self::PortfolioMargin => "PORTFOLIO_MARGIN".to_string(),
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

    #[classattr]
    #[pyo3(name = "ISOLATED_MARGIN")]
    fn py_isolated_margin() -> Self {
        Self::IsolatedMargin
    }

    #[classattr]
    #[pyo3(name = "REGULAR_MARGIN")]
    fn py_regular_margin() -> Self {
        Self::RegularMargin
    }

    #[classattr]
    #[pyo3(name = "PORTFOLIO_MARGIN")]
    fn py_portfolio_margin() -> Self {
        Self::PortfolioMargin
    }
}

#[pymethods]
impl BybitPositionMode {
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

    #[classattr]
    #[pyo3(name = "MERGED_SINGLE")]
    fn py_merged_single() -> Self {
        Self::MergedSingle
    }

    #[classattr]
    #[pyo3(name = "BOTH_SIDES")]
    fn py_both_sides() -> Self {
        Self::BothSides
    }
}

#[pymethods]
impl BybitMarginAction {
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

    #[classattr]
    #[pyo3(name = "BORROW")]
    fn py_borrow() -> Self {
        Self::Borrow
    }

    #[classattr]
    #[pyo3(name = "REPAY")]
    fn py_repay() -> Self {
        Self::Repay
    }

    #[classattr]
    #[pyo3(name = "GET_BORROW_AMOUNT")]
    fn py_get_borrow_amount() -> Self {
        Self::GetBorrowAmount
    }
}

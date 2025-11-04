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

//! dYdX enumerations Python bindings.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};
use strum::IntoEnumIterator;

use crate::http::models::{
    DydxCandleResolution, DydxFillType, DydxFundingOrderSide, DydxLiquidity, DydxMarketStatus,
    DydxMarketType, DydxOrderSide, DydxOrderStatus, DydxOrderType, DydxPositionSide,
    DydxPositionStatus, DydxTimeInForce, DydxTradeType, DydxTradingRewardAggregationPeriod,
    DydxTransferType,
};

// =============================================================================
// Market Data Enums
// =============================================================================

#[pymethods]
impl DydxMarketStatus {
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
            stringify!(DydxMarketStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxTradeType {
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
            stringify!(DydxTradeType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxCandleResolution {
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
            stringify!(DydxCandleResolution),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Position Enums
// =============================================================================

#[pymethods]
impl DydxPositionStatus {
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
            stringify!(DydxPositionStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxPositionSide {
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
            stringify!(DydxPositionSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Order Enums
// =============================================================================

#[pymethods]
impl DydxOrderSide {
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
            stringify!(DydxOrderSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxOrderType {
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
            stringify!(DydxOrderType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxOrderStatus {
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
            stringify!(DydxOrderStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxTimeInForce {
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
            stringify!(DydxTimeInForce),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Fill Enums
// =============================================================================

#[pymethods]
impl DydxLiquidity {
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
            stringify!(DydxLiquidity),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxFillType {
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
            stringify!(DydxFillType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl DydxMarketType {
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
            stringify!(DydxMarketType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Transfer Enums
// =============================================================================

#[pymethods]
impl DydxTransferType {
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
            stringify!(DydxTransferType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Funding Enums
// =============================================================================

#[pymethods]
impl DydxFundingOrderSide {
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
            stringify!(DydxFundingOrderSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

// =============================================================================
// Rewards Enums
// =============================================================================

#[pymethods]
impl DydxTradingRewardAggregationPeriod {
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
            stringify!(DydxTradingRewardAggregationPeriod),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn value(&self) -> String {
        self.as_ref().to_string()
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>) -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.extract()?;
        let tokenized = data_str.as_str();
        Self::from_str(tokenized).map_err(to_pyvalue_err)
    }
}

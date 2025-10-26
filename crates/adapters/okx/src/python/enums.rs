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

//! OKX enumerations Python bindings.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};
use strum::IntoEnumIterator;

use crate::common::enums::{
    OKXContractType, OKXInstrumentType, OKXMarginMode, OKXOrderStatus, OKXPositionMode,
    OKXTradeMode, OKXVipLevel,
};

#[pymethods]
impl OKXInstrumentType {
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
            stringify!(OKXInstrumentType),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "ANY")]
    fn py_any() -> Self {
        Self::Any
    }

    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "MARGIN")]
    fn py_margin() -> Self {
        Self::Margin
    }

    #[classattr]
    #[pyo3(name = "SWAP")]
    fn py_swap() -> Self {
        Self::Swap
    }

    #[classattr]
    #[pyo3(name = "FUTURES")]
    fn futures() -> Self {
        Self::Futures
    }

    #[classattr]
    #[pyo3(name = "OPTION")]
    fn option() -> Self {
        Self::Option
    }
}

#[pymethods]
impl OKXContractType {
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
            stringify!(OKXContractType),
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
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_cls: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: String = data.str()?.extract()?;
        Self::from_str(&data_str).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[must_use]
    fn variants() -> Vec<String> {
        Self::iter().map(|x| x.to_string()).collect()
    }

    #[classattr]
    #[pyo3(name = "NONE")]
    fn py_none() -> Self {
        Self::None
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
}

#[pymethods]
impl OKXMarginMode {
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
            stringify!(OKXMarginMode),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "NONE")]
    fn py_none() -> Self {
        Self::None
    }

    #[classattr]
    #[pyo3(name = "ISOLATED")]
    fn py_isolated() -> Self {
        Self::Isolated
    }

    #[classattr]
    #[pyo3(name = "CROSS")]
    fn py_cross() -> Self {
        Self::Cross
    }
}

#[pymethods]
impl OKXTradeMode {
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
            stringify!(OKXTradeMode),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "CASH")]
    fn py_cash() -> Self {
        Self::Cash
    }

    #[classattr]
    #[pyo3(name = "ISOLATED")]
    fn py_isolated() -> Self {
        Self::Isolated
    }

    #[classattr]
    #[pyo3(name = "CROSS")]
    fn py_cross() -> Self {
        Self::Cross
    }

    #[classattr]
    #[pyo3(name = "SPOT_ISOLATED")]
    fn py_spot_isolated() -> Self {
        Self::SpotIsolated
    }
}

#[pymethods]
impl OKXPositionMode {
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
            stringify!(OKXPositionMode),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "NET_MODE")]
    fn py_net_mode() -> Self {
        Self::NetMode
    }

    #[classattr]
    #[pyo3(name = "LONG_SHORT_MODE")]
    fn py_long_short_mode() -> Self {
        Self::LongShortMode
    }
}

#[pymethods]
impl OKXOrderStatus {
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
            stringify!(OKXOrderStatus),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "CANCELED")]
    fn py_canceled() -> Self {
        Self::Canceled
    }

    #[classattr]
    #[pyo3(name = "LIVE")]
    fn py_live() -> Self {
        Self::Live
    }

    #[classattr]
    #[pyo3(name = "EFFECTIVE")]
    fn py_effective() -> Self {
        Self::Effective
    }

    #[classattr]
    #[pyo3(name = "PARTIALLY_FILLED")]
    fn py_partially_filled() -> Self {
        Self::PartiallyFilled
    }

    #[classattr]
    #[pyo3(name = "FILLED")]
    fn py_filled() -> Self {
        Self::Filled
    }

    #[classattr]
    #[pyo3(name = "MMP_CANCELED")]
    fn py_mmp_canceled() -> Self {
        Self::MmpCanceled
    }

    #[classattr]
    #[pyo3(name = "ORDER_PLACED")]
    fn py_order_placed() -> Self {
        Self::OrderPlaced
    }
}

#[pymethods]
impl OKXVipLevel {
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
            stringify!(OKXVipLevel),
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
    pub fn value(&self) -> u8 {
        *self as u8
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
    #[pyo3(name = "VIP0")]
    fn py_vip0() -> Self {
        Self::Vip0
    }

    #[classattr]
    #[pyo3(name = "VIP1")]
    fn py_vip1() -> Self {
        Self::Vip1
    }

    #[classattr]
    #[pyo3(name = "VIP2")]
    fn py_vip2() -> Self {
        Self::Vip2
    }

    #[classattr]
    #[pyo3(name = "VIP3")]
    fn py_vip3() -> Self {
        Self::Vip3
    }

    #[classattr]
    #[pyo3(name = "VIP4")]
    fn py_vip4() -> Self {
        Self::Vip4
    }

    #[classattr]
    #[pyo3(name = "VIP5")]
    fn py_vip5() -> Self {
        Self::Vip5
    }

    #[classattr]
    #[pyo3(name = "VIP6")]
    fn py_vip6() -> Self {
        Self::Vip6
    }

    #[classattr]
    #[pyo3(name = "VIP7")]
    fn py_vip7() -> Self {
        Self::Vip7
    }

    #[classattr]
    #[pyo3(name = "VIP8")]
    fn py_vip8() -> Self {
        Self::Vip8
    }

    #[classattr]
    #[pyo3(name = "VIP9")]
    fn py_vip9() -> Self {
        Self::Vip9
    }
}

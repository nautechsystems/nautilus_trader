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
use pyo3::{prelude::*, types::PyType, PyTypeInfo};

use crate::databento::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

#[pymethods]
impl DatabentoStatisticType {
    #[new]
    fn py_new(py: Python<'_>, value: &PyAny) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(t, value).map_err(to_pyvalue_err)
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
            stringify!(DatabentoStatisticType),
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

    // #[classmethod]
    // fn variants(_: &PyType, py: Python<'_>) -> EnumIterator {
    //     EnumIterator::new::<Self>(py)
    // }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &PyType, data: &PyAny) -> PyResult<Self> {
        let data_str: &str = data.str().and_then(|s| s.extract())?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
    #[classattr]
    #[pyo3(name = "OPENING_PRICE")]
    fn py_opening_price() -> Self {
        Self::OpeningPrice
    }

    #[classattr]
    #[pyo3(name = "INDICATIVE_OPENING_PRICE")]
    fn py_indicative_opening_price() -> Self {
        Self::IndicativeOpeningPrice
    }

    #[classattr]
    #[pyo3(name = "SETTLEMENT_PRICE")]
    fn py_settlement_price() -> Self {
        Self::SettlementPrice
    }

    #[classattr]
    #[pyo3(name = "TRADING_SESSION_LOW_PRICE")]
    fn py_trading_session_low_price() -> Self {
        Self::TradingSessionLowPrice
    }

    #[classattr]
    #[pyo3(name = "TRADING_SESSION_HIGH_PRICE")]
    fn py_trading_session_high_price() -> Self {
        Self::TradingSessionHighPrice
    }

    #[classattr]
    #[pyo3(name = "CLEARED_VOLUME")]
    fn py_cleared_volume() -> Self {
        Self::ClearedVolume
    }

    #[classattr]
    #[pyo3(name = "LOWEST_OFFER")]
    fn py_lowest_offer() -> Self {
        Self::LowestOffer
    }

    #[classattr]
    #[pyo3(name = "HIGHEST_BID")]
    fn py_highest_bid() -> Self {
        Self::HighestBid
    }

    #[classattr]
    #[pyo3(name = "OPEN_INTEREST")]
    fn py_open_interest() -> Self {
        Self::OpenInterest
    }

    #[classattr]
    #[pyo3(name = "FIXING_PRICE")]
    fn py_fixing_price() -> Self {
        Self::FixingPrice
    }

    #[classattr]
    #[pyo3(name = "CLOSE_PRICE")]
    fn py_close_price() -> Self {
        Self::ClosePrice
    }

    #[classattr]
    #[pyo3(name = "NET_CHANGE")]
    fn py_net_change() -> Self {
        Self::NetChange
    }

    #[classattr]
    #[pyo3(name = "VWAP")]
    fn py_vwap() -> Self {
        Self::Vwap
    }
}

#[pymethods]
impl DatabentoStatisticUpdateAction {
    #[new]
    fn py_new(py: Python<'_>, value: &PyAny) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(t, value).map_err(to_pyvalue_err)
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
            stringify!(DatabentoStatisticUpdateAction),
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

    // #[classmethod]
    // fn variants(_: &PyType, py: Python<'_>) -> EnumIterator {
    //     EnumIterator::new::<Self>(py)
    // }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &PyType, data: &PyAny) -> PyResult<Self> {
        let data_str: &str = data.str().and_then(|s| s.extract())?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
    #[classattr]
    #[pyo3(name = "ADDED")]
    fn py_added() -> Self {
        Self::Added
    }

    #[classattr]
    #[pyo3(name = "DELETED")]
    fn py_deleted() -> Self {
        Self::Deleted
    }
}

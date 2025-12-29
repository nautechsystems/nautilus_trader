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

//! Deribit enumerations Python bindings.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};
use strum::IntoEnumIterator;

use crate::{
    http::models::{DeribitCurrency, DeribitInstrumentKind, DeribitOptionType},
    websocket::enums::{DeribitUpdateInterval, DeribitWsChannel},
};

#[pymethods]
impl DeribitCurrency {
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
            stringify!(DeribitCurrency),
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
    #[pyo3(name = "BTC")]
    fn py_btc() -> Self {
        Self::BTC
    }

    #[classattr]
    #[pyo3(name = "ETH")]
    fn py_eth() -> Self {
        Self::ETH
    }

    #[classattr]
    #[pyo3(name = "USDC")]
    fn py_usdc() -> Self {
        Self::USDC
    }

    #[classattr]
    #[pyo3(name = "USDT")]
    fn py_usdt() -> Self {
        Self::USDT
    }

    #[classattr]
    #[pyo3(name = "EURR")]
    fn py_eurr() -> Self {
        Self::EURR
    }
}

#[pymethods]
impl DeribitInstrumentKind {
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
            stringify!(DeribitInstrumentKind),
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
    #[pyo3(name = "FUTURE")]
    fn py_future() -> Self {
        Self::Future
    }

    #[classattr]
    #[pyo3(name = "OPTION")]
    fn py_option() -> Self {
        Self::Option
    }

    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "FUTURE_COMBO")]
    fn py_future_combo() -> Self {
        Self::FutureCombo
    }

    #[classattr]
    #[pyo3(name = "OPTION_COMBO")]
    fn py_option_combo() -> Self {
        Self::OptionCombo
    }
}

#[pymethods]
impl DeribitOptionType {
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
            stringify!(DeribitOptionType),
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
    #[pyo3(name = "CALL")]
    fn py_call() -> Self {
        Self::Call
    }

    #[classattr]
    #[pyo3(name = "PUT")]
    fn py_put() -> Self {
        Self::Put
    }
}

#[pymethods]
impl DeribitUpdateInterval {
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
            stringify!(DeribitUpdateInterval),
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
    #[pyo3(name = "RAW")]
    fn py_raw() -> Self {
        Self::Raw
    }

    #[classattr]
    #[pyo3(name = "MS100")]
    fn py_ms100() -> Self {
        Self::Ms100
    }

    #[classattr]
    #[pyo3(name = "AGG2")]
    fn py_agg2() -> Self {
        Self::Agg2
    }
}

#[pymethods]
impl DeribitWsChannel {
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
            stringify!(DeribitWsChannel),
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
    #[pyo3(name = "TRADES")]
    fn py_trades() -> Self {
        Self::Trades
    }

    #[classattr]
    #[pyo3(name = "BOOK")]
    fn py_book() -> Self {
        Self::Book
    }

    #[classattr]
    #[pyo3(name = "TICKER")]
    fn py_ticker() -> Self {
        Self::Ticker
    }

    #[classattr]
    #[pyo3(name = "QUOTE")]
    fn py_quote() -> Self {
        Self::Quote
    }

    #[classattr]
    #[pyo3(name = "PRICE_INDEX")]
    fn py_price_index() -> Self {
        Self::PriceIndex
    }

    #[classattr]
    #[pyo3(name = "PRICE_RANKING")]
    fn py_price_ranking() -> Self {
        Self::PriceRanking
    }

    #[classattr]
    #[pyo3(name = "VOLATILITY_INDEX")]
    fn py_volatility_index() -> Self {
        Self::VolatilityIndex
    }

    #[classattr]
    #[pyo3(name = "ESTIMATED_EXPIRATION_PRICE")]
    fn py_estimated_expiration_price() -> Self {
        Self::EstimatedExpirationPrice
    }

    #[classattr]
    #[pyo3(name = "PERPETUAL")]
    fn py_perpetual() -> Self {
        Self::Perpetual
    }

    #[classattr]
    #[pyo3(name = "MARK_PRICE_OPTIONS")]
    fn py_mark_price_options() -> Self {
        Self::MarkPriceOptions
    }

    #[classattr]
    #[pyo3(name = "PLATFORM_STATE")]
    fn py_platform_state() -> Self {
        Self::PlatformState
    }

    #[classattr]
    #[pyo3(name = "ANNOUNCEMENTS")]
    fn py_announcements() -> Self {
        Self::Announcements
    }

    #[classattr]
    #[pyo3(name = "CHART_TRADES")]
    fn py_chart_trades() -> Self {
        Self::ChartTrades
    }

    #[classattr]
    #[pyo3(name = "USER_ORDERS")]
    fn py_user_orders() -> Self {
        Self::UserOrders
    }

    #[classattr]
    #[pyo3(name = "USER_TRADES")]
    fn py_user_trades() -> Self {
        Self::UserTrades
    }

    #[classattr]
    #[pyo3(name = "USER_PORTFOLIO")]
    fn py_user_portfolio() -> Self {
        Self::UserPortfolio
    }

    #[classattr]
    #[pyo3(name = "USER_CHANGES")]
    fn py_user_changes() -> Self {
        Self::UserChanges
    }

    #[classattr]
    #[pyo3(name = "USER_ACCESS_LOG")]
    fn py_user_access_log() -> Self {
        Self::UserAccessLog
    }
}

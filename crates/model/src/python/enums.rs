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

//! Enumerations for the trading domain model.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, exceptions::PyValueError, prelude::*, types::PyType};

use crate::{
    enums::{
        AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BetSide,
        BookAction, BookType, ContingencyType, CurrencyType, InstrumentClass, InstrumentCloseType,
        LiquiditySide, MarketStatus, MarketStatusAction, OmsType, OptionKind, OrderSide,
        OrderStatus, OrderType, PositionSide, PriceType, RecordFlag, TimeInForce, TradingState,
        TrailingOffsetType, TriggerType,
    },
    python::common::EnumIterator,
};

#[pymethods]
impl AccountType {
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
            stringify!(AccountType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "CASH")]
    fn py_cash() -> Self {
        Self::Cash
    }

    #[classattr]
    #[pyo3(name = "MARGIN")]
    fn py_margin() -> Self {
        Self::Margin
    }

    #[classattr]
    #[pyo3(name = "BETTING")]
    fn py_betting() -> Self {
        Self::Betting
    }
}

#[pymethods]
impl AggregationSource {
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
            stringify!(AggregationSource),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "EXTERNAL")]
    fn py_external() -> Self {
        Self::External
    }

    #[classattr]
    #[pyo3(name = "INTERNAL")]
    fn py_internal() -> Self {
        Self::Internal
    }
}

#[pymethods]
impl AggressorSide {
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
            stringify!(AggressorSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_AGGRESSOR")]
    fn py_no_aggressor() -> Self {
        Self::NoAggressor
    }

    #[classattr]
    #[pyo3(name = "BUYER")]
    fn py_buyer() -> Self {
        Self::Buyer
    }

    #[classattr]
    #[pyo3(name = "SELLER")]
    fn py_seller() -> Self {
        Self::Seller
    }
}

#[pymethods]
impl AssetClass {
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
            stringify!(AssetClass),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "FX")]
    fn py_fx() -> Self {
        Self::FX
    }

    #[classattr]
    #[pyo3(name = "EQUITY")]
    fn py_equity() -> Self {
        Self::Equity
    }

    #[classattr]
    #[pyo3(name = "COMMODITY")]
    fn py_commodity() -> Self {
        Self::Commodity
    }

    #[classattr]
    #[pyo3(name = "DEBT")]
    fn py_debt() -> Self {
        Self::Debt
    }

    #[classattr]
    #[pyo3(name = "INDEX")]
    fn py_index() -> Self {
        Self::Index
    }

    #[classattr]
    #[pyo3(name = "CRYPTOCURRENCY")]
    fn py_cryptocurrency() -> Self {
        Self::Cryptocurrency
    }

    #[classattr]
    #[pyo3(name = "ALTERNATIVE")]
    fn py_alternative() -> Self {
        Self::Alternative
    }
}

#[pymethods]
impl InstrumentClass {
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
            stringify!(InstrumentClass),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "SWAP")]
    fn py_swap() -> Self {
        Self::Swap
    }

    #[classattr]
    #[pyo3(name = "FUTURE")]
    fn py_future() -> Self {
        Self::Future
    }

    #[classattr]
    #[pyo3(name = "FORWARD")]
    fn py_forward() -> Self {
        Self::Forward
    }

    #[classattr]
    #[pyo3(name = "CFD")]
    fn py_cfd() -> Self {
        Self::Cfd
    }

    #[classattr]
    #[pyo3(name = "BOND")]
    fn py_bond() -> Self {
        Self::Bond
    }

    #[classattr]
    #[pyo3(name = "OPTION")]
    fn py_option() -> Self {
        Self::Option
    }

    #[classattr]
    #[pyo3(name = "WARRANT")]
    fn py_warrant() -> Self {
        Self::Warrant
    }

    #[classattr]
    #[pyo3(name = "SPORTS_BETTING")]
    fn py_sports_betting() -> Self {
        Self::SportsBetting
    }
}

#[pymethods]
impl BarAggregation {
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
            stringify!(BarAggregation),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
    #[classattr]
    #[pyo3(name = "TICK")]
    fn py_tick() -> Self {
        Self::Tick
    }

    #[classattr]
    #[pyo3(name = "TICK_IMBALANCE")]
    fn py_tick_imbalance() -> Self {
        Self::TickImbalance
    }

    #[classattr]
    #[pyo3(name = "TICK_RUNS")]
    fn py_tick_runs() -> Self {
        Self::TickRuns
    }

    #[classattr]
    #[pyo3(name = "VOLUME")]
    fn py_volume() -> Self {
        Self::Volume
    }

    #[classattr]
    #[pyo3(name = "VOLUME_IMBALANCE")]
    fn py_volume_imbalance() -> Self {
        Self::VolumeImbalance
    }

    #[classattr]
    #[pyo3(name = "VOLUME_RUNS")]
    fn py_volume_runs() -> Self {
        Self::VolumeRuns
    }

    #[classattr]
    #[pyo3(name = "VALUE")]
    fn py_value() -> Self {
        Self::Value
    }

    #[classattr]
    #[pyo3(name = "VALUE_IMBALANCE")]
    fn py_value_imbalance() -> Self {
        Self::ValueImbalance
    }

    #[classattr]
    #[pyo3(name = "VALUE_RUNS")]
    fn py_value_runs() -> Self {
        Self::ValueRuns
    }

    #[classattr]
    #[pyo3(name = "MILLISECOND")]
    fn py_millisecond() -> Self {
        Self::Millisecond
    }

    #[classattr]
    #[pyo3(name = "SECOND")]
    fn py_second() -> Self {
        Self::Second
    }

    #[classattr]
    #[pyo3(name = "MINUTE")]
    fn py_minute() -> Self {
        Self::Minute
    }

    #[classattr]
    #[pyo3(name = "HOUR")]
    fn py_hour() -> Self {
        Self::Hour
    }

    #[classattr]
    #[pyo3(name = "DAY")]
    fn py_day() -> Self {
        Self::Day
    }

    #[classattr]
    #[pyo3(name = "WEEK")]
    fn py_week() -> Self {
        Self::Week
    }

    #[classattr]
    #[pyo3(name = "MONTH")]
    fn py_month() -> Self {
        Self::Month
    }
}

#[pymethods]
impl BetSide {
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
            stringify!(BetSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classmethod]
    #[pyo3(name = "from_order_side")]
    fn py_from_order_side(_: &Bound<'_, PyType>, order_side: OrderSide) -> Self {
        order_side.into()
    }

    #[classattr]
    #[pyo3(name = "BACK")]
    fn py_back() -> Self {
        Self::Back
    }

    #[classattr]
    #[pyo3(name = "LAY")]
    fn py_lay() -> Self {
        Self::Lay
    }

    #[pyo3(name = "opposite")]
    fn py_opposite(&self) -> Self {
        self.opposite()
    }
}

#[pymethods]
impl BookAction {
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
            stringify!(BookAction),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "ADD")]
    fn py_add() -> Self {
        Self::Add
    }

    #[classattr]
    #[pyo3(name = "UPDATE")]
    fn py_update() -> Self {
        Self::Update
    }

    #[classattr]
    #[pyo3(name = "DELETE")]
    fn py_delete() -> Self {
        Self::Delete
    }

    #[classattr]
    #[pyo3(name = "CLEAR")]
    fn py_clear() -> Self {
        Self::Clear
    }
}

#[pymethods]
impl ContingencyType {
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
            stringify!(ContingencyType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_CONTINGENCY")]
    fn py_no_contingency() -> Self {
        Self::NoContingency
    }

    #[classattr]
    #[pyo3(name = "OCO")]
    fn py_oco() -> Self {
        Self::Oco
    }

    #[classattr]
    #[pyo3(name = "OTO")]
    fn py_oto() -> Self {
        Self::Oto
    }

    #[classattr]
    #[pyo3(name = "OUO")]
    fn py_ouo() -> Self {
        Self::Ouo
    }
}

#[pymethods]
impl CurrencyType {
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
            stringify!(CurrencyType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "CRYPTO")]
    fn py_crypto() -> Self {
        Self::Crypto
    }

    #[classattr]
    #[pyo3(name = "FIAT")]
    fn py_fiat() -> Self {
        Self::Fiat
    }

    #[classattr]
    #[pyo3(name = "COMMODITY_BACKED")]
    fn py_commodity_backed() -> Self {
        Self::CommodityBacked
    }
}

#[pymethods]
impl InstrumentCloseType {
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
            stringify!(InstrumentCloseType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "END_OF_SESSION")]
    fn py_end_of_session() -> Self {
        Self::EndOfSession
    }

    #[classattr]
    #[pyo3(name = "CONTRACT_EXPIRED")]
    fn py_contract_expired() -> Self {
        Self::ContractExpired
    }
}

#[pymethods]
impl LiquiditySide {
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
            stringify!(LiquditySide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_LIQUIDITY_SIDE")]
    fn py_no_liquidity_side() -> Self {
        Self::NoLiquiditySide
    }

    #[classattr]
    #[pyo3(name = "MAKER")]
    fn py_maker() -> Self {
        Self::Maker
    }

    #[classattr]
    #[pyo3(name = "TAKER")]
    fn py_taker() -> Self {
        Self::Taker
    }
}

#[pymethods]
impl MarketStatus {
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
            stringify!(MarketStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "Open")]
    fn py_open() -> Self {
        Self::Open
    }

    #[classattr]
    #[pyo3(name = "CLOSED")]
    fn py_closed() -> Self {
        Self::Closed
    }

    #[classattr]
    #[pyo3(name = "PAUSED")]
    fn py_paused() -> Self {
        Self::Paused
    }

    // # TODO: Unfortunately can't use this yet due to Cython (C enum namespacing)
    // #[classattr]
    // #[pyo3(name = "HALTED")]
    // fn py_halted() -> Self {
    //     Self::Halted
    // }

    #[classattr]
    #[pyo3(name = "SUSPENDED")]
    fn py_suspended() -> Self {
        Self::Suspended
    }

    #[classattr]
    #[pyo3(name = "NOT_AVAILABLE")]
    fn py_not_available() -> Self {
        Self::NotAvailable
    }
}

#[pymethods]
impl MarketStatusAction {
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
            stringify!(MarketStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NONE")]
    fn py_none() -> Self {
        Self::None
    }

    #[classattr]
    #[pyo3(name = "PRE_OPEN")]
    fn py_pre_open() -> Self {
        Self::PreOpen
    }

    #[classattr]
    #[pyo3(name = "PRE_CROSS")]
    fn py_pre_cross() -> Self {
        Self::Pause
    }

    #[classattr]
    #[pyo3(name = "QUOTING")]
    fn py_quoting() -> Self {
        Self::Quoting
    }

    #[classattr]
    #[pyo3(name = "CROSS")]
    fn py_cross() -> Self {
        Self::Cross
    }

    #[classattr]
    #[pyo3(name = "ROTATION")]
    fn py_rotation() -> Self {
        Self::Rotation
    }

    #[classattr]
    #[pyo3(name = "NEW_PRICE_INDICATION")]
    fn py_new_price_indication() -> Self {
        Self::NewPriceIndication
    }

    #[classattr]
    #[pyo3(name = "TRADING")]
    fn py_trading() -> Self {
        Self::Trading
    }

    #[classattr]
    #[pyo3(name = "HALT")]
    fn py_halt() -> Self {
        Self::Halt
    }

    #[classattr]
    #[pyo3(name = "PAUSE")]
    fn py_pause() -> Self {
        Self::Pause
    }

    #[classattr]
    #[pyo3(name = "SUSPEND")]
    fn py_suspend() -> Self {
        Self::Suspend
    }

    #[classattr]
    #[pyo3(name = "PRE_CLOSE")]
    fn py_pre_close() -> Self {
        Self::PreClose
    }

    #[classattr]
    #[pyo3(name = "CLOSE")]
    fn py_close() -> Self {
        Self::Close
    }

    #[classattr]
    #[pyo3(name = "POST_CLOSE")]
    fn py_post_close() -> Self {
        Self::PostClose
    }

    #[classattr]
    #[pyo3(name = "SHORT_SELL_RESTRICTION_CHANGE")]
    fn py_short_sell_restriction() -> Self {
        Self::ShortSellRestrictionChange
    }

    #[classattr]
    #[pyo3(name = "NOT_AVAILABLE_FOR_TRADING")]
    fn py_not_available_for_trading() -> Self {
        Self::NotAvailableForTrading
    }
}

#[pymethods]
impl OmsType {
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
            stringify!(OmsType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "UNSPECIFIED")]
    fn py_unspecified() -> Self {
        Self::Unspecified
    }

    #[classattr]
    #[pyo3(name = "NETTING")]
    fn py_netting() -> Self {
        Self::Netting
    }

    #[classattr]
    #[pyo3(name = "HEDGING")]
    fn py_hedging() -> Self {
        Self::Hedging
    }
}

#[pymethods]
impl OptionKind {
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
            stringify!(OptionKind),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
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
impl OrderSide {
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
            stringify!(OrderSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_ORDER_SIDE")]
    fn py_no_order_side() -> Self {
        Self::NoOrderSide
    }

    #[classattr]
    #[pyo3(name = "BUY")]
    fn py_buy() -> Self {
        Self::Buy
    }

    #[classattr]
    #[pyo3(name = "SELL")]
    fn py_sell() -> Self {
        Self::Sell
    }
}

#[pymethods]
impl OrderStatus {
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
            stringify!(OrderStatus),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "INITIALIZED")]
    fn py_initialized() -> Self {
        Self::Initialized
    }

    #[classattr]
    #[pyo3(name = "DENIED")]
    fn py_denied() -> Self {
        Self::Denied
    }

    #[classattr]
    #[pyo3(name = "EMULATED")]
    fn py_emulated() -> Self {
        Self::Emulated
    }

    #[classattr]
    #[pyo3(name = "RELEASED")]
    fn py_released() -> Self {
        Self::Released
    }

    #[classattr]
    #[pyo3(name = "SUBMITTED")]
    fn py_submitted() -> Self {
        Self::Submitted
    }

    #[classattr]
    #[pyo3(name = "ACCEPTED")]
    fn py_accepted() -> Self {
        Self::Accepted
    }

    #[classattr]
    #[pyo3(name = "REJECTED")]
    fn py_rejected() -> Self {
        Self::Rejected
    }

    #[classattr]
    #[pyo3(name = "CANCELED")]
    fn py_canceled() -> Self {
        Self::Canceled
    }

    #[classattr]
    #[pyo3(name = "EXPIRED")]
    fn py_expired() -> Self {
        Self::Expired
    }

    #[classattr]
    #[pyo3(name = "TRIGGERED")]
    fn py_triggered() -> Self {
        Self::Triggered
    }

    #[classattr]
    #[pyo3(name = "PENDING_UPDATE")]
    fn py_pending_update() -> Self {
        Self::PendingUpdate
    }

    #[classattr]
    #[pyo3(name = "PENDING_CANCEL")]
    fn py_pending_cancel() -> Self {
        Self::PendingCancel
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
}

#[pymethods]
impl OrderType {
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
            stringify!(OrderType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "MARKET")]
    fn py_market() -> Self {
        Self::Market
    }

    #[classattr]
    #[pyo3(name = "LIMIT")]
    fn py_limit() -> Self {
        Self::Limit
    }

    #[classattr]
    #[pyo3(name = "STOP_MARKET")]
    fn py_stop_market() -> Self {
        Self::StopMarket
    }

    #[classattr]
    #[pyo3(name = "STOP_LIMIT")]
    fn py_stop_limit() -> Self {
        Self::StopLimit
    }

    #[classattr]
    #[pyo3(name = "MARKET_TO_LIMIT")]
    fn py_market_to_limit() -> Self {
        Self::MarketToLimit
    }

    #[classattr]
    #[pyo3(name = "MARKET_IF_TOUCHED")]
    fn py_market_if_touched() -> Self {
        Self::MarketIfTouched
    }

    #[classattr]
    #[pyo3(name = "LIMIT_IF_TOUCHED")]
    fn py_limit_if_touched() -> Self {
        Self::LimitIfTouched
    }

    #[classattr]
    #[pyo3(name = "TRAILING_STOP_MARKET")]
    fn py_trailing_stop_market() -> Self {
        Self::TrailingStopMarket
    }

    #[classattr]
    #[pyo3(name = "TRAILING_STOP_LIMIT")]
    fn py_trailing_stop_limit() -> Self {
        Self::TrailingStopLimit
    }
}

#[pymethods]
impl PositionSide {
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
            stringify!(PositionSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_POSITION_SIDE")]
    fn py_no_position_side() -> Self {
        Self::NoPositionSide
    }

    #[classattr]
    #[pyo3(name = "FLAT")]
    fn py_flat() -> Self {
        Self::Flat
    }

    #[classattr]
    #[pyo3(name = "LONG")]
    fn py_long() -> Self {
        Self::Long
    }

    #[classattr]
    #[pyo3(name = "SHORT")]
    fn py_short() -> Self {
        Self::Short
    }
}

#[pymethods]
impl PriceType {
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
            stringify!(PositionSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classmethod]
    #[pyo3(name = "from_int")]
    fn py_from_int(_: &Bound<'_, PyType>, value: i32) -> PyResult<Self> {
        Self::from_repr(value as usize)
            .ok_or_else(|| PyValueError::new_err(format!("Invalid PriceType value: {value}")))
    }

    #[classattr]
    #[pyo3(name = "BID")]
    fn py_bid() -> Self {
        Self::Bid
    }

    #[classattr]
    #[pyo3(name = "ASK")]
    fn py_ask() -> Self {
        Self::Ask
    }

    #[classattr]
    #[pyo3(name = "MID")]
    fn py_mid() -> Self {
        Self::Mid
    }

    #[classattr]
    #[pyo3(name = "LAST")]
    fn py_last() -> Self {
        Self::Last
    }

    #[classattr]
    #[pyo3(name = "MARK")]
    fn py_mark() -> Self {
        Self::Mark
    }
}

#[pymethods]
impl RecordFlag {
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
            stringify!(RecordFlag),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "LAST")]
    fn py_last() -> Self {
        Self::F_LAST
    }

    #[classattr]
    #[pyo3(name = "TOB")]
    fn py_tob() -> Self {
        Self::F_TOB
    }

    #[classattr]
    #[pyo3(name = "SNAPSHOT")]
    fn py_snapshot() -> Self {
        Self::F_SNAPSHOT
    }

    #[classattr]
    #[pyo3(name = "MBP")]
    fn py_mbp() -> Self {
        Self::F_MBP
    }

    #[pyo3(name = "matches")]
    fn py_matches(&self, value: u8) -> bool {
        self.matches(value)
    }
}

#[pymethods]
impl TimeInForce {
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
            stringify!(TimeInForce),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "GTC")]
    fn py_gtc() -> Self {
        Self::Gtc
    }

    #[classattr]
    #[pyo3(name = "IOC")]
    fn py_ioc() -> Self {
        Self::Ioc
    }

    #[classattr]
    #[pyo3(name = "FOK")]
    fn py_fok() -> Self {
        Self::Fok
    }

    #[classattr]
    #[pyo3(name = "GTD")]
    fn py_gtd() -> Self {
        Self::Gtd
    }

    #[classattr]
    #[pyo3(name = "DAY")]
    fn py_day() -> Self {
        Self::Day
    }

    #[classattr]
    #[pyo3(name = "AT_THE_OPEN")]
    fn py_at_the_open() -> Self {
        Self::AtTheOpen
    }

    #[classattr]
    #[pyo3(name = "AT_THE_CLOSE")]
    fn py_at_the_close() -> Self {
        Self::AtTheClose
    }
}

#[pymethods]
impl TrailingOffsetType {
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
            stringify!(TrailingOffsetType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_TRAILING_OFFSET")]
    fn py_no_trailing_offset() -> Self {
        Self::NoTrailingOffset
    }

    #[classattr]
    #[pyo3(name = "PRICE")]
    fn py_price() -> Self {
        Self::Price
    }

    #[classattr]
    #[pyo3(name = "BASIS_POINTS")]
    fn py_basis_points() -> Self {
        Self::BasisPoints
    }

    #[classattr]
    #[pyo3(name = "TICKS")]
    fn py_ticks() -> Self {
        Self::Ticks
    }

    #[classattr]
    #[pyo3(name = "PRICE_TIER")]
    fn py_price_tier() -> Self {
        Self::PriceTier
    }
}

#[pymethods]
impl TriggerType {
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
            stringify!(TriggerType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "NO_TRIGGER")]
    fn py_no_trigger() -> Self {
        Self::NoTrigger
    }

    #[classattr]
    #[pyo3(name = "DEFAULT")]
    fn py_default() -> Self {
        Self::Default
    }

    #[classattr]
    #[pyo3(name = "BID_ASK")]
    fn py_bid_ask() -> Self {
        Self::BidAsk
    }

    #[classattr]
    #[pyo3(name = "LAST_PRICE")]
    fn py_last_trade() -> Self {
        Self::LastPrice
    }

    #[classattr]
    #[pyo3(name = "DOUBLE_LAST")]
    fn py_double_last() -> Self {
        Self::DoubleLast
    }

    #[classattr]
    #[pyo3(name = "DOUBLE_BID_ASK")]
    fn py_double_bid_ask() -> Self {
        Self::DoubleBidAsk
    }

    #[classattr]
    #[pyo3(name = "LAST_OR_BID_ASK")]
    fn py_last_or_bid_ask() -> Self {
        Self::LastOrBidAsk
    }

    #[classattr]
    #[pyo3(name = "MID_POINT")]
    fn py_mid_point() -> Self {
        Self::MidPoint
    }

    #[classattr]
    #[pyo3(name = "MARK_PRICE")]
    fn py_mark_price() -> Self {
        Self::MarkPrice
    }

    #[classattr]
    #[pyo3(name = "INDEX_PRICE")]
    fn py_index_price() -> Self {
        Self::IndexPrice
    }
}

#[pymethods]
impl BookType {
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
            stringify!(BookType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "L1_MBP")]
    fn py_l1_mbp() -> Self {
        Self::L1_MBP
    }

    #[classattr]
    #[pyo3(name = "L2_MBP")]
    fn py_l2_mbp() -> Self {
        Self::L2_MBP
    }

    #[classattr]
    #[pyo3(name = "L3_MBO")]
    fn py_l3_mbo() -> Self {
        Self::L3_MBO
    }
}

#[pymethods]
impl TradingState {
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
            stringify!(TradingState),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "ACTIVE")]
    fn py_active() -> Self {
        Self::Active
    }

    #[classattr]
    #[pyo3(name = "HALTED")]
    fn py_halted() -> Self {
        Self::Halted
    }

    #[classattr]
    #[pyo3(name = "REDUCING")]
    fn py_reducing() -> Self {
        Self::Reducing
    }
}

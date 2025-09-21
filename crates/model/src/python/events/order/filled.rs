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

use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3},
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::OrderFilled,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

#[pymethods]
impl OrderFilled {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (trader_id, strategy_id, instrument_id, client_order_id, venue_order_id, account_id, trade_id, order_side, order_type, last_qty, last_px, currency, liquidity_side, event_id, ts_event, ts_init, reconciliation, position_id=None, commission=None))]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        reconciliation: bool,
        position_id: Option<PositionId>,
        commission: Option<Money>,
    ) -> Self {
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            currency,
            liquidity_side,
            event_id,
            ts_event.into(),
            ts_init.into(),
            reconciliation,
            position_id,
            commission,
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "is_buy")]
    fn py_is_buy(&self) -> bool {
        self.is_buy()
    }

    #[getter]
    #[pyo3(name = "is_sell")]
    fn py_is_sell(&self) -> bool {
        self.is_sell()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> VenueOrderId {
        self.venue_order_id
    }

    #[getter]
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "trade_id")]
    fn py_trade_id(&self) -> TradeId {
        self.trade_id
    }

    #[getter]
    #[pyo3(name = "order_side")]
    fn py_order_side(&self) -> OrderSide {
        self.order_side
    }

    #[getter]
    #[pyo3(name = "last_qty")]
    fn py_last_qty(&self) -> Quantity {
        self.last_qty
    }

    #[getter]
    #[pyo3(name = "last_px")]
    fn py_last_px(&self) -> Price {
        self.last_px
    }

    #[getter]
    #[pyo3(name = "currency")]
    fn py_currency(&self) -> Currency {
        self.currency
    }

    #[getter]
    #[pyo3(name = "liquidity_side")]
    fn py_liquidity_side(&self) -> LiquiditySide {
        self.liquidity_side
    }

    #[getter]
    #[pyo3(name = "event_id")]
    fn py_event_id(&self) -> UUID4 {
        self.event_id
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "reconciliation")]
    fn py_reconciliation(&self) -> bool {
        self.reconciliation
    }

    #[getter]
    #[pyo3(name = "position_id")]
    fn py_position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    #[getter]
    #[pyo3(name = "commission")]
    fn py_commission(&self) -> Option<Money> {
        self.commission
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    /// Constructs an [`OrderFilled`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if deserialization from the Python dict fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    /// Converts this [`OrderFilled`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization into a Python dict fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(OrderFilled))?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("client_order_id", self.client_order_id.to_string())?;
        dict.set_item("venue_order_id", self.venue_order_id.to_string())?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("trade_id", self.trade_id.to_string())?;
        dict.set_item("order_side", self.order_side.to_string())?;
        dict.set_item("order_type", self.order_type.to_string())?;
        dict.set_item("last_qty", self.last_qty.to_string())?;
        dict.set_item("last_px", self.last_px.to_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("liquidity_side", self.liquidity_side.to_string())?;
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("reconciliation", self.reconciliation)?;
        dict.set_item("info", PyDict::new(py))?;
        match self.position_id {
            Some(position_id) => dict.set_item("position_id", position_id.to_string())?,
            None => dict.set_item("position_id", py.None())?,
        }
        match self.commission {
            Some(commission) => dict.set_item("commission", commission.to_string())?,
            None => dict.set_item("commission", py.None())?,
        }
        Ok(dict.into())
    }
}

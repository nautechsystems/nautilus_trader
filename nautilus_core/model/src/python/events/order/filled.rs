// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    time::UnixNanos,
    uuid::UUID4,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::prelude::ToPrimitive;

use crate::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::order::filled::OrderFilled,
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, trade_id::TradeId, trader_id::TraderId,
        venue_order_id::VenueOrderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[pymethods]
impl OrderFilled {
    #[allow(clippy::too_many_arguments)]
    #[new]
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
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        position_id: Option<PositionId>,
        commission: Option<Money>,
    ) -> PyResult<Self> {
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
            ts_event,
            ts_init,
            reconciliation,
            position_id,
            commission,
        )
        .map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        let position_id_str = match self.position_id {
            Some(position_id) => position_id.to_string(),
            None => "None".to_string(),
        };
        let commission_str = match self.commission {
            Some(commission) => commission.to_string(),
            None => "None".to_string(),
        };
        format!(
            "{}(\
            trader_id={}, \
            strategy_id={}, \
            instrument_id={}, \
            client_order_id={}, \
            venue_order_id={}, \
            account_id={}, \
            trade_id={}, \
            position_id={}, \
            order_side={}, \
            order_type={}, \
            last_qty={}, \
            last_px={} {}, \
            commission={}, \
            liquidity_side={}, \
            event_id={}, \
            ts_event={}, \
            ts_init={})",
            stringify!(OrderFilled),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.trade_id,
            position_id_str,
            self.order_side,
            self.order_type,
            self.last_qty,
            self.last_px,
            self.currency.code,
            commission_str,
            self.liquidity_side,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }

    fn __str__(&self) -> String {
        let position_id_str = match self.position_id {
            Some(position_id) => position_id.to_string(),
            None => "None".to_string(),
        };
        let commission_str = match self.commission {
            Some(commission) => commission.to_string(),
            None => "None".to_string(),
        };
        format!(
            "{}(\
            instrument_id={}, \
            client_order_id={}, \
            venue_order_id={}, \
            account_id={}, \
            trade_id={}, \
            position_id={}, \
            order_side={}, \
            order_type={}, \
            last_qty={}, \
            last_px={} {}, \
            commission={}, \
            liquidity_side={}, \
            ts_event={})",
            stringify!(OrderFilled),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.trade_id,
            position_id_str,
            self.order_side,
            self.order_type,
            self.last_qty,
            self.last_px,
            self.currency.code,
            commission_str,
            self.liquidity_side,
            self.ts_event
        )
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

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
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
        dict.set_item("ts_event", self.ts_event.to_u64())?;
        dict.set_item("ts_init", self.ts_init.to_u64())?;
        dict.set_item("reconciliation", self.reconciliation)?;
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

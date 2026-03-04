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

use nautilus_core::{UUID4, python::IntoPyObjectNautilusExt};
use pyo3::{basic::CompareOp, prelude::*};

use crate::{
    enums::{OrderSide, PositionSide},
    events::PositionOpened,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    types::{Currency, Price, Quantity},
};

#[pymethods]
impl PositionOpened {
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

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "position_id")]
    fn py_position_id(&self) -> PositionId {
        self.position_id
    }

    #[getter]
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "opening_order_id")]
    fn py_opening_order_id(&self) -> ClientOrderId {
        self.opening_order_id
    }

    #[getter]
    #[pyo3(name = "entry")]
    fn py_entry(&self) -> OrderSide {
        self.entry
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> PositionSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "signed_qty")]
    fn py_signed_qty(&self) -> f64 {
        self.signed_qty
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Quantity {
        self.quantity
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
    #[pyo3(name = "avg_px_open")]
    fn py_avg_px_open(&self) -> f64 {
        self.avg_px_open
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
}

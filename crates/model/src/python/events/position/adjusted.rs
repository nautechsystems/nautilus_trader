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
use rust_decimal::Decimal;

use crate::{
    enums::PositionAdjustmentType,
    events::PositionAdjusted,
    identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
    types::Money,
};

#[pymethods]
impl PositionAdjusted {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (trader_id, strategy_id, instrument_id, position_id, account_id, adjustment_type, quantity_change, pnl_change, reason, event_id, ts_event, ts_init))]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        adjustment_type: PositionAdjustmentType,
        quantity_change: Option<Decimal>,
        pnl_change: Option<Money>,
        reason: Option<String>,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            adjustment_type,
            quantity_change,
            pnl_change,
            reason.map(|s| s.as_str().into()),
            event_id,
            ts_event.into(),
            ts_init.into(),
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
    #[pyo3(name = "adjustment_type")]
    fn py_adjustment_type(&self) -> PositionAdjustmentType {
        self.adjustment_type
    }

    #[getter]
    #[pyo3(name = "quantity_change")]
    fn py_quantity_change(&self) -> Option<Decimal> {
        self.quantity_change
    }

    #[getter]
    #[pyo3(name = "pnl_change")]
    fn py_pnl_change(&self) -> Option<Money> {
        self.pnl_change
    }

    #[getter]
    #[pyo3(name = "reason")]
    fn py_reason(&self) -> Option<String> {
        self.reason.map(|r| r.to_string())
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

    /// Constructs a [`PositionAdjusted`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if deserialization from the Python dict fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    /// Converts this [`PositionAdjusted`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization into a Python dict fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(PositionAdjusted))?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("position_id", self.position_id.to_string())?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("adjustment_type", self.adjustment_type.to_string())?;
        match self.quantity_change {
            Some(quantity_change) => {
                dict.set_item("quantity_change", quantity_change.to_string())?;
            }
            None => dict.set_item("quantity_change", py.None())?,
        }
        match self.pnl_change {
            Some(pnl_change) => dict.set_item("pnl_change", pnl_change.to_string())?,
            None => dict.set_item("pnl_change", py.None())?,
        }
        match self.reason {
            Some(reason) => dict.set_item("reason", reason.to_string())?,
            None => dict.set_item("reason", py.None())?,
        }
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.into())
    }
}

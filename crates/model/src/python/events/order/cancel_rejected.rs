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

use std::str::FromStr;

use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3, to_pyvalue_err},
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use ustr::Ustr;

use crate::{
    events::OrderCancelRejected,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
};

#[pymethods]
impl OrderCancelRejected {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (trader_id, strategy_id, instrument_id, client_order_id, reason, event_id, ts_event, ts_init, reconciliation, venue_order_id=None, account_id=None))]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        reconciliation: bool,
        venue_order_id: Option<VenueOrderId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Self> {
        let reason = Ustr::from_str(reason).map_err(to_pyvalue_err)?;
        Ok(Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event.into(),
            ts_init.into(),
            reconciliation,
            venue_order_id,
            account_id,
        ))
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

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
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
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    #[getter]
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "reason")]
    fn py_reason(&self) -> String {
        self.reason.as_str().to_string()
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
        self.reconciliation != 0
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(OrderCancelRejected))?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("client_order_id", self.client_order_id.to_string())?;
        dict.set_item("reason", self.reason.as_str())?;
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("reconciliation", self.reconciliation)?;
        match self.venue_order_id {
            Some(venue_order_id) => dict.set_item("venue_order_id", venue_order_id.to_string())?,
            None => dict.set_item("venue_order_id", py.None())?,
        }
        match self.account_id {
            Some(account_id) => dict.set_item("account_id", account_id.to_string())?,
            None => dict.set_item("account_id", py.None())?,
        }
        Ok(dict.into())
    }
}

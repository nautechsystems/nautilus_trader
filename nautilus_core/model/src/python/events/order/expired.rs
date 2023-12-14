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
    events::order::expired::OrderExpired,
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
    },
};

#[pymethods]
impl OrderExpired {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        venue_order_id: Option<VenueOrderId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Self> {
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        )
        .map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderExpired),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id
                .map(|venue_order_id| format!("{}", venue_order_id))
                .unwrap_or_else(|| "None".to_string()),
            self.account_id
                .map(|account_id| format!("{}", account_id))
                .unwrap_or_else(|| "None".to_string()),
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}(instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, ts_event={})",
            stringify!(OrderExpired),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id
                .map(|venue_order_id| format!("{}", venue_order_id))
                .unwrap_or_else(|| "None".to_string()),
            self.account_id
                .map(|account_id| format!("{}", account_id))
                .unwrap_or_else(|| "None".to_string()),
            self.ts_event,
        )
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
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.to_u64())?;
        dict.set_item("ts_init", self.ts_init.to_u64())?;
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

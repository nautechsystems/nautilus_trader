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

use std::collections::HashMap;

use nautilus_core::{python::to_pyruntime_err, uuid::UUID4};
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce},
    events::order::initialized::OrderInitialized,
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{
        base::{str_hashmap_to_ustr, Order},
        market_to_limit::MarketToLimitOrder,
    },
    python::events::order::{order_event_to_pyobject, pyobject_to_order_event},
    types::quantity::Quantity,
};

#[pymethods]
impl MarketToLimitOrder {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        init_id: UUID4,
        ts_init: u64,
        expire_time: Option<u64>,
        display_qty: Option<Quantity>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<String, String>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<String>>,
    ) -> Self {
        let exec_algorithm_params = exec_algorithm_params.map(str_hashmap_to_ustr);
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force,
            expire_time.map(std::convert::Into::into),
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags.map(|vec| vec.into_iter().map(|s| Ustr::from(s.as_str())).collect()),
            init_id,
            ts_init.into(),
        )
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "events")]
    fn py_events(&self, py: Python<'_>) -> PyResult<Vec<PyObject>> {
        self.events()
            .into_iter()
            .map(|event| order_event_to_pyobject(py, event.clone()))
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "create")]
    fn py_create(init: OrderInitialized) -> PyResult<Self> {
        Ok(MarketToLimitOrder::from(init))
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, event: PyObject, py: Python<'_>) -> PyResult<()> {
        let event_any = pyobject_to_order_event(py, event).unwrap();
        self.apply(event_any).map(|_| ()).map_err(to_pyruntime_err)
    }
}

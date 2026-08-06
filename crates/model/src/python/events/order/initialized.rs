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

use indexmap::IndexMap;
use nautilus_core::{
    UUID4, UnixNanos,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3, to_pyvalue_err},
};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyDict, PyList},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    events::OrderInitialized,
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::str_indexmap_to_ustr,
    types::{Price, Quantity},
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl OrderInitialized {
    /// Represents an event where an order has been initialized.
    ///
    /// This is a seed event which can instantiate any order through a creation
    /// method. This event should contain enough information to be able to send it
    /// 'over the wire' and have a valid order created with exactly the same
    /// properties as if it had been instantiated locally.
    #[expect(clippy::too_many_arguments)]
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "domain event constructor requires multiple boolean flags"
    )]
    #[new]
    #[pyo3(signature = (trader_id, strategy_id, instrument_id, client_order_id, order_side, order_type, quantity, time_in_force, post_only, reduce_only, quote_quantity, reconciliation, event_id, ts_event, ts_init, price=None, activation_price=None, trigger_price=None, trigger_type=None, limit_offset=None, trailing_offset=None, trailing_offset_type=None, expire_time=None, display_qty=None, emulation_trigger=None, trigger_instrument_id=None, contingency_type=None, order_list_id=None, linked_order_ids=None, parent_order_id=None, exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=None))]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        reconciliation: bool,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        price: Option<Price>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_offset: Option<Decimal>,
        trailing_offset: Option<Decimal>,
        trailing_offset_type: Option<TrailingOffsetType>,
        expire_time: Option<u64>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<String>>,
    ) -> PyResult<Self> {
        Self::new_checked(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            reconciliation,
            event_id,
            ts_event.into(),
            ts_init.into(),
            price,
            activation_price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time.map(UnixNanos::from),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params.map(str_indexmap_to_ustr),
            exec_spawn_id,
            tags.map(|vec| vec.iter().map(|s| Ustr::from(s)).collect()),
        )
        .map_err(to_pyvalue_err)
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
    #[pyo3(name = "order_side")]
    fn py_order_side(&self) -> OrderSide {
        self.order_side
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "time_in_force")]
    fn py_time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    #[getter]
    #[pyo3(name = "post_only")]
    fn py_post_only(&self) -> bool {
        self.post_only
    }

    #[getter]
    #[pyo3(name = "reduce_only")]
    fn py_reduce_only(&self) -> bool {
        self.reduce_only
    }

    #[getter]
    #[pyo3(name = "quote_quantity")]
    fn py_quote_quantity(&self) -> bool {
        self.quote_quantity
    }

    #[getter]
    #[pyo3(name = "reconciliation")]
    fn py_reconciliation(&self) -> bool {
        self.reconciliation
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
    #[pyo3(name = "price")]
    fn py_price(&self) -> Option<Price> {
        self.price
    }

    #[getter]
    #[pyo3(name = "activation_price")]
    fn py_activation_price(&self) -> Option<Price> {
        self.activation_price
    }

    #[getter]
    #[pyo3(name = "trigger_price")]
    fn py_trigger_price(&self) -> Option<Price> {
        self.trigger_price
    }

    #[getter]
    #[pyo3(name = "trigger_type")]
    fn py_trigger_type(&self) -> Option<TriggerType> {
        self.trigger_type
    }

    #[getter]
    #[pyo3(name = "limit_offset")]
    fn py_limit_offset(&self) -> Option<Decimal> {
        self.limit_offset
    }

    #[getter]
    #[pyo3(name = "trailing_offset")]
    fn py_trailing_offset(&self) -> Option<Decimal> {
        self.trailing_offset
    }

    #[getter]
    #[pyo3(name = "trailing_offset_type")]
    fn py_trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        self.trailing_offset_type
    }

    #[getter]
    #[pyo3(name = "expire_time")]
    fn py_expire_time(&self) -> Option<u64> {
        self.expire_time.map(|ts| ts.as_u64())
    }

    #[getter]
    #[pyo3(name = "display_qty")]
    fn py_display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    #[getter]
    #[pyo3(name = "emulation_trigger")]
    fn py_emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    #[getter]
    #[pyo3(name = "trigger_instrument_id")]
    fn py_trigger_instrument_id(&self) -> Option<InstrumentId> {
        self.trigger_instrument_id
    }

    #[getter]
    #[pyo3(name = "contingency_type")]
    fn py_contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    #[getter]
    #[pyo3(name = "order_list_id")]
    fn py_order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    #[getter]
    #[pyo3(name = "linked_order_ids")]
    fn py_linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        self.linked_order_ids.clone()
    }

    #[getter]
    #[pyo3(name = "parent_order_id")]
    fn py_parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    #[getter]
    #[pyo3(name = "exec_algorithm_id")]
    fn py_exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    #[getter]
    #[pyo3(name = "exec_algorithm_params")]
    fn py_exec_algorithm_params(&self) -> Option<IndexMap<String, String>> {
        self.exec_algorithm_params.as_ref().map(|params| {
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect()
        })
    }

    #[getter]
    #[pyo3(name = "exec_spawn_id")]
    fn py_exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    #[getter]
    #[pyo3(name = "tags")]
    fn py_tags(&self) -> Option<Vec<String>> {
        self.tags
            .as_ref()
            .map(|tags| tags.iter().map(ToString::to_string).collect())
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(OrderInitialized))?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("client_order_id", self.client_order_id.to_string())?;
        dict.set_item("order_side", self.order_side.to_string())?;
        dict.set_item("order_type", self.order_type.to_string())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("time_in_force", self.time_in_force.to_string())?;
        dict.set_item("post_only", self.post_only)?;
        dict.set_item("reduce_only", self.reduce_only)?;
        dict.set_item("quote_quantity", self.quote_quantity)?;
        dict.set_item("reconciliation", self.reconciliation)?;
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        match self.price {
            Some(price) => dict.set_item("price", price.to_string())?,
            None => dict.set_item("price", py.None())?,
        }

        match self.activation_price {
            Some(activation_price) => {
                dict.set_item("activation_price", activation_price.to_string())?;
            }
            None => dict.set_item("activation_price", py.None())?,
        }

        match self.trigger_price {
            Some(trigger_price) => dict.set_item("trigger_price", trigger_price.to_string())?,
            None => dict.set_item("trigger_price", py.None())?,
        }

        match self.trigger_type {
            Some(trigger_type) => dict.set_item("trigger_type", trigger_type.to_string())?,
            None => dict.set_item("trigger_type", py.None())?,
        }

        match self.limit_offset {
            Some(limit_offset) => dict.set_item("limit_offset", limit_offset.to_string())?,
            None => dict.set_item("limit_offset", py.None())?,
        }

        match self.trailing_offset {
            Some(trailing_offset) => {
                dict.set_item("trailing_offset", trailing_offset.to_string())?;
            }
            None => dict.set_item("trailing_offset", py.None())?,
        }

        match self.trailing_offset_type {
            Some(trailing_offset_type) => {
                dict.set_item("trailing_offset_type", trailing_offset_type.to_string())?;
            }
            None => dict.set_item("trailing_offset_type", py.None())?,
        }

        match self.expire_time {
            Some(expire_time) => dict.set_item("expire_time", expire_time.as_u64())?,
            None => dict.set_item("expire_time", py.None())?,
        }

        match self.display_qty {
            Some(display_qty) => dict.set_item("display_qty", display_qty.to_string())?,
            None => dict.set_item("display_qty", py.None())?,
        }

        match self.emulation_trigger {
            Some(emulation_trigger) => {
                dict.set_item("emulation_trigger", emulation_trigger.to_string())?;
            }
            None => dict.set_item("emulation_trigger", py.None())?,
        }

        match self.trigger_instrument_id {
            Some(trigger_instrument_id) => {
                dict.set_item("trigger_instrument_id", trigger_instrument_id.to_string())?;
            }
            None => dict.set_item("trigger_instrument_id", py.None())?,
        }

        match self.contingency_type {
            Some(contingency_type) => {
                dict.set_item("contingency_type", contingency_type.to_string())?;
            }
            None => dict.set_item("contingency_type", py.None())?,
        }

        match self.order_list_id {
            Some(order_list_id) => dict.set_item("order_list_id", order_list_id.to_string())?,
            None => dict.set_item("order_list_id", py.None())?,
        }

        match &self.linked_order_ids {
            Some(linked_order_ids) => {
                let py_linked_order_ids = PyList::empty(py);
                for linked_order_id in linked_order_ids {
                    py_linked_order_ids.append(linked_order_id.to_string())?;
                }
                dict.set_item("linked_order_ids", py_linked_order_ids)?;
            }
            None => dict.set_item("linked_order_ids", py.None())?,
        }

        match self.parent_order_id {
            Some(parent_order_id) => {
                dict.set_item("parent_order_id", parent_order_id.to_string())?;
            }
            None => dict.set_item("parent_order_id", py.None())?,
        }

        match self.exec_algorithm_id {
            Some(exec_algorithm_id) => {
                dict.set_item("exec_algorithm_id", exec_algorithm_id.to_string())?;
            }
            None => dict.set_item("exec_algorithm_id", py.None())?,
        }

        match &self.exec_algorithm_params {
            Some(exec_algorithm_params) => {
                let py_exec_algorithm_params = PyDict::new(py);
                for (key, value) in exec_algorithm_params {
                    py_exec_algorithm_params.set_item(key.to_string(), value.to_string())?;
                }
                dict.set_item("exec_algorithm_params", py_exec_algorithm_params)?;
            }
            None => dict.set_item("exec_algorithm_params", py.None())?,
        }

        match self.exec_spawn_id {
            Some(exec_spawn_id) => dict.set_item("exec_spawn_id", exec_spawn_id.to_string())?,
            None => dict.set_item("exec_spawn_id", py.None())?,
        }

        match &self.tags {
            Some(tags) => dict.set_item(
                "tags",
                tags.iter().map(|x| x.to_string()).collect::<Vec<String>>(),
            )?,
            None => dict.set_item("tags", py.None())?,
        }

        match self.causation_id {
            Some(causation_id) => dict.set_item("causation_id", causation_id.to_string())?,
            None => dict.set_item("causation_id", py.None())?,
        }
        Ok(dict.into())
    }
}

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

use nautilus_core::{
    nanos::UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
    uuid::UUID4,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    events::order::initialized::OrderInitialized,
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{
        base::{str_hashmap_to_ustr, Order},
        stop_limit::StopLimitOrder,
    },
    python::{
        common::commissions_from_hashmap,
        events::order::{order_event_to_pyobject, pyobject_to_order_event},
    },
    types::{price::Price, quantity::Quantity},
};

#[pymethods]
impl StopLimitOrder {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        init_id: UUID4,
        ts_init: u64,
        expire_time: Option<u64>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
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
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time.map(std::convert::Into::into),
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
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

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            _ => panic!("Not implemented"),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[staticmethod]
    #[pyo3(name = "create")]
    fn py_create(init: OrderInitialized) -> PyResult<Self> {
        Ok(StopLimitOrder::from(init))
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
    #[pyo3(name = "side")]
    fn py_order_side(&self) -> OrderSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price
    }

    #[getter]
    #[pyo3(name = "trigger_price")]
    fn py_trigger_price(&self) -> Price {
        self.trigger_price
    }

    #[getter]
    #[pyo3(name = "trigger_type")]
    fn py_trigger_type(&self) -> TriggerType {
        self.trigger_type
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "time_in_force")]
    fn py_time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    #[getter]
    #[pyo3(name = "expire_time")]
    fn py_expire_time(&self) -> Option<u64> {
        self.expire_time.map(std::convert::Into::into)
    }

    #[getter]
    #[pyo3(name = "status")]
    fn py_order_status(&self) -> OrderStatus {
        self.status
    }

    #[getter]
    #[pyo3(name = "init_id")]
    fn py_init_id(&self) -> UUID4 {
        self.init_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "init_event")]
    fn py_init_event(&self, py: Python<'_>) -> PyResult<PyObject> {
        match self.init_event() {
            Some(event) => order_event_to_pyobject(py, event),
            None => Ok(py.None()),
        }
    }

    #[getter]
    #[pyo3(name = "has_price")]
    fn py_has_price(&self) -> bool {
        true
    }

    #[getter]
    #[pyo3(name = "is_passive")]
    fn py_is_passive(&self) -> bool {
        self.is_passive()
    }

    #[getter]
    #[pyo3(name = "is_aggressive")]
    fn py_is_aggressive(&self) -> bool {
        self.is_aggressive()
    }

    #[getter]
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[getter]
    #[pyo3(name = "is_open")]
    fn py_is_open(&self) -> bool {
        self.is_open()
    }

    #[getter]
    #[pyo3(name = "has_trigger_price")]
    fn py_has_trigger_price(&self) -> bool {
        true
    }

    #[getter]
    #[pyo3(name = "is_post_only")]
    fn py_post_only(&self) -> bool {
        self.is_post_only
    }

    #[getter]
    #[pyo3(name = "is_reduce_only")]
    fn py_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    #[getter]
    #[pyo3(name = "is_quote_quantity")]
    fn py_quote_quantity(&self) -> bool {
        self.is_quote_quantity
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
    fn py_exec_algorithm_params(&self) -> Option<HashMap<&str, &str>> {
        self.exec_algorithm_params.as_ref().map(|x| {
            x.into_iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
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
    fn py_tags(&self) -> Option<Vec<&str>> {
        self.tags
            .as_ref()
            .map(|vec| vec.iter().map(|s| s.as_str()).collect())
    }

    #[getter]
    #[pyo3(name = "events")]
    fn py_events(&self, py: Python<'_>) -> PyResult<Vec<PyObject>> {
        self.events()
            .into_iter()
            .map(|event| order_event_to_pyobject(py, event.clone()))
            .collect()
    }

    #[pyo3(name = "to_dict")]
    fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("client_order_id", self.client_order_id.to_string())?;
        dict.set_item("side", self.side.to_string())?;
        dict.set_item("type", self.order_type.to_string())?;
        dict.set_item("side", self.side.to_string())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("status", self.status.to_string())?;
        dict.set_item("price", self.price.to_string())?;
        dict.set_item("trigger_price", self.trigger_price.to_string())?;
        dict.set_item("trigger_type", self.trigger_type.to_string())?;
        dict.set_item("filled_qty", self.filled_qty.to_string())?;
        dict.set_item("time_in_force", self.time_in_force.to_string())?;
        dict.set_item("is_post_only", self.is_post_only)?;
        dict.set_item("is_reduce_only", self.is_reduce_only)?;
        dict.set_item("is_quote_quantity", self.is_quote_quantity)?;
        dict.set_item("init_id", self.init_id.to_string())?;
        dict.set_item(
            "expire_time_ns",
            self.expire_time.filter(|&t| t != 0).map(|t| t.as_u64()),
        )?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("ts_last", self.ts_last.as_u64())?;
        dict.set_item(
            "commissions",
            commissions_from_hashmap(py, self.commissions())?,
        )?;
        self.last_trade_id.map_or_else(
            || dict.set_item("last_trade_id", py.None()),
            |x| dict.set_item("last_trade_id", x.to_string()),
        )?;
        self.avg_px.map_or_else(
            || dict.set_item("avg_px", py.None()),
            |x| dict.set_item("avg_px", x.to_string()),
        )?;
        self.position_id.map_or_else(
            || dict.set_item("position_id", py.None()),
            |x| dict.set_item("position_id", x.to_string()),
        )?;
        self.liquidity_side.map_or_else(
            || dict.set_item("liquidity_side", py.None()),
            |x| dict.set_item("liquidity_side", x.to_string()),
        )?;
        self.slippage.map_or_else(
            || dict.set_item("slippage", py.None()),
            |x| dict.set_item("slippage", x.to_string()),
        )?;
        self.account_id.map_or_else(
            || dict.set_item("account_id", py.None()),
            |x| dict.set_item("account_id", x.to_string()),
        )?;
        self.venue_order_id.map_or_else(
            || dict.set_item("venue_order_id", py.None()),
            |x| dict.set_item("venue_order_id", x.to_string()),
        )?;
        self.display_qty.map_or_else(
            || dict.set_item("display_qty", py.None()),
            |x| dict.set_item("display_qty", x.to_string()),
        )?;
        self.emulation_trigger.map_or_else(
            || dict.set_item("emulation_trigger", py.None()),
            |x| dict.set_item("emulation_trigger", x.to_string()),
        )?;
        dict.set_item("trigger_instrument_id", self.trigger_instrument_id)?;
        self.contingency_type.map_or_else(
            || dict.set_item("contingency_type", py.None()),
            |x| dict.set_item("contingency_type", x.to_string()),
        )?;
        self.order_list_id.map_or_else(
            || dict.set_item("order_list_id", py.None()),
            |x| dict.set_item("order_list_id", x.to_string()),
        )?;
        dict.set_item(
            "linked_order_ids",
            self.linked_order_ids.as_ref().map(|x| {
                x.iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<String>>()
            }),
        )?;
        self.parent_order_id.map_or_else(
            || dict.set_item("parent_order_id", py.None()),
            |x| dict.set_item("parent_order_id", x.to_string()),
        )?;
        self.exec_algorithm_id.map_or_else(
            || dict.set_item("exec_algorithm_id", py.None()),
            |x| dict.set_item("exec_algorithm_id", x.to_string()),
        )?;
        dict.set_item(
            "exec_algorithm_params",
            self.exec_algorithm_params.as_ref().map(|x| {
                x.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<HashMap<String, String>>()
            }),
        )?;
        self.exec_spawn_id.map_or_else(
            || dict.set_item("exec_spawn_id", py.None()),
            |x| dict.set_item("exec_spawn_id", x.to_string()),
        )?;
        dict.set_item(
            "tags",
            self.tags
                .as_ref()
                .map(|vec| vec.iter().map(|s| s.to_string()).collect::<Vec<String>>()),
        )?;
        Ok(dict.into())
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        let dict = values.as_ref(py);
        let trader_id = TraderId::from(dict.get_item("trader_id")?.unwrap().extract::<&str>()?);
        let strategy_id =
            StrategyId::from(dict.get_item("strategy_id")?.unwrap().extract::<&str>()?);
        let instrument_id =
            InstrumentId::from(dict.get_item("instrument_id")?.unwrap().extract::<&str>()?);
        let client_order_id = ClientOrderId::from(
            dict.get_item("client_order_id")?
                .unwrap()
                .extract::<&str>()?,
        );
        let order_side = dict
            .get_item("side")?
            .unwrap()
            .extract::<&str>()?
            .parse::<OrderSide>()
            .unwrap();
        let quantity = Quantity::from(dict.get_item("quantity")?.unwrap().extract::<&str>()?);
        let price = Price::from(dict.get_item("price")?.unwrap().extract::<&str>()?);
        let trigger_price =
            Price::from(dict.get_item("trigger_price")?.unwrap().extract::<&str>()?);
        let trigger_type = dict
            .get_item("trigger_type")?
            .unwrap()
            .extract::<&str>()?
            .parse::<TriggerType>()
            .unwrap();
        let time_in_force = dict
            .get_item("time_in_force")?
            .unwrap()
            .extract::<&str>()?
            .parse::<TimeInForce>()
            .unwrap();
        let post_only = dict.get_item("is_post_only")?.unwrap().extract::<bool>()?;
        let reduce_only = dict
            .get_item("is_reduce_only")?
            .unwrap()
            .extract::<bool>()?;
        let quote_quantity = dict
            .get_item("is_quote_quantity")?
            .unwrap()
            .extract::<bool>()?;
        let expire_time = dict
            .get_item("expire_time")
            .map(|x| x.and_then(|x| x.extract::<u64>().ok()))
            .unwrap();
        let display_quantity = dict
            .get_item("display_qty")
            .map(|x| x.and_then(|x| x.extract::<Quantity>().ok()))
            .unwrap();
        let emulation_trigger = dict
            .get_item("emulation_trigger")
            .map(|x| x.and_then(|x| x.extract::<&str>().unwrap().parse::<TriggerType>().ok()))
            .unwrap();
        let trigger_instrument_id = dict
            .get_item("trigger_instrument_id")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(item.parse::<InstrumentId>().unwrap()),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let contingency_type = dict
            .get_item("contingency_type")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(item.parse::<ContingencyType>().unwrap()),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let order_list_id = dict
            .get_item("order_list_id")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(OrderListId::from(item)),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let linked_order_ids = dict.get_item("linked_order_ids").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<Vec<String>>();
                match extracted_str {
                    Ok(item) => Some(
                        item.iter()
                            .map(|x| ClientOrderId::from(x.as_str()))
                            .collect(),
                    ),
                    Err(_) => None,
                }
            })
        })?;
        let parent_order_id = dict
            .get_item("parent_order_id")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(ClientOrderId::from(item)),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let exec_algorithm_id = dict
            .get_item("exec_algorithm_id")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(ExecAlgorithmId::from(item)),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let exec_algorithm_params = dict.get_item("exec_algorithm_params").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<HashMap<String, String>>();
                match extracted_str {
                    Ok(item) => Some(str_hashmap_to_ustr(item)),
                    Err(_) => None,
                }
            })
        })?;
        let exec_spawn_id = dict
            .get_item("exec_spawn_id")
            .map(|x| {
                x.and_then(|x| {
                    let extracted = x.extract::<&str>();
                    match extracted {
                        Ok(item) => Some(ClientOrderId::from(item)),
                        Err(_) => None,
                    }
                })
            })
            .unwrap();
        let tags = dict.get_item("tags").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<Vec<String>>();
                match extracted_str {
                    Ok(item) => Some(item.iter().map(|s| Ustr::from(&s)).collect()),
                    Err(_) => None,
                }
            })
        })?;
        let init_id = dict
            .get_item("init_id")
            .map(|x| x.and_then(|inner| inner.extract::<&str>().unwrap().parse::<UUID4>().ok()))?
            .unwrap();
        let ts_init = dict.get_item("ts_init")?.unwrap().extract::<u64>()?;
        let stop_limit_order = Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time.map(UnixNanos::from),
            post_only,
            reduce_only,
            quote_quantity,
            display_quantity,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            init_id,
            ts_init.into(),
        );
        Ok(stop_limit_order)
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, event: PyObject, py: Python<'_>) -> PyResult<()> {
        let event_any = pyobject_to_order_event(py, event).unwrap();
        self.apply(event_any).map(|_| ()).map_err(to_pyruntime_err)
    }
}

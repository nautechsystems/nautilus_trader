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

use nautilus_core::{python::to_pyvalue_err, time::UnixNanos, uuid::UUID4};
use pyo3::{
    basic::CompareOp,
    pymethods,
    types::{PyDict, PyList},
    IntoPy, Py, PyAny, PyObject, PyResult, Python,
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, PositionSide, TimeInForce},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, strategy_id::StrategyId,
        trader_id::TraderId,
    },
    orders::{
        base::{str_hashmap_to_ustr, OrderCore},
        market::MarketOrder,
    },
    types::{currency::Currency, money::Money, quantity::Quantity},
};

#[pymethods]
impl MarketOrder {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        init_id: UUID4,
        ts_init: UnixNanos,
        time_in_force: TimeInForce,
        reduce_only: bool,
        quote_quantity: bool,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<String, String>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force,
            init_id,
            ts_init,
            reduce_only,
            quote_quantity,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params.map(str_hashmap_to_ustr),
            exec_spawn_id,
            tags.map(|s| Ustr::from(&s)),
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

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[pyo3(name = "signed_decimal_qty")]
    fn py_signed_decimal_qty(&self) -> Decimal {
        self.signed_decimal_qty()
    }

    #[pyo3(name = "would_reduce_only")]
    fn py_would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
        self.would_reduce_only(side, position_qty)
    }

    #[pyo3(name = "commission")]
    fn py_commission(&self, currency: &Currency) -> Option<Money> {
        self.commission(currency)
    }

    #[pyo3(name = "commissions")]
    fn py_commissions(&self) -> HashMap<Currency, Money> {
        self.commissions()
    }

    #[getter]
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
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
    #[pyo3(name = "init_id")]
    fn py_init_id(&self) -> UUID4 {
        self.init_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
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
    fn py_exec_algorithm_params(&self) -> Option<HashMap<String, String>> {
        self.exec_algorithm_params.clone().map(|x| {
            x.into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
    }

    #[getter]
    #[pyo3(name = "exec_spawn_id")]
    fn py_exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    #[getter]
    #[pyo3(name = "is_reduce_only")]
    fn py_is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    #[getter]
    #[pyo3(name = "is_quote_quantity")]
    fn py_is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    #[getter]
    #[pyo3(name = "contingency_type")]
    fn py_contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "emulation_trigger")]
    fn py_emulation_trigger(&self) -> Option<String> {
        self.emulation_trigger.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "time_in_force")]
    fn py_time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    #[getter]
    #[pyo3(name = "tags")]
    fn py_tags(&self) -> Option<String> {
        self.tags.map(|x| x.to_string())
    }

    #[staticmethod]
    #[pyo3(name = "opposite_side")]
    fn py_opposite_side(side: OrderSide) -> OrderSide {
        OrderCore::opposite_side(side)
    }

    #[staticmethod]
    #[pyo3(name = "closing_side")]
    fn py_closing_side(side: PositionSide) -> OrderSide {
        OrderCore::closing_side(side)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("client_order_id", self.client_order_id.to_string())?;
        dict.set_item("side", self.side.to_string())?;
        dict.set_item("type", self.order_type.to_string())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("status", self.status.to_string())?;
        dict.set_item("time_in_force", self.time_in_force.to_string())?;
        dict.set_item("is_reduce_only", self.is_reduce_only)?;
        dict.set_item("is_quote_quantity", self.is_quote_quantity)?;
        dict.set_item("filled_qty", self.filled_qty.to_string())?;
        dict.set_item("init_id", self.init_id.to_string())?;
        dict.set_item("ts_init", self.ts_init)?;
        dict.set_item("ts_last", self.ts_last)?;
        let commissions_dict = PyDict::new(py);
        for (key, value) in &self.commissions {
            commissions_dict.set_item(key.code.to_string(), value.to_string())?;
        }
        dict.set_item("commissions", commissions_dict)?;
        self.venue_order_id.map_or_else(
            || dict.set_item("venue_order_id", py.None()),
            |x| dict.set_item("venue_order_id", x.to_string()),
        )?;
        self.emulation_trigger.map_or_else(
            || dict.set_item("emulation_trigger", py.None()),
            |x| dict.set_item("emulation_trigger", x.to_string()),
        )?;
        self.contingency_type.map_or_else(
            || dict.set_item("contingency_type", py.None()),
            |x| dict.set_item("contingency_type", x.to_string()),
        )?;
        self.order_list_id.map_or_else(
            || dict.set_item("order_list_id", py.None()),
            |x| dict.set_item("order_list_id", x.to_string()),
        )?;
        self.linked_order_ids.clone().map_or_else(
            || dict.set_item("linked_order_ids", py.None()),
            |linked_order_ids| {
                let linked_order_ids_list = PyList::new(
                    py,
                    linked_order_ids
                        .iter()
                        .map(std::string::ToString::to_string),
                );
                dict.set_item("linked_order_ids", linked_order_ids_list)
            },
        )?;
        self.parent_order_id.map_or_else(
            || dict.set_item("parent_order_id", py.None()),
            |x| dict.set_item("parent_order_id", x.to_string()),
        )?;
        self.exec_algorithm_id.map_or_else(
            || dict.set_item("exec_algorithm_id", py.None()),
            |x| dict.set_item("exec_algorithm_id", x.to_string()),
        )?;
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
        self.exec_spawn_id.map_or_else(
            || dict.set_item("exec_spawn_id", py.None()),
            |x| dict.set_item("exec_spawn_id", x.to_string()),
        )?;
        self.tags.map_or_else(
            || dict.set_item("tags", py.None()),
            |x| dict.set_item("tags", x.to_string()),
        )?;
        self.account_id.map_or_else(
            || dict.set_item("account_id", py.None()),
            |x| dict.set_item("account_id", x.to_string()),
        )?;
        self.slippage.map_or_else(
            || dict.set_item("slippage", py.None()),
            |x| dict.set_item("slippage", x.to_string()),
        )?;
        self.position_id.map_or_else(
            || dict.set_item("position_id", py.None()),
            |x| dict.set_item("position_id", x.to_string()),
        )?;
        self.liquidity_side.map_or_else(
            || dict.set_item("liquidity_side", py.None()),
            |x| dict.set_item("liquidity_side", x.to_string()),
        )?;
        self.last_trade_id.map_or_else(
            || dict.set_item("last_trade_id", py.None()),
            |x| dict.set_item("last_trade_id", x.to_string()),
        )?;
        self.avg_px.map_or_else(
            || dict.set_item("avg_px", py.None()),
            |x| dict.set_item("avg_px", x.to_string()),
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
        let time_in_force = dict
            .get_item("time_in_force")?
            .unwrap()
            .extract::<&str>()?
            .parse::<TimeInForce>()
            .unwrap();
        let init_id = dict
            .get_item("init_id")
            .map(|x| x.and_then(|inner| inner.extract::<&str>().unwrap().parse::<UUID4>().ok()))?
            .unwrap();
        let ts_init = dict.get_item("ts_init")?.unwrap().extract::<UnixNanos>()?;
        let is_reduce_only = dict
            .get_item("is_reduce_only")?
            .unwrap()
            .extract::<bool>()?;
        let is_quote_quantity = dict
            .get_item("is_quote_quantity")?
            .unwrap()
            .extract::<bool>()?;
        let contingency_type = dict.get_item("contingency_type").map(|x| {
            x.and_then(|inner| {
                inner
                    .extract::<&str>()
                    .unwrap()
                    .parse::<ContingencyType>()
                    .ok()
            })
        })?;
        let order_list_id = dict.get_item("order_list_id").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<&str>();
                match extracted_str {
                    Ok(item) => item.parse::<OrderListId>().ok(),
                    Err(_) => None,
                }
            })
        })?;
        let linked_order_ids = dict.get_item("linked_order_ids").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<Vec<&str>>();
                match extracted_str {
                    Ok(item) => Some(
                        item.iter()
                            .map(|x| x.parse::<ClientOrderId>().unwrap())
                            .collect(),
                    ),
                    Err(_) => None,
                }
            })
        })?;
        let parent_order_id = dict.get_item("parent_order_id").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<&str>();
                match extracted_str {
                    Ok(item) => item.parse::<ClientOrderId>().ok(),
                    Err(_) => None,
                }
            })
        })?;
        let exec_algorithm_id = dict.get_item("exec_algorithm_id").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<&str>();
                match extracted_str {
                    Ok(item) => item.parse::<ExecAlgorithmId>().ok(),
                    Err(_) => None,
                }
            })
        })?;
        let exec_algorithm_params = dict.get_item("exec_algorithm_params").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<HashMap<String, String>>();
                match extracted_str {
                    Ok(item) => Some(str_hashmap_to_ustr(item)),
                    Err(_) => None,
                }
            })
        })?;
        let exec_spawn_id = dict.get_item("exec_spawn_id").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<&str>();
                match extracted_str {
                    Ok(item) => item.parse::<ClientOrderId>().ok(),
                    Err(_) => None,
                }
            })
        })?;
        let tags = dict.get_item("tags").map(|x| {
            x.and_then(|inner| {
                let extracted_str = inner.extract::<&str>();
                match extracted_str {
                    Ok(item) => Some(Ustr::from(item)),
                    Err(_) => None,
                }
            })
        })?;
        let market_order = Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force,
            init_id,
            ts_init,
            is_reduce_only,
            is_quote_quantity,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        )
        .unwrap();
        Ok(market_order)
    }
}

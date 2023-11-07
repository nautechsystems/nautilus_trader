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

use std::collections::HashMap;

use nautilus_core::{python::to_pyvalue_err, time::UnixNanos, uuid::UUID4};
use pyo3::prelude::*;
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, PositionSide, TimeInForce},
    identifiers::{
        client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
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
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        quantity,
        init_id,
        ts_init,
        time_in_force=TimeInForce::Gtd,
        reduce_only=false,
        quote_quantity=false,
        contingency_type=None,
        order_list_id=None,
        linked_order_ids=None,
        parent_order_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        exec_spawn_id=None,
        tags=None,
    ))]
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
        MarketOrder::new(
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
}

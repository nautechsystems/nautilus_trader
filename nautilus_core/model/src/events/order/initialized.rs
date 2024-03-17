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

use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use derive_builder::Builder;
use nautilus_core::{time::UnixNanos, uuid::UUID4};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{
        client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, strategy_id::StrategyId,
        trader_id::TraderId,
    },
    types::{price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, PartialEq, Eq, Debug, Builder, Serialize, Deserialize)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderInitialized {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub time_in_force: TimeInForce,
    pub post_only: bool,
    pub reduce_only: bool,
    pub quote_quantity: bool,
    pub reconciliation: bool,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub limit_offset: Option<Price>,
    pub trailing_offset: Option<Price>,
    pub trailing_offset_type: Option<TrailingOffsetType>,
    pub expire_time: Option<UnixNanos>,
    pub display_qty: Option<Quantity>,
    pub emulation_trigger: Option<TriggerType>,
    pub trigger_instrument_id: Option<InstrumentId>,
    pub contingency_type: Option<ContingencyType>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub exec_algorithm_params: Option<HashMap<Ustr, Ustr>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<Ustr>,
}

impl Default for OrderInitialized {
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            strategy_id: StrategyId::default(),
            instrument_id: InstrumentId::default(),
            client_order_id: ClientOrderId::default(),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(100_000.0, 0).unwrap(),
            price: Default::default(),
            trigger_price: Default::default(),
            trigger_type: Default::default(),
            time_in_force: TimeInForce::Day,
            expire_time: Default::default(),
            post_only: Default::default(),
            reduce_only: Default::default(),
            display_qty: Default::default(),
            quote_quantity: Default::default(),
            limit_offset: Default::default(),
            trailing_offset: Default::default(),
            trailing_offset_type: Default::default(),
            emulation_trigger: Default::default(),
            trigger_instrument_id: Default::default(),
            contingency_type: Default::default(),
            order_list_id: Default::default(),
            linked_order_ids: Default::default(),
            parent_order_id: Default::default(),
            exec_algorithm_id: Default::default(),
            exec_algorithm_params: Default::default(),
            exec_spawn_id: Default::default(),
            tags: Default::default(),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: Default::default(),
        }
    }
}

impl OrderInitialized {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_offset: Option<Price>,
        trailing_offset: Option<Price>,
        trailing_offset_type: Option<TrailingOffsetType>,
        expire_time: Option<UnixNanos>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Ustr>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
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
            ts_event,
            ts_init,
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time,
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
            tags,
        })
    }
}

impl Display for OrderInitialized {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderInitialized(\
            instrument_id={}, \
            client_order_id={}, \
            side={}, \
            type={}, \
            quantity={}, \
            time_in_force={}, \
            post_only={}, \
            reduce_only={}, \
            quote_quantity={}, \
            price={}, \
            emulation_trigger={}, \
            trigger_instrument_id={}, \
            contingency_type={}, \
            order_list_id={}, \
            linked_order_ids=[{}], \
            parent_order_id={}, \
            exec_algorithm_id={}, \
            exec_algorithm_params={}, \
            exec_spawn_id={}, \
            tags={})",
            self.instrument_id,
            self.client_order_id,
            self.order_side,
            self.order_type,
            self.quantity,
            self.time_in_force,
            self.post_only,
            self.reduce_only,
            self.quote_quantity,
            self.price
                .map_or("None".to_string(), |price| format!("{price}")),
            self.emulation_trigger
                .map_or("None".to_string(), |trigger| format!("{trigger}")),
            self.trigger_instrument_id
                .map_or("None".to_string(), |instrument_id| format!(
                    "{instrument_id}"
                )),
            self.contingency_type
                .map_or("None".to_string(), |contingency_type| format!(
                    "{contingency_type}"
                )),
            self.order_list_id
                .map_or("None".to_string(), |order_list_id| format!(
                    "{order_list_id}"
                )),
            self.linked_order_ids
                .as_ref()
                .map_or("None".to_string(), |linked_order_ids| linked_order_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")),
            self.parent_order_id
                .map_or("None".to_string(), |parent_order_id| format!(
                    "{parent_order_id}"
                )),
            self.exec_algorithm_id
                .map_or("None".to_string(), |exec_algorithm_id| format!(
                    "{exec_algorithm_id}"
                )),
            self.exec_algorithm_params
                .as_ref()
                .map_or("None".to_string(), |exec_algorithm_params| format!(
                    "{exec_algorithm_params:?}"
                )),
            self.exec_spawn_id
                .map_or("None".to_string(), |exec_spawn_id| format!(
                    "{exec_spawn_id}"
                )),
            self.tags
                .as_ref()
                .map_or("None".to_string(), |tags| format!("{tags}")),
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod test {
    use rstest::rstest;

    use crate::events::order::{initialized::OrderInitialized, stubs::*};
    #[rstest]
    fn test_order_initialized(order_initialized_buy_limit: OrderInitialized) {
        let display = format!("{order_initialized_buy_limit}");
        assert_eq!(
            display,
            "OrderInitialized(instrument_id=BTCUSDT.COINBASE, client_order_id=O-20200814-102234-001-001-1, \
            side=BUY, type=LIMIT, quantity=0.561, time_in_force=DAY, post_only=true, reduce_only=true, \
            quote_quantity=false, price=22000, emulation_trigger=BID_ASK, trigger_instrument_id=BTCUSDT.COINBASE, \
            contingency_type=OTO, order_list_id=1, linked_order_ids=[O-2020872378424], parent_order_id=None, \
            exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=None)");
    }
}

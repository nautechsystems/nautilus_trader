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

#![allow(dead_code)]

use crate::enums::OrderStatus;
use crate::events::order::{OrderEvent, OrderInitialized};
use rust_fsm::*;

#[derive(Debug)]
struct OrderFsm;

impl StateMachineImpl for OrderFsm {
    type State = OrderStatus;
    type Input = OrderEvent;
    type Output = OrderStatus;
    const INITIAL_STATE: Self::State = OrderStatus::Initialized;

    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        match (state, input) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => Some(OrderStatus::Denied),
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => {
                Some(OrderStatus::Submitted)
            }
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => {
                Some(OrderStatus::Triggered)
            }
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            _ => None,
        }
    }

    fn output(state: &Self::State, input: &Self::Input) -> Option<Self::Output> {
        match (state, input) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => Some(OrderStatus::Denied),
            _ => None,
        }
    }
}

struct Order {
    events: Vec<OrderEvent>,
    fsm: StateMachine<OrderFsm>,
    pub status: OrderStatus,
}

impl Order {
    pub fn new(init: OrderInitialized) -> Self {
        Self {
            events: vec![OrderEvent::OrderInitialized(init)],
            fsm: StateMachine::new(),
            status: OrderStatus::Initialized,
        }
    }

    pub fn last_event(&self) -> &OrderEvent {
        match self.events.last() {
            Some(last) => last,
            None => panic!("events was empty"),
        }
    }

    pub fn apply(&mut self, event: OrderEvent) {
        match event {
            OrderEvent::OrderDenied(_) => {} // Do nothing
            _ => panic!("unrecognized event"),
        }

        let status = self.fsm.consume(&event).unwrap();
        match status {
            Some(status) => self.status = status,
            None => panic!("invalid state transition"),
        }
        self.events.push(event);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{OrderSide, OrderType, TimeInForce};

    use crate::events::order::OrderDenied;
    use crate::identifiers::client_order_id::ClientOrderId;
    use crate::identifiers::instrument_id::InstrumentId;
    use crate::identifiers::strategy_id::StrategyId;
    use crate::identifiers::trader_id::TraderId;
    use crate::types::quantity::Quantity;

    #[test]
    fn test_order_state_transition() {
        let init = OrderInitialized {
            trader_id: TraderId::new("TRADER-001"),
            strategy_id: StrategyId::new("S-001"),
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            client_order_id: ClientOrderId::new("O-123456789"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(1.0, 8),
            time_in_force: TimeInForce::Day,
            post_only: false,
            reduce_only: false,
            emulation_trigger: None,
            contingency_type: None,
            order_list_id: None,
            linked_order_ids: None,
            parent_order_id: None,
            tags: None,
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
            reconciliation: false,
        };

        let mut order = Order::new(init);
        let denied = OrderDenied {
            trader_id: TraderId::new("TRADER-001"),
            strategy_id: StrategyId::new("S-001"),
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            client_order_id: ClientOrderId::new("O-123456789"),
            reason: "".to_string(),
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
        };

        let event = OrderEvent::OrderDenied(denied);

        order.apply(event);
        assert_eq!(order.status, OrderStatus::Denied);
    }
}

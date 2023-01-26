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
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrderError {
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("Unrecognized event")]
    UnrecognizedEvent,
}

#[derive(Debug)]
struct OrderFsm;

impl StateMachineImpl for OrderFsm {
    type State = OrderStatus;
    type Input = OrderEvent;
    type Output = OrderStatus;
    const INITIAL_STATE: Self::State = OrderStatus::Initialized;

    #[rustfmt::skip]
    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        match (state, input) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => Some(OrderStatus::Denied),
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => Some(OrderStatus::Submitted),
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Submitted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Submitted, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Submitted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Submitted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Submitted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Accepted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Accepted, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::Accepted, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::Accepted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Accepted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Accepted, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Accepted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Accepted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Canceled, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Canceled, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::PendingUpdate, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::PendingUpdate, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PendingCancel, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PendingCancel, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingCancel, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::PendingCancel, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PendingCancel, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Triggered, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Triggered, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::Triggered, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::Triggered, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Triggered, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Triggered, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Triggered, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            _ => None,
        }
    }

    #[rustfmt::skip]
    fn output(state: &Self::State, input: &Self::Input) -> Option<Self::Output> {
        match (state, input) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => Some(OrderStatus::Denied),
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => Some(OrderStatus::Submitted),
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Submitted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Submitted, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Submitted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Submitted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Submitted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Accepted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Accepted, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::Accepted, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::Accepted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Accepted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Accepted, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Accepted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Accepted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Canceled, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Canceled, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::PendingUpdate, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::PendingUpdate, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PendingCancel, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PendingCancel, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingCancel, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::PendingCancel, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PendingCancel, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Triggered, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Triggered, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::Triggered, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::Triggered, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Triggered, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Triggered, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Triggered, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PartiallyFilled, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
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
            _ => panic!("events was empty"),
        }
    }

    pub fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError> {
        match self.fsm.consume(&event) {
            Ok(status) => {
                if let Some(status) = status {
                    self.status = status;
                } else {
                    return Err(OrderError::InvalidStateTransition);
                }
            }
            Err(_) => {
                return Err(OrderError::InvalidStateTransition);
            }
        }

        match event {
            OrderEvent::OrderDenied(_) => {} // Do nothing
            _ => return Err(OrderError::UnrecognizedEvent),
        }

        self.events.push(event);
        Ok(())
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

        let _ = order.apply(event);
        assert_eq!(order.status, OrderStatus::Denied);
    }
}

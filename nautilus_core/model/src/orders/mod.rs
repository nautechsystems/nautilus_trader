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

use crate::enums::{
    ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
    TriggerType,
};
use crate::events::order::{OrderEvent, OrderInitialized};
use crate::identifiers::account_id::AccountId;
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::order_list_id::OrderListId;
use crate::identifiers::position_id::PositionId;
use crate::identifiers::strategy_id::StrategyId;
use crate::identifiers::trade_id::TradeId;
use crate::identifiers::trader_id::TraderId;
use crate::identifiers::venue_order_id::VenueOrderId;
use crate::types::quantity::Quantity;
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;
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
    type Input = OrderEvent;
    type State = OrderStatus;
    type Output = OrderStatus;
    const INITIAL_STATE: Self::State = OrderStatus::Initialized;

    #[rustfmt::skip]
    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        match (state, input) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => Some(OrderStatus::Denied),
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => Some(OrderStatus::Submitted),
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),  // Covers external orders
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),  // Covers external orders
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),  // Covers emulated and external orders
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),  // Covers emulated and external orders
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered), // Covers emulated and external orders
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),
            (OrderStatus::Submitted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),  // Covers FOK and IOC cases
            (OrderStatus::Submitted, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::Submitted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),  // Covers emulated StopLimit order
            (OrderStatus::Submitted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Submitted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Accepted, OrderEvent::OrderRejected(_)) => Some(OrderStatus::Rejected),  // Covers StopLimit order
            (OrderStatus::Accepted, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),
            (OrderStatus::Accepted, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::Accepted, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::Accepted, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::Accepted, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::Accepted, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::Accepted, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::Canceled, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),  // Real world possibility
            (OrderStatus::Canceled, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),  // Real world possibility
            (OrderStatus::PendingUpdate, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),
            (OrderStatus::PendingUpdate, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderExpired(_)) => Some(OrderStatus::Expired),
            (OrderStatus::PendingUpdate, OrderEvent::OrderTriggered(_)) => Some(OrderStatus::Triggered),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingUpdate(_)) => Some(OrderStatus::PendingUpdate),  // Allow multiple requests
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),
            (OrderStatus::PendingUpdate, OrderEvent::OrderPartiallyFilled(_)) => Some(OrderStatus::PartiallyFilled),
            (OrderStatus::PendingUpdate, OrderEvent::OrderFilled(_)) => Some(OrderStatus::Filled),
            (OrderStatus::PendingCancel, OrderEvent::OrderPendingCancel(_)) => Some(OrderStatus::PendingCancel),  // Allow multiple requests
            (OrderStatus::PendingCancel, OrderEvent::OrderCanceled(_)) => Some(OrderStatus::Canceled),
            (OrderStatus::PendingCancel, OrderEvent::OrderAccepted(_)) => Some(OrderStatus::Accepted),  // Allow failed cancel requests
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
    venue_order_ids: Vec<VenueOrderId>,
    trade_ids: Vec<TradeId>,
    fsm: StateMachine<OrderFsm>,
    pub status: OrderStatus,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub position_id: Option<PositionId>,
    pub account_id: Option<AccountId>,
    pub last_trade_id: Option<TradeId>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub time_in_force: TimeInForce,
    pub liquidity_side: Option<LiquiditySide>,
    pub is_post_only: bool,
    pub is_reduce_only: bool,
    pub emulation_trigger: Option<TriggerType>,
    pub contingency_type: Option<ContingencyType>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub tags: Option<String>,
    pub filled_qty: Quantity,
    pub leaves_qty: Quantity,
    pub avg_px: Option<f64>,
    pub slippage: Option<f64>,
    pub init_id: UUID4,
    pub ts_init: UnixNanos,
    pub ts_last: UnixNanos,
}

impl PartialEq<Self> for Order {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id
    }
}

impl Eq for Order {}

impl Order {
    /// Initialize a new `Order` by consuming the given `OrderInitialized` event.
    pub fn new(init: OrderInitialized) -> Self {
        Self {
            events: Vec::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            fsm: StateMachine::new(),
            status: OrderStatus::Initialized,
            trader_id: init.trader_id,
            strategy_id: init.strategy_id,
            instrument_id: init.instrument_id,
            client_order_id: init.client_order_id,
            venue_order_id: None,
            position_id: None,
            account_id: None,
            last_trade_id: None,
            side: init.order_side,
            order_type: init.order_type,
            quantity: init.quantity.clone(),
            time_in_force: init.time_in_force,
            liquidity_side: None,
            is_post_only: init.post_only,
            is_reduce_only: init.reduce_only,
            emulation_trigger: init.emulation_trigger,
            contingency_type: init.contingency_type,
            order_list_id: init.order_list_id,
            linked_order_ids: init.linked_order_ids,
            parent_order_id: init.parent_order_id,
            tags: init.tags,
            filled_qty: Quantity::new(0.0, 0),
            leaves_qty: init.quantity,
            avg_px: None,
            slippage: None,
            init_id: init.event_id,
            ts_init: init.ts_event,
            ts_last: init.ts_event,
        }
    }

    pub fn init_event(&self) -> OrderInitialized {
        OrderInitialized {
            trader_id: self.trader_id.clone(),
            strategy_id: self.strategy_id.clone(),
            instrument_id: self.instrument_id.clone(),
            client_order_id: self.client_order_id.clone(),
            order_side: self.side,
            order_type: self.order_type,
            quantity: self.quantity.clone(),
            time_in_force: self.time_in_force,
            post_only: self.is_post_only,
            reduce_only: self.is_reduce_only,
            emulation_trigger: self.emulation_trigger,
            contingency_type: self.contingency_type,
            order_list_id: self.order_list_id.clone(),
            linked_order_ids: self.linked_order_ids.clone(),
            parent_order_id: self.parent_order_id.clone(),
            tags: self.tags.clone(),
            event_id: self.init_id.clone(),
            ts_event: self.ts_init,
            ts_init: self.ts_init,
            reconciliation: false,
        }
    }

    pub fn last_event(&self) -> Option<&OrderEvent> {
        self.events.last()
    }

    pub fn events(&self) -> Vec<OrderEvent> {
        self.events.clone()
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn venue_order_ids(&self) -> Vec<VenueOrderId> {
        self.venue_order_ids.clone()
    }

    pub fn trade_ids(&self) -> Vec<TradeId> {
        self.trade_ids.clone()
    }

    pub fn is_buy(&self) -> bool {
        self.side == OrderSide::Buy
    }

    pub fn is_sell(&self) -> bool {
        self.side == OrderSide::Sell
    }

    pub fn is_passive(&self) -> bool {
        self.order_type != OrderType::Market
    }

    pub fn is_aggressive(&self) -> bool {
        self.order_type == OrderType::Market
    }

    pub fn is_emulated(&self) -> bool {
        self.emulation_trigger.is_some()
    }

    pub fn is_contingency(&self) -> bool {
        self.contingency_type.is_some()
    }

    pub fn is_parent_order(&self) -> bool {
        match self.contingency_type {
            Some(c) => c == ContingencyType::Oto,
            None => false,
        }
    }

    pub fn is_child_order(&self) -> bool {
        self.parent_order_id.is_some()
    }

    pub fn is_open(&self) -> bool {
        if self.emulation_trigger.is_some() {
            return false;
        }
        self.status == OrderStatus::Accepted
            || self.status == OrderStatus::Triggered
            || self.status == OrderStatus::PendingCancel
            || self.status == OrderStatus::PendingUpdate
            || self.status == OrderStatus::PartiallyFilled
    }

    pub fn is_closed(&self) -> bool {
        self.status == OrderStatus::Denied
            || self.status == OrderStatus::Rejected
            || self.status == OrderStatus::Canceled
            || self.status == OrderStatus::Expired
            || self.status == OrderStatus::Filled
    }

    pub fn is_inflight(&self) -> bool {
        if self.emulation_trigger.is_some() {
            return false;
        }
        self.status == OrderStatus::Submitted
            || self.status == OrderStatus::PendingCancel
            || self.status == OrderStatus::PendingUpdate
    }

    pub fn is_pending_update(&self) -> bool {
        self.status == OrderStatus::PendingUpdate
    }

    pub fn is_pending_cancel(&self) -> bool {
        self.status == OrderStatus::PendingCancel
    }

    pub fn opposite_side(side: OrderSide) -> OrderSide {
        match side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        }
    }

    pub fn closing_side(side: PositionSide) -> OrderSide {
        match side {
            PositionSide::Long => OrderSide::Sell,
            PositionSide::Short => OrderSide::Buy,
            PositionSide::Flat => OrderSide::NoOrderSide,
            PositionSide::NoPositionSide => OrderSide::NoOrderSide,
        }
    }

    pub fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
        if side == PositionSide::Flat {
            return false;
        }

        match (self.side, side) {
            (OrderSide::Buy, PositionSide::Long) => false,
            (OrderSide::Buy, PositionSide::Short) => self.leaves_qty > position_qty,
            (OrderSide::Sell, PositionSide::Short) => false,
            (OrderSide::Sell, PositionSide::Long) => self.leaves_qty > position_qty,
            _ => true,
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
    use crate::enums::*;
    use crate::events::order::{stub_order_denied, stub_order_initialized};

    #[test]
    fn test_order_initialized() {
        let init = stub_order_initialized();
        let order = Order::new(init.clone());

        assert_eq!(order.status, OrderStatus::Initialized);
        assert_eq!(order.init_event(), init);
        assert_eq!(order.last_event(), None);
        assert_eq!(order.event_count(), 0);
        assert!(order.venue_order_ids.is_empty());
        assert!(order.trade_ids.is_empty());
    }

    #[test]
    fn test_order_state_transition_denied() {
        let init = stub_order_initialized();
        let mut order = Order::new(init);
        let denied = stub_order_denied();
        let event = OrderEvent::OrderDenied(denied);

        let _ = order.apply(event.clone());

        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());
        assert!(!order.is_open());
        assert_eq!(order.event_count(), 1);
        assert_eq!(order.last_event(), Some(&event));
    }
}

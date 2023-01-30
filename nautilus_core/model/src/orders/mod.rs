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

use std::rc::Rc;

use crate::enums::{
    ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
    TriggerType,
};
use crate::events::order::{
    OrderAccepted, OrderCancelRejected, OrderDenied, OrderEvent, OrderInitialized, OrderMetadata,
    OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderSubmitted,
    OrderUpdated,
};
use crate::identifiers::account_id::AccountId;
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::order_list_id::OrderListId;
use crate::identifiers::position_id::PositionId;
use crate::identifiers::trade_id::TradeId;
use crate::identifiers::venue_order_id::VenueOrderId;
use crate::types::fixed::fixed_i64_to_f64;
use crate::types::price::Price;
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

impl From<TransitionImpossibleError> for OrderError {
    fn from(_: TransitionImpossibleError) -> Self {
        OrderError::InvalidStateTransition
    }
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
    venue_order_ids: Vec<VenueOrderId>, // TODO(cs): Should be `Vec<&VenueOrderId>` or similar
    trade_ids: Vec<TradeId>,            // TODO(cs): Should be `Vec<&TradeId>` or similar
    fsm: StateMachine<OrderFsm>,
    previous_status: Option<OrderStatus>,
    triggered_price: Option<Price>,
    pub status: OrderStatus,
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub position_id: Option<PositionId>,
    pub account_id: Option<AccountId>,
    pub last_trade_id: Option<TradeId>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub time_in_force: TimeInForce,
    pub expire_time: Option<UnixNanos>,
    pub liquidity_side: Option<LiquiditySide>,
    pub is_post_only: bool,
    pub is_reduce_only: bool,
    pub display_qty: Option<Quantity>,
    pub limit_offset: Option<Price>,
    pub trailing_offset: Option<Price>,
    pub trailing_offset_type: Option<TriggerType>,
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
    pub ts_triggered: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub ts_last: UnixNanos,
}

impl PartialEq<Self> for Order {
    fn eq(&self, other: &Self) -> bool {
        // TODO: can implement deref and deref mut for order metadata here too
        self.metadata.client_order_id == other.metadata.client_order_id
    }
}

impl Eq for Order {}

impl From<OrderInitialized> for Order {
    fn from(value: OrderInitialized) -> Self {
        Self {
            events: Vec::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            fsm: StateMachine::new(),
            previous_status: None,
            triggered_price: None,
            status: OrderStatus::Initialized,
            metadata: value.metadata,
            venue_order_id: None,
            position_id: None,
            account_id: None,
            last_trade_id: None,
            side: value.order_side,
            order_type: value.order_type,
            quantity: value.quantity.clone(),
            price: value.price,
            trigger_price: value.trigger_price,
            trigger_type: value.trigger_type,
            time_in_force: value.time_in_force,
            expire_time: None,
            liquidity_side: None,
            is_post_only: value.post_only,
            is_reduce_only: value.reduce_only,
            display_qty: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
            emulation_trigger: value.emulation_trigger,
            contingency_type: value.contingency_type,
            order_list_id: value.order_list_id,
            linked_order_ids: value.linked_order_ids,
            parent_order_id: value.parent_order_id,
            tags: value.tags,
            filled_qty: Quantity::new(0.0, 0),
            leaves_qty: value.quantity,
            avg_px: None,
            slippage: None,
            init_id: value.event_id,
            ts_triggered: None,
            ts_init: value.ts_event,
            ts_last: value.ts_event,
        }
    }
}

impl From<&Order> for OrderInitialized {
    fn from(value: &Order) -> Self {
        Self {
            metadata: value.metadata.clone(),
            order_side: value.side,
            order_type: value.order_type,
            quantity: value.quantity.clone(),
            price: value.price.clone(),
            trigger_price: value.triggered_price.clone(),
            trigger_type: value.trigger_type,
            time_in_force: value.time_in_force,
            expire_time: value.expire_time,
            post_only: value.is_post_only,
            reduce_only: value.is_reduce_only,
            display_qty: value.display_qty.clone(),
            limit_offset: value.limit_offset.clone(),
            trailing_offset: value.trailing_offset.clone(),
            trailing_offset_type: value.trailing_offset_type,
            emulation_trigger: value.emulation_trigger,
            contingency_type: value.contingency_type,
            order_list_id: value.order_list_id.clone(),
            linked_order_ids: value.linked_order_ids.clone(),
            parent_order_id: value.parent_order_id.clone(),
            tags: value.tags.clone(),
            event_id: value.init_id.clone(),
            ts_event: value.ts_init,
            ts_init: value.ts_init,
            reconciliation: false,
        }
    }
}

impl Order {
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
            (OrderSide::Buy, PositionSide::Short) => self.leaves_qty <= position_qty,
            (OrderSide::Sell, PositionSide::Short) => false,
            (OrderSide::Sell, PositionSide::Long) => self.leaves_qty <= position_qty,
            _ => true,
        }
    }

    pub fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError> {
        let status = self
            .fsm
            .consume(&event)?
            .map(|status| {
                self.previous_status.replace(self.status);
                status
            })
            .ok_or(OrderError::InvalidStateTransition)?;
        self.status = status;

        match &event {
            OrderEvent::OrderDenied(event) => self.denied(event),
            OrderEvent::OrderSubmitted(event) => self.submitted(event),
            OrderEvent::OrderRejected(event) => self.rejected(event),
            OrderEvent::OrderAccepted(event) => self.accepted(event),
            OrderEvent::OrderPendingUpdate(event) => self.pending_update(event),
            OrderEvent::OrderUpdated(event) => self.updated(event),
            _ => return Err(OrderError::UnrecognizedEvent),
        }

        self.events.push(event);
        Ok(())
    }

    fn denied(&self, _event: &OrderDenied) {
        // Do nothing else
    }

    fn submitted(&mut self, event: &OrderSubmitted) {
        self.account_id = Some(event.account_id.clone())
    }

    fn accepted(&mut self, event: &OrderAccepted) {
        self.venue_order_id = Some(event.venue_order_id.clone());
    }

    fn rejected(&self, _event: &OrderRejected) {
        // Do nothing else
    }

    fn pending_update(&self, _event: &OrderPendingUpdate) {
        // Do nothing else
    }

    fn pending_cancel(&self, _event: &OrderPendingCancel) {
        // Do nothing else
    }

    fn modify_rejected(&mut self, _event: &OrderModifyRejected) {
        self.status = self.previous_status.unwrap();
    }

    fn cancel_rejected(&mut self, _event: &OrderCancelRejected) {
        self.status = self.previous_status.unwrap();
    }

    fn updated(&mut self, event: &OrderUpdated) {
        match &event.venue_order_id {
            Some(venue_order_id) => {
                if self.venue_order_id.is_some()
                    && venue_order_id != self.venue_order_id.as_ref().unwrap()
                {
                    self.venue_order_id = Some(venue_order_id.clone());
                    self.venue_order_ids.push(venue_order_id.clone()); // TODO(cs): Temporary clone
                }
            }
            None => {}
        }
        if let Some(price) = &event.price {
            if self.price.is_some() {
                self.price.replace(price.clone());
            } else {
                panic!("invalid update of `price` when None")
            }
        }

        if let Some(trigger_price) = &event.trigger_price {
            if self.trigger_price.is_some() {
                self.trigger_price.replace(trigger_price.clone());
            } else {
                panic!("invalid update of `trigger_price` when None")
            }
        }

        self.quantity.raw = event.quantity.raw;
        self.leaves_qty = Quantity::from_raw(
            self.quantity.raw - self.filled_qty.raw,
            self.quantity.precision,
        );
    }

    fn set_slippage(&mut self) {
        self.slippage = self.avg_px.and_then(|avg_px| {
            self.price
                .as_ref()
                .map(|price| fixed_i64_to_f64(price.raw))
                .map(|price| match self.side {
                    OrderSide::Buy if avg_px > price => Some(avg_px - price),
                    OrderSide::Sell if avg_px < price => Some(price - avg_px),
                    _ => None,
                })
                .unwrap_or(None)
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::enums::*;
    use crate::events::order::*;

    #[test]
    fn test_order_initialized() {
        let order: Order = OrderInitializedBuilder::default().build().unwrap().into();

        assert_eq!(order.status, OrderStatus::Initialized);
        assert_eq!(order.last_event(), None);
        assert_eq!(order.event_count(), 0);
        assert!(order.venue_order_ids.is_empty());
        assert!(order.trade_ids.is_empty());
        assert!(order.is_buy());
        assert!(!order.is_sell());
        assert!(!order.is_passive());
        assert!(order.is_aggressive());
        assert!(!order.is_emulated());
        assert!(!order.is_contingency());
        assert!(!order.is_parent_order());
        assert!(!order.is_child_order());
        assert!(!order.is_open());
        assert!(!order.is_closed());
        assert!(!order.is_inflight());
        assert!(!order.is_pending_update());
        assert!(!order.is_pending_cancel());
    }

    #[rstest(
        order_side,
        expected_side,
        case(OrderSide::Buy, OrderSide::Sell),
        case(OrderSide::Sell, OrderSide::Buy),
        case(OrderSide::NoOrderSide, OrderSide::NoOrderSide)
    )]
    fn test_order_opposite_side(order_side: OrderSide, expected_side: OrderSide) {
        let result = Order::opposite_side(order_side);
        assert_eq!(result, expected_side)
    }

    #[rstest(
        position_side,
        expected_side,
        case(PositionSide::Long, OrderSide::Sell),
        case(PositionSide::Short, OrderSide::Buy),
        case(PositionSide::NoPositionSide, OrderSide::NoOrderSide)
    )]
    fn test_closing_side(position_side: PositionSide, expected_side: OrderSide) {
        let result = Order::closing_side(position_side);
        assert_eq!(result, expected_side)
    }

    #[rustfmt::skip]
    #[rstest(
        order_side, order_qty, position_side, position_qty, expected,
        case(OrderSide::Buy, Quantity::from(100), PositionSide::Long, Quantity::from(50), false),
        case(OrderSide::Buy, Quantity::from(50), PositionSide::Short, Quantity::from(50), true),
        case(OrderSide::Buy, Quantity::from(50), PositionSide::Short, Quantity::from(100), true),
        case(OrderSide::Buy, Quantity::from(50), PositionSide::Flat, Quantity::from(0), false),
        case(OrderSide::Sell, Quantity::from(50), PositionSide::Flat, Quantity::from(0), false),
        case(OrderSide::Sell, Quantity::from(50), PositionSide::Long, Quantity::from(50), true),
        case(OrderSide::Sell, Quantity::from(50), PositionSide::Long, Quantity::from(100), true),
        case(OrderSide::Sell, Quantity::from(100), PositionSide::Short, Quantity::from(50), false),
    )]
    fn test_would_reduce_only(
        order_side: OrderSide,
        order_qty: Quantity,
        position_side: PositionSide,
        position_qty: Quantity,
        expected: bool,
    ) {
        let order: Order = OrderInitializedBuilder::default()
            .order_side(order_side)
            .quantity(order_qty)
            .build()
            .unwrap()
            .into();

        assert_eq!(
            order.would_reduce_only(position_side, position_qty),
            expected
        );
    }

    #[test]
    fn test_order_state_transition_denied() {
        let init = OrderInitializedBuilder::default().build().unwrap();
        let denied = OrderDeniedBuilder::default().build().unwrap();
        let mut order: Order = init.into();
        let event = OrderEvent::OrderDenied(denied);

        let _ = order.apply(event.clone());

        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());
        assert!(!order.is_open());
        assert_eq!(order.event_count(), 1);
        assert_eq!(order.last_event(), Some(&event));
    }
}

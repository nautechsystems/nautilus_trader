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

use nautilus_core::{time::UnixNanos, uuid::UUID4};
use thiserror;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::order::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEvent, OrderExpired,
        OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate,
        OrderRejected, OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, position_id::PositionId,
        strategy_id::StrategyId, trade_id::TradeId, trader_id::TraderId,
        venue_order_id::VenueOrderId,
    },
    types::{price::Price, quantity::Quantity},
};

#[derive(thiserror::Error, Debug)]
pub enum OrderError {
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("Unrecognized event")]
    UnrecognizedEvent,
}

impl OrderStatus {
    #[rustfmt::skip]
    pub fn transition(&mut self, event: &OrderEvent) -> Result<OrderStatus, OrderError> {
        let new_state = match (self, event) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => OrderStatus::Denied,
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => OrderStatus::Submitted,
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,  // Covers external orders
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,  // Covers external orders
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // Covers emulated and external orders
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,  // Covers emulated and external orders
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => OrderStatus::Triggered, // Covers emulated and external orders
            (OrderStatus::Submitted, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,
            (OrderStatus::Submitted, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,
            (OrderStatus::Submitted, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // Covers FOK and IOC cases
            (OrderStatus::Submitted, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,
            (OrderStatus::Submitted, OrderEvent::OrderTriggered(_)) => OrderStatus::Triggered,  // Covers emulated StopLimit order
            (OrderStatus::Submitted, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::Submitted, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::Accepted, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,  // Covers StopLimit order
            (OrderStatus::Accepted, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,
            (OrderStatus::Accepted, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::Accepted, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,
            (OrderStatus::Accepted, OrderEvent::OrderTriggered(_)) => OrderStatus::Triggered,
            (OrderStatus::Accepted, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,
            (OrderStatus::Accepted, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::Accepted, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::Canceled, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,  // Real world possibility
            (OrderStatus::Canceled, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,  // Real world possibility
            (OrderStatus::PendingUpdate, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,
            (OrderStatus::PendingUpdate, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,
            (OrderStatus::PendingUpdate, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,
            (OrderStatus::PendingUpdate, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,
            (OrderStatus::PendingUpdate, OrderEvent::OrderTriggered(_)) => OrderStatus::Triggered,
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,  // Allow multiple requests
            (OrderStatus::PendingUpdate, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::PendingUpdate, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::PendingUpdate, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::PendingCancel, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,
            (OrderStatus::PendingCancel, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,  // Allow multiple requests
            (OrderStatus::PendingCancel, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,
            (OrderStatus::PendingCancel, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,
            (OrderStatus::PendingCancel, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,  // Allow failed cancel requests
            (OrderStatus::PendingCancel, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::PendingCancel, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::Triggered, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,
            (OrderStatus::Triggered, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,
            (OrderStatus::Triggered, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::Triggered, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,
            (OrderStatus::Triggered, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,
            (OrderStatus::Triggered, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::Triggered, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::PartiallyFilled, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            _ => return Err(OrderError::InvalidStateTransition),
        };
        Ok(new_state)
    }
}

pub trait Order {
    fn status(&self) -> OrderStatus;
    fn trader_id(&self) -> TraderId;
    fn strategy_id(&self) -> StrategyId;
    fn instrument_id(&self) -> InstrumentId;
    fn client_order_id(&self) -> ClientOrderId;
    fn venue_order_id(&self) -> Option<VenueOrderId>;
    fn position_id(&self) -> Option<PositionId>;
    fn account_id(&self) -> Option<AccountId>;
    fn last_trade_id(&self) -> Option<TradeId>;
    fn side(&self) -> OrderSide;
    fn order_type(&self) -> OrderType;
    fn quantity(&self) -> Quantity;
    fn time_in_force(&self) -> TimeInForce;
    fn expire_time(&self) -> Option<UnixNanos>;
    fn price(&self) -> Option<Price>;
    fn trigger_price(&self) -> Option<Price>;
    fn trigger_type(&self) -> Option<TriggerType>;
    fn liquidity_side(&self) -> Option<LiquiditySide>;
    fn is_post_only(&self) -> bool;
    fn is_reduce_only(&self) -> bool;
    fn is_quote_quantity(&self) -> bool;
    fn display_qty(&self) -> Option<Quantity>;
    fn limit_offset(&self) -> Option<Price>;
    fn trailing_offset(&self) -> Option<Price>;
    fn trailing_offset_type(&self) -> Option<TrailingOffsetType>;
    fn emulation_trigger(&self) -> Option<TriggerType>;
    fn trigger_instrument_id(&self) -> Option<InstrumentId>;
    fn contingency_type(&self) -> Option<ContingencyType>;
    fn order_list_id(&self) -> Option<OrderListId>;
    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>>;
    fn parent_order_id(&self) -> Option<ClientOrderId>;
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
    fn exec_algorithm_params(&self) -> Option<HashMap<String, String>>;
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn tags(&self) -> Option<String>;
    fn filled_qty(&self) -> Quantity;
    fn leaves_qty(&self) -> Quantity;
    fn avg_px(&self) -> Option<f64>;
    fn slippage(&self) -> Option<f64>;
    fn init_id(&self) -> UUID4;
    fn ts_init(&self) -> UnixNanos;
    fn ts_last(&self) -> UnixNanos;

    fn events(&self) -> Vec<&OrderEvent>;

    fn last_event(&self) -> &OrderEvent {
        // Safety: `Order` specification guarantees at least one event (`OrderInitialized`)
        self.events().last().unwrap()
    }

    fn event_count(&self) -> usize {
        self.events().len()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId>;

    fn trade_ids(&self) -> Vec<&TradeId>;

    fn is_buy(&self) -> bool {
        self.side() == OrderSide::Buy
    }

    fn is_sell(&self) -> bool {
        self.side() == OrderSide::Sell
    }

    fn is_passive(&self) -> bool {
        self.order_type() != OrderType::Market
    }

    fn is_aggressive(&self) -> bool {
        self.order_type() == OrderType::Market
    }

    fn is_emulated(&self) -> bool {
        self.emulation_trigger().is_some()
    }

    fn is_contingency(&self) -> bool {
        self.contingency_type().is_some()
    }

    fn is_parent_order(&self) -> bool {
        match self.contingency_type() {
            Some(c) => c == ContingencyType::Oto,
            None => false,
        }
    }

    fn is_child_order(&self) -> bool {
        self.parent_order_id().is_some()
    }

    fn is_open(&self) -> bool {
        self.emulation_trigger().is_none()
            && matches!(
                self.status(),
                OrderStatus::Accepted
                    | OrderStatus::Triggered
                    | OrderStatus::PendingCancel
                    | OrderStatus::PendingUpdate
                    | OrderStatus::PartiallyFilled
            )
    }

    fn is_closed(&self) -> bool {
        matches!(
            self.status(),
            OrderStatus::Denied
                | OrderStatus::Rejected
                | OrderStatus::Canceled
                | OrderStatus::Expired
                | OrderStatus::Filled
        )
    }

    fn is_inflight(&self) -> bool {
        self.emulation_trigger().is_none()
            && matches!(
                self.status(),
                OrderStatus::Submitted | OrderStatus::PendingCancel | OrderStatus::PendingUpdate
            )
    }

    fn is_pending_update(&self) -> bool {
        self.status() == OrderStatus::PendingUpdate
    }

    fn is_pending_cancel(&self) -> bool {
        self.status() == OrderStatus::PendingCancel
    }
}

impl<T> From<&T> for OrderInitialized
where
    T: Order,
{
    fn from(order: &T) -> Self {
        Self {
            trader_id: order.trader_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            order_side: order.side(),
            order_type: order.order_type(),
            quantity: order.quantity(),
            price: order.price(),
            trigger_price: order.trigger_price(),
            trigger_type: order.trigger_type(),
            time_in_force: order.time_in_force(),
            expire_time: order.expire_time(),
            post_only: order.is_post_only(),
            reduce_only: order.is_reduce_only(),
            quote_quantity: order.is_quote_quantity(),
            display_qty: order.display_qty(),
            limit_offset: order.limit_offset(),
            trailing_offset: order.trailing_offset(),
            trailing_offset_type: order.trailing_offset_type(),
            emulation_trigger: order.emulation_trigger(),
            trigger_instrument_id: order.trigger_instrument_id(),
            contingency_type: order.contingency_type(),
            order_list_id: order.order_list_id(),
            linked_order_ids: order.linked_order_ids(),
            parent_order_id: order.parent_order_id(),
            exec_algorithm_id: order.exec_algorithm_id(),
            exec_algorithm_params: order.exec_algorithm_params(),
            exec_spawn_id: order.exec_spawn_id(),
            tags: order.tags(),
            event_id: order.init_id(),
            ts_event: order.ts_init(),
            ts_init: order.ts_init(),
            reconciliation: false,
        }
    }
}

pub struct OrderCore {
    pub events: Vec<OrderEvent>,
    pub venue_order_ids: Vec<VenueOrderId>,
    pub trade_ids: Vec<TradeId>,
    pub previous_status: Option<OrderStatus>,
    pub has_price: bool,
    pub has_trigger_price: bool,
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
    pub is_reduce_only: bool,
    pub is_quote_quantity: bool,
    pub contingency_type: Option<ContingencyType>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub exec_algorithm_params: Option<HashMap<String, String>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<String>,
    pub filled_qty: Quantity,
    pub leaves_qty: Quantity,
    pub avg_px: Option<f64>,
    pub slippage: Option<f64>,
    pub init_id: UUID4,
    pub ts_init: UnixNanos,
    pub ts_last: UnixNanos,
}

impl OrderCore {
    #[must_use]
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
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            events: Vec::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            previous_status: None,
            has_price: true,          // TODO
            has_trigger_price: false, // TODO
            status: OrderStatus::Initialized,
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id: None,
            position_id: None,
            account_id: None,
            last_trade_id: None,
            side: order_side,
            order_type,
            quantity,
            time_in_force,
            liquidity_side: None,
            is_reduce_only: reduce_only,
            is_quote_quantity: quote_quantity,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            filled_qty: Quantity::zero(quantity.precision),
            leaves_qty: quantity,
            avg_px: None,
            slippage: None,
            init_id,
            ts_init,
            ts_last: ts_init,
        }
    }

    fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError> {
        let new_status = self.status.transition(&event)?;
        self.previous_status = Some(self.status);
        self.status = new_status;

        match &event {
            OrderEvent::OrderDenied(event) => self.denied(event),
            OrderEvent::OrderSubmitted(event) => self.submitted(event),
            OrderEvent::OrderRejected(event) => self.rejected(event),
            OrderEvent::OrderAccepted(event) => self.accepted(event),
            OrderEvent::OrderPendingUpdate(event) => self.pending_update(event),
            OrderEvent::OrderPendingCancel(event) => self.pending_cancel(event),
            OrderEvent::OrderModifyRejected(event) => self.modify_rejected(event),
            OrderEvent::OrderCancelRejected(event) => self.cancel_rejected(event),
            OrderEvent::OrderUpdated(event) => self.updated(event),
            OrderEvent::OrderTriggered(event) => self.triggered(event),
            OrderEvent::OrderCanceled(event) => self.canceled(event),
            OrderEvent::OrderExpired(event) => self.expired(event),
            _ => return Err(OrderError::UnrecognizedEvent),
        }

        self.events.push(event);
        Ok(())
    }

    fn denied(&self, _event: &OrderDenied) {
        // Do nothing else
    }

    fn submitted(&mut self, event: &OrderSubmitted) {
        self.account_id = Some(event.account_id)
    }

    fn accepted(&mut self, event: &OrderAccepted) {
        self.venue_order_id = Some(event.venue_order_id);
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

    fn triggered(&mut self, _event: &OrderTriggered) {}

    fn canceled(&mut self, _event: &OrderCanceled) {}

    fn expired(&mut self, _event: &OrderExpired) {}

    fn updated(&mut self, event: &OrderUpdated) {
        match &event.venue_order_id {
            Some(venue_order_id) => {
                if self.venue_order_id.is_some()
                    && venue_order_id != self.venue_order_id.as_ref().unwrap()
                {
                    self.venue_order_id = Some(*venue_order_id);
                    self.venue_order_ids.push(*venue_order_id);
                }
            }
            None => {}
        }

        // TODO
        // if let Some(price) = &event.price {
        //     if self.price.is_some() {
        //         self.price.replace(*price);
        //     } else {
        //         panic!("invalid update of `price` when None")
        //     }
        // }
        //
        // if let Some(trigger_price) = &event.trigger_price {
        //     if self.trigger_price.is_some() {
        //         self.trigger_price.replace(*trigger_price);
        //     } else {
        //         panic!("invalid update of `trigger_price` when None")
        //     }
        // }

        self.quantity = event.quantity;
        self.leaves_qty = self.quantity - self.filled_qty;
    }

    fn filled(&mut self, event: &OrderFilled) {
        self.venue_order_id = Some(event.venue_order_id);
        self.position_id = event.position_id;
        self.trade_ids.push(event.trade_id);
        self.last_trade_id = Some(event.trade_id);
        self.liquidity_side = Some(event.liquidity_side);
        self.filled_qty += &event.last_qty;
        self.leaves_qty -= &event.last_qty;
        self.ts_last = event.ts_event;
        self.set_avg_px(&event.last_qty, &event.last_px);
        // self.set_slippage(); // TODO
    }

    fn set_avg_px(&mut self, last_qty: &Quantity, last_px: &Price) {
        if self.avg_px.is_none() {
            self.avg_px = Some(last_px.as_f64());
        }

        let filled_qty = self.filled_qty.as_f64();
        let total_qty = filled_qty + last_qty.as_f64();

        let avg_px = self
            .avg_px
            .unwrap()
            .mul_add(filled_qty, last_px.as_f64() * last_qty.as_f64())
            / total_qty;
        self.avg_px = Some(avg_px);
    }

    // TODO
    // fn set_slippage(&mut self) {
    //     if self.has_price {
    //         self.slippage = self.avg_px.and_then(|avg_px| {
    //             self.price
    //                 .as_ref()
    //                 .map(|price| fixed_i64_to_f64(price.raw))
    //                 .and_then(|price| match self.side() {
    //                     OrderSide::Buy if avg_px > price => Some(avg_px - price),
    //                     OrderSide::Sell if avg_px < price => Some(price - avg_px),
    //                     _ => None,
    //                 })
    //         })
    //     }
    // }

    fn opposite_side(&self, side: OrderSide) -> OrderSide {
        match side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        }
    }

    fn closing_side(&self, side: PositionSide) -> OrderSide {
        match side {
            PositionSide::Long => OrderSide::Sell,
            PositionSide::Short => OrderSide::Buy,
            PositionSide::Flat => OrderSide::NoOrderSide,
            PositionSide::NoPositionSide => OrderSide::NoOrderSide,
        }
    }

    fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
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
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{OrderSide, OrderStatus, PositionSide},
        events::order::{OrderDeniedBuilder, OrderEvent, OrderInitializedBuilder},
        orders::market::MarketOrder,
    };

    #[rstest(
        order_side,
        expected_side,
        case(OrderSide::Buy, OrderSide::Sell),
        case(OrderSide::Sell, OrderSide::Buy),
        case(OrderSide::NoOrderSide, OrderSide::NoOrderSide)
    )]
    fn test_order_opposite_side(order_side: OrderSide, expected_side: OrderSide) {
        let order = MarketOrder::default();
        let result = order.opposite_side(order_side);
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
        let order = MarketOrder::default();
        let result = order.closing_side(position_side);
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
        let order: MarketOrder = OrderInitializedBuilder::default()
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
        let mut order: MarketOrder = init.into();
        let event = OrderEvent::OrderDenied(denied);

        let _ = order.apply(event.clone());

        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());
        assert!(!order.is_open());
        assert_eq!(order.event_count(), 1);
        assert_eq!(order.last_event(), &event);
    }

    // #[test]
    // fn test_buy_order_life_cycle_to_filled() {
    //     let init = OrderInitializedBuilder::default().build().unwrap();
    //     let submitted = OrderSubmittedBuilder::default().build().unwrap();
    //     let accepted = OrderAcceptedBuilder::default().build().unwrap();
    //
    //     // TODO: We should derive defaults for the below
    //     let filled = OrderFilledBuilder::default()
    //         .trader_id(TraderId::default())
    //         .strategy_id(StrategyId::default())
    //         .instrument_id(InstrumentId::default())
    //         .account_id(AccountId::default())
    //         .client_order_id(ClientOrderId::default())
    //         .venue_order_id(VenueOrderId::default())
    //         .position_id(None)
    //         .order_side(OrderSide::Buy)
    //         .order_type(OrderType::Market)
    //         .trade_id(TradeId::new("001"))
    //         .event_id(UUID4::default())
    //         .ts_event(UnixNanos::default())
    //         .ts_init(UnixNanos::default())
    //         .reconciliation(false)
    //         .build()
    //         .unwrap();
    //
    //     let client_order_id = init.client_order_id;
    //     let mut order: MarketOrder = init.into();
    //     let _ = order.apply(OrderEvent::OrderSubmitted(submitted));
    //     let _ = order.apply(OrderEvent::OrderAccepted(accepted));
    //     let _ = order.apply(OrderEvent::OrderFilled(filled));
    //
    //     assert_eq!(order.client_order_id, client_order_id);
    // }
}

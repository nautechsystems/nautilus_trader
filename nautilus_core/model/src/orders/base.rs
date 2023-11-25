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
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror;
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::order::{
        accepted::OrderAccepted, cancel_rejected::OrderCancelRejected, canceled::OrderCanceled,
        denied::OrderDenied, emulated::OrderEmulated, event::OrderEvent, expired::OrderExpired,
        filled::OrderFilled, initialized::OrderInitialized, modified_rejected::OrderModifyRejected,
        pending_cancel::OrderPendingCancel, pending_update::OrderPendingUpdate,
        rejected::OrderRejected, released::OrderReleased, submitted::OrderSubmitted,
        triggered::OrderTriggered, updated::OrderUpdated,
    },
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, position_id::PositionId,
        strategy_id::StrategyId, symbol::Symbol, trade_id::TradeId, trader_id::TraderId,
        venue::Venue, venue_order_id::VenueOrderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[derive(thiserror::Error, Debug)]
pub enum OrderError {
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("Invalid event for order type")]
    InvalidOrderEvent,
    #[error("Unrecognized event")]
    UnrecognizedEvent,
    #[error("No previous state")]
    NoPreviousState,
}

const VALID_STOP_ORDER_TYPES: &[OrderType] = &[
    OrderType::StopMarket,
    OrderType::StopLimit,
    OrderType::MarketIfTouched,
    OrderType::LimitIfTouched,
];

const VALID_LIMIT_ORDER_TYPES: &[OrderType] = &[
    OrderType::Limit,
    OrderType::StopLimit,
    OrderType::LimitIfTouched,
    OrderType::MarketIfTouched,
];

pub fn ustr_hashmap_to_str(h: HashMap<Ustr, Ustr>) -> HashMap<String, String> {
    h.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn str_hashmap_to_ustr(h: HashMap<String, String>) -> HashMap<Ustr, Ustr> {
    h.into_iter()
        .map(|(k, v)| (Ustr::from(&k), Ustr::from(&v)))
        .collect()
}

impl OrderStatus {
    #[rustfmt::skip]
    pub fn transition(&mut self, event: &OrderEvent) -> Result<OrderStatus, OrderError> {
        let new_state = match (self, event) {
            (OrderStatus::Initialized, OrderEvent::OrderDenied(_)) => OrderStatus::Denied,
            (OrderStatus::Initialized, OrderEvent::OrderEmulated(_)) => OrderStatus::Emulated,  // Emulated orders
            (OrderStatus::Initialized, OrderEvent::OrderReleased(_)) => OrderStatus::Released,  // Emulated orders
            (OrderStatus::Initialized, OrderEvent::OrderSubmitted(_)) => OrderStatus::Submitted,
            (OrderStatus::Initialized, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,  // External orders
            (OrderStatus::Initialized, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,  // External orders
            (OrderStatus::Initialized, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // External orders
            (OrderStatus::Initialized, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,  // External orders
            (OrderStatus::Initialized, OrderEvent::OrderTriggered(_)) => OrderStatus::Triggered, // External orders
            (OrderStatus::Emulated, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // Emulated orders
            (OrderStatus::Emulated, OrderEvent::OrderExpired(_)) => OrderStatus::Expired,  // Emulated orders
            (OrderStatus::Emulated, OrderEvent::OrderReleased(_)) => OrderStatus::Released,  // Emulated orders
            (OrderStatus::Released, OrderEvent::OrderSubmitted(_)) => OrderStatus::Submitted,  // Emulated orders
            (OrderStatus::Released, OrderEvent::OrderDenied(_)) => OrderStatus::Denied,  // Emulated orders
            (OrderStatus::Released, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // Execution algo
            (OrderStatus::Submitted, OrderEvent::OrderPendingUpdate(_)) => OrderStatus::PendingUpdate,
            (OrderStatus::Submitted, OrderEvent::OrderPendingCancel(_)) => OrderStatus::PendingCancel,
            (OrderStatus::Submitted, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,
            (OrderStatus::Submitted, OrderEvent::OrderCanceled(_)) => OrderStatus::Canceled,  // FOK and IOC cases
            (OrderStatus::Submitted, OrderEvent::OrderAccepted(_)) => OrderStatus::Accepted,
            (OrderStatus::Submitted, OrderEvent::OrderPartiallyFilled(_)) => OrderStatus::PartiallyFilled,
            (OrderStatus::Submitted, OrderEvent::OrderFilled(_)) => OrderStatus::Filled,
            (OrderStatus::Accepted, OrderEvent::OrderRejected(_)) => OrderStatus::Rejected,  // StopLimit order
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
    fn symbol(&self) -> Symbol;
    fn venue(&self) -> Venue;
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
    fn exec_algorithm_params(&self) -> Option<HashMap<Ustr, Ustr>>;
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn tags(&self) -> Option<Ustr>;
    fn filled_qty(&self) -> Quantity;
    fn leaves_qty(&self) -> Quantity;
    fn avg_px(&self) -> Option<f64>;
    fn slippage(&self) -> Option<f64>;
    fn init_id(&self) -> UUID4;
    fn ts_init(&self) -> UnixNanos;
    fn ts_last(&self) -> UnixNanos;

    fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError>;
    fn update(&mut self, event: &OrderUpdated);

    fn events(&self) -> Vec<&OrderEvent>;
    fn last_event(&self) -> &OrderEvent {
        // SAFETY: Unwrap safe as `Order` specification guarantees at least one event (`OrderInitialized`)
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
        self.status() == OrderStatus::Emulated
    }

    fn is_active_local(&self) -> bool {
        matches!(
            self.status(),
            OrderStatus::Initialized | OrderStatus::Emulated | OrderStatus::Released
        )
    }

    fn is_primary(&self) -> bool {
        // TODO: Guarantee `exec_spawn_id` is some if `exec_algorithm_id` is some
        self.exec_algorithm_id().is_some()
            && self.client_order_id() == self.exec_spawn_id().unwrap()
    }

    fn is_secondary(&self) -> bool {
        // TODO: Guarantee `exec_spawn_id` is some if `exec_algorithm_id` is some
        self.exec_algorithm_id().is_some()
            && self.client_order_id() != self.exec_spawn_id().unwrap()
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

    fn is_canceled(&self) -> bool {
        self.status() == OrderStatus::Canceled
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderCore {
    pub events: Vec<OrderEvent>,
    pub commissions: HashMap<Currency, Money>,
    pub venue_order_ids: Vec<VenueOrderId>,
    pub trade_ids: Vec<TradeId>,
    pub previous_status: Option<OrderStatus>,
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
    pub emulation_trigger: Option<TriggerType>,
    pub contingency_type: Option<ContingencyType>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub exec_algorithm_params: Option<HashMap<Ustr, Ustr>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<Ustr>,
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
        emulation_trigger: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Ustr>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            events: Vec::new(),
            commissions: HashMap::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            previous_status: None,
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
            emulation_trigger,
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

    pub fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError> {
        assert_eq!(self.client_order_id, event.client_order_id());
        assert_eq!(self.strategy_id, event.strategy_id());

        let new_status = self.status.transition(&event)?;
        self.previous_status = Some(self.status);
        self.status = new_status;

        match &event {
            OrderEvent::OrderDenied(event) => self.denied(event),
            OrderEvent::OrderEmulated(event) => self.emulated(event),
            OrderEvent::OrderReleased(event) => self.released(event),
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
            OrderEvent::OrderFilled(event) => self.filled(event),
            _ => return Err(OrderError::UnrecognizedEvent),
        }

        self.ts_last = event.ts_event();
        self.events.push(event);
        Ok(())
    }

    fn denied(&self, _event: &OrderDenied) {
        // Do nothing else
    }

    fn emulated(&self, _event: &OrderEmulated) {
        // Do nothing else
    }

    fn released(&mut self, _event: &OrderReleased) {
        self.emulation_trigger = None;
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
        self.status = self
            .previous_status
            .unwrap_or_else(|| panic!("{}", OrderError::NoPreviousState));
    }

    fn cancel_rejected(&mut self, _event: &OrderCancelRejected) {
        self.status = self
            .previous_status
            .unwrap_or_else(|| panic!("{}", OrderError::NoPreviousState));
    }

    fn triggered(&mut self, _event: &OrderTriggered) {}

    fn canceled(&mut self, _event: &OrderCanceled) {}

    fn expired(&mut self, _event: &OrderExpired) {}

    fn updated(&mut self, event: &OrderUpdated) {
        if let Some(venue_order_id) = &event.venue_order_id {
            if self.venue_order_id.is_none()
                || venue_order_id != self.venue_order_id.as_ref().unwrap()
            {
                self.venue_order_id = Some(*venue_order_id);
                self.venue_order_ids.push(*venue_order_id);
            }
        }
    }

    fn filled(&mut self, event: &OrderFilled) {
        self.venue_order_id = Some(event.venue_order_id);
        self.position_id = event.position_id;
        self.trade_ids.push(event.trade_id);
        self.last_trade_id = Some(event.trade_id);
        self.liquidity_side = Some(event.liquidity_side);
        self.filled_qty += event.last_qty;
        self.leaves_qty -= event.last_qty;
        self.ts_last = event.ts_event;
        self.set_avg_px(event.last_qty, event.last_px);
    }

    fn set_avg_px(&mut self, last_qty: Quantity, last_px: Price) {
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

    pub fn set_slippage(&mut self, price: Price) {
        self.slippage = self.avg_px.and_then(|avg_px| {
            let current_price = price.as_f64();
            match self.side {
                OrderSide::Buy if avg_px > current_price => Some(avg_px - current_price),
                OrderSide::Sell if avg_px < current_price => Some(current_price - avg_px),
                _ => None,
            }
        })
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

    pub fn signed_decimal_qty(&self) -> Decimal {
        match self.side {
            OrderSide::Buy => self.quantity.as_decimal(),
            OrderSide::Sell => -self.quantity.as_decimal(),
            _ => panic!("invalid order side"),
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

    pub fn commission(&self, currency: &Currency) -> Option<Money> {
        self.commissions.get(currency).copied()
    }

    pub fn commissions(&self) -> HashMap<Currency, Money> {
        self.commissions.clone()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        enums::{OrderSide, OrderStatus, PositionSide},
        events::order::{
            accepted::OrderAcceptedBuilder, denied::OrderDeniedBuilder, filled::OrderFilledBuilder,
            initialized::OrderInitializedBuilder, submitted::OrderSubmittedBuilder,
        },
        orders::market::MarketOrder,
    };

    fn test_initialize_market_order() {
        let order = MarketOrder::default();
        assert_eq!(order.events().len(), 1);
        assert_eq!(
            stringify!(order.events().get(0)),
            stringify!(OrderInitialized)
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSide::Sell)]
    #[case(OrderSide::Sell, OrderSide::Buy)]
    #[case(OrderSide::NoOrderSide, OrderSide::NoOrderSide)]
    fn test_order_opposite_side(#[case] order_side: OrderSide, #[case] expected_side: OrderSide) {
        let result = OrderCore::opposite_side(order_side);
        assert_eq!(result, expected_side)
    }

    #[rstest]
    #[case(PositionSide::Long, OrderSide::Sell)]
    #[case(PositionSide::Short, OrderSide::Buy)]
    #[case(PositionSide::NoPositionSide, OrderSide::NoOrderSide)]
    fn test_closing_side(#[case] position_side: PositionSide, #[case] expected_side: OrderSide) {
        let result = OrderCore::closing_side(position_side);
        assert_eq!(result, expected_side)
    }

    #[rstest]
    #[case(OrderSide::Buy, dec!(10_000))]
    #[case(OrderSide::Sell, dec!(-10_000))]
    fn test_signed_decimal_qty(#[case] order_side: OrderSide, #[case] expected: Decimal) {
        let order: MarketOrder = OrderInitializedBuilder::default()
            .order_side(order_side)
            .quantity(Quantity::from(10_000))
            .build()
            .unwrap()
            .into();

        let result = order.signed_decimal_qty();
        assert_eq!(result, expected)
    }

    #[rustfmt::skip]
    #[rstest]
    #[case(OrderSide::Buy, Quantity::from(100), PositionSide::Long, Quantity::from(50), false)]
    #[case(OrderSide::Buy, Quantity::from(50), PositionSide::Short, Quantity::from(50), true)]
    #[case(OrderSide::Buy, Quantity::from(50), PositionSide::Short, Quantity::from(100), true)]
    #[case(OrderSide::Buy, Quantity::from(50), PositionSide::Flat, Quantity::from(0), false)]
    #[case(OrderSide::Sell, Quantity::from(50), PositionSide::Flat, Quantity::from(0), false)]
    #[case(OrderSide::Sell, Quantity::from(50), PositionSide::Long, Quantity::from(50), true)]
    #[case(OrderSide::Sell, Quantity::from(50), PositionSide::Long, Quantity::from(100), true)]
    #[case(OrderSide::Sell, Quantity::from(100), PositionSide::Short, Quantity::from(50), false)]
    fn test_would_reduce_only(
        #[case] order_side: OrderSide,
        #[case] order_qty: Quantity,
        #[case] position_side: PositionSide,
        #[case] position_qty: Quantity,
        #[case] expected: bool,
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

    #[rstest]
    fn test_order_state_transition_denied() {
        let mut order: MarketOrder = OrderInitializedBuilder::default().build().unwrap().into();
        let denied = OrderDeniedBuilder::default().build().unwrap();
        let event = OrderEvent::OrderDenied(denied);

        order.apply(event.clone()).unwrap();

        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());
        assert!(!order.is_open());
        assert_eq!(order.event_count(), 1);
        assert_eq!(order.last_event(), &event);
    }

    #[rstest]
    fn test_order_life_cycle_to_filled() {
        let init = OrderInitializedBuilder::default().build().unwrap();
        let submitted = OrderSubmittedBuilder::default().build().unwrap();
        let accepted = OrderAcceptedBuilder::default().build().unwrap();
        let filled = OrderFilledBuilder::default().build().unwrap();

        let mut order: MarketOrder = init.clone().into();
        order.apply(OrderEvent::OrderSubmitted(submitted)).unwrap();
        order.apply(OrderEvent::OrderAccepted(accepted)).unwrap();
        order.apply(OrderEvent::OrderFilled(filled)).unwrap();

        assert_eq!(order.client_order_id, init.client_order_id);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from(100000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        assert_eq!(order.avg_px(), Some(1.0));
        assert!(!order.is_open());
        assert!(order.is_closed());
        assert_eq!(order.commission(&Currency::USD()), None);
        assert_eq!(order.commissions(), HashMap::new());
    }
}

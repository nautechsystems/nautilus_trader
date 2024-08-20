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

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::any::OrderAny;
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::order::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

const STOP_ORDER_TYPES: &[OrderType] = &[
    OrderType::StopMarket,
    OrderType::StopLimit,
    OrderType::MarketIfTouched,
    OrderType::LimitIfTouched,
];

const LIMIT_ORDER_TYPES: &[OrderType] = &[
    OrderType::Limit,
    OrderType::StopLimit,
    OrderType::LimitIfTouched,
    OrderType::MarketIfTouched,
];

const LOCAL_ACTIVE_ORDER_STATUS: &[OrderStatus] = &[
    OrderStatus::Initialized,
    OrderStatus::Emulated,
    OrderStatus::Released,
];

#[derive(thiserror::Error, Debug)]
pub enum OrderError {
    #[error("Order not found: {0}")]
    NotFound(ClientOrderId),
    #[error("Order invariant failed: must have a side for this operation")]
    NoOrderSide,
    #[error("Invalid event for order type")]
    InvalidOrderEvent,
    #[error("Invalid order state transition")]
    InvalidStateTransition,
    #[error("Order was already initialized")]
    AlreadyInitialized,
    #[error("Order had no previous state")]
    NoPreviousState,
}

#[must_use]
pub fn ustr_hashmap_to_str(h: HashMap<Ustr, Ustr>) -> HashMap<String, String> {
    h.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[must_use]
pub fn str_hashmap_to_ustr(h: HashMap<String, String>) -> HashMap<Ustr, Ustr> {
    h.into_iter()
        .map(|(k, v)| (Ustr::from(&k), Ustr::from(&v)))
        .collect()
}

impl OrderStatus {
    #[rustfmt::skip]
    pub fn transition(&mut self, event: &OrderEventAny) -> Result<Self, OrderError> {
        let new_state = match (self, event) {
            (Self::Initialized, OrderEventAny::Denied(_)) => Self::Denied,
            (Self::Initialized, OrderEventAny::Emulated(_)) => Self::Emulated,  // Emulated orders
            (Self::Initialized, OrderEventAny::Released(_)) => Self::Released,  // Emulated orders
            (Self::Initialized, OrderEventAny::Submitted(_)) => Self::Submitted,
            (Self::Initialized, OrderEventAny::Rejected(_)) => Self::Rejected,  // External orders
            (Self::Initialized, OrderEventAny::Accepted(_)) => Self::Accepted,  // External orders
            (Self::Initialized, OrderEventAny::Canceled(_)) => Self::Canceled,  // External orders
            (Self::Initialized, OrderEventAny::Expired(_)) => Self::Expired,  // External orders
            (Self::Initialized, OrderEventAny::Triggered(_)) => Self::Triggered, // External orders
            (Self::Emulated, OrderEventAny::Canceled(_)) => Self::Canceled,  // Emulated orders
            (Self::Emulated, OrderEventAny::Expired(_)) => Self::Expired,  // Emulated orders
            (Self::Emulated, OrderEventAny::Released(_)) => Self::Released,  // Emulated orders
            (Self::Released, OrderEventAny::Submitted(_)) => Self::Submitted,  // Emulated orders
            (Self::Released, OrderEventAny::Denied(_)) => Self::Denied,  // Emulated orders
            (Self::Released, OrderEventAny::Canceled(_)) => Self::Canceled,  // Execution algo
            (Self::Submitted, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Submitted, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Submitted, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::Submitted, OrderEventAny::Canceled(_)) => Self::Canceled,  // FOK and IOC cases
            (Self::Submitted, OrderEventAny::Accepted(_)) => Self::Accepted,
            (Self::Submitted, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::Submitted, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Accepted, OrderEventAny::Rejected(_)) => Self::Rejected,  // StopLimit order
            (Self::Accepted, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Accepted, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Accepted, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::Accepted, OrderEventAny::Triggered(_)) => Self::Triggered,
            (Self::Accepted, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::Accepted, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::Accepted, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Canceled, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,  // Real world possibility
            (Self::Canceled, OrderEventAny::Filled(_)) => Self::Filled,  // Real world possibility
            (Self::PendingUpdate, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::PendingUpdate, OrderEventAny::Accepted(_)) => Self::Accepted,
            (Self::PendingUpdate, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PendingUpdate, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PendingUpdate, OrderEventAny::Triggered(_)) => Self::Triggered,
            (Self::PendingUpdate, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,  // Allow multiple requests
            (Self::PendingUpdate, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::PendingUpdate, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::PendingUpdate, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::PendingCancel, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::PendingCancel, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,  // Allow multiple requests
            (Self::PendingCancel, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PendingCancel, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PendingCancel, OrderEventAny::Accepted(_)) => Self::Accepted,  // Allow failed cancel requests
            (Self::PendingCancel, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::PendingCancel, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Triggered, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::Triggered, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Triggered, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Triggered, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::Triggered, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::Triggered, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::Triggered, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::PartiallyFilled, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::PartiallyFilled, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::PartiallyFilled, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PartiallyFilled, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PartiallyFilled, OrderEventAny::PartiallyFilled(_)) => Self::PartiallyFilled,
            (Self::PartiallyFilled, OrderEventAny::Filled(_)) => Self::Filled,
            _ => return Err(OrderError::InvalidStateTransition),
        };
        Ok(new_state)
    }
}

pub trait Order: 'static + Send {
    fn into_any(self) -> OrderAny;
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
    fn linked_order_ids(&self) -> Option<&[ClientOrderId]>;
    fn parent_order_id(&self) -> Option<ClientOrderId>;
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
    fn exec_algorithm_params(&self) -> Option<&HashMap<Ustr, Ustr>>;
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn tags(&self) -> Option<&[Ustr]>;
    fn filled_qty(&self) -> Quantity;
    fn leaves_qty(&self) -> Quantity;
    fn avg_px(&self) -> Option<f64>;
    fn slippage(&self) -> Option<f64>;
    fn init_id(&self) -> UUID4;
    fn ts_init(&self) -> UnixNanos;
    fn ts_last(&self) -> UnixNanos;

    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError>;
    fn update(&mut self, event: &OrderUpdated);

    fn events(&self) -> Vec<&OrderEventAny>;
    fn last_event(&self) -> &OrderEventAny {
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
        if let Some(emulation_trigger) = self.emulation_trigger() {
            if emulation_trigger != TriggerType::NoTrigger {
                return false;
            }
        }

        matches!(
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
        if let Some(emulation_trigger) = self.emulation_trigger() {
            if emulation_trigger != TriggerType::NoTrigger {
                return false;
            }
        }

        matches!(
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

    fn is_spawned(&self) -> bool {
        self.exec_spawn_id().map_or(false, |exec_spawn_id| {
            exec_spawn_id != self.client_order_id()
        })
    }
}

impl From<OrderAny> for Box<dyn Order> {
    fn from(order: OrderAny) -> Box<dyn Order> {
        match order {
            OrderAny::Limit(order) => Box::new(order),
            OrderAny::LimitIfTouched(order) => Box::new(order),
            OrderAny::Market(order) => Box::new(order),
            OrderAny::MarketIfTouched(order) => Box::new(order),
            OrderAny::MarketToLimit(order) => Box::new(order),
            OrderAny::StopLimit(order) => Box::new(order),
            OrderAny::StopMarket(order) => Box::new(order),
            OrderAny::TrailingStopLimit(order) => Box::new(order),
            OrderAny::TrailingStopMarket(order) => Box::new(order),
        }
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
            linked_order_ids: order.linked_order_ids().map(|x| x.to_vec()),
            parent_order_id: order.parent_order_id(),
            exec_algorithm_id: order.exec_algorithm_id(),
            exec_algorithm_params: order.exec_algorithm_params().map(|x| x.to_owned()),
            exec_spawn_id: order.exec_spawn_id(),
            tags: order.tags().map(|x| x.to_vec()),
            event_id: order.init_id(),
            ts_event: order.ts_init(),
            ts_init: order.ts_init(),
            reconciliation: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderCore {
    pub events: Vec<OrderEventAny>,
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
    pub tags: Option<Vec<Ustr>>,
    pub filled_qty: Quantity,
    pub leaves_qty: Quantity,
    pub avg_px: Option<f64>,
    pub slippage: Option<f64>,
    pub init_id: UUID4,
    pub ts_init: UnixNanos,
    pub ts_last: UnixNanos,
}

impl OrderCore {
    /// Creates a new [`OrderCore`] instance.
    pub fn new(init: OrderInitialized) -> Self {
        let events: Vec<OrderEventAny> = vec![OrderEventAny::Initialized(init.clone())];
        Self {
            events,
            commissions: HashMap::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            previous_status: None,
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
            quantity: init.quantity,
            time_in_force: init.time_in_force,
            liquidity_side: Some(LiquiditySide::NoLiquiditySide),
            is_reduce_only: init.reduce_only,
            is_quote_quantity: init.quote_quantity,
            emulation_trigger: init.emulation_trigger.or(Some(TriggerType::NoTrigger)),
            contingency_type: init
                .contingency_type
                .or(Some(ContingencyType::NoContingency)),
            order_list_id: init.order_list_id,
            linked_order_ids: init.linked_order_ids,
            parent_order_id: init.parent_order_id,
            exec_algorithm_id: init.exec_algorithm_id,
            exec_algorithm_params: init.exec_algorithm_params,
            exec_spawn_id: init.exec_spawn_id,
            tags: init.tags,
            filled_qty: Quantity::zero(init.quantity.precision),
            leaves_qty: init.quantity,
            avg_px: None,
            slippage: None,
            init_id: init.event_id,
            ts_init: init.ts_event,
            ts_last: init.ts_event,
        }
    }

    pub fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        assert_eq!(self.client_order_id, event.client_order_id());
        assert_eq!(self.strategy_id, event.strategy_id());

        let new_status = self.status.transition(&event)?;
        self.previous_status = Some(self.status);
        self.status = new_status;

        match &event {
            OrderEventAny::Initialized(_) => return Err(OrderError::AlreadyInitialized),
            OrderEventAny::Denied(event) => self.denied(event),
            OrderEventAny::Emulated(event) => self.emulated(event),
            OrderEventAny::Released(event) => self.released(event),
            OrderEventAny::Submitted(event) => self.submitted(event),
            OrderEventAny::Rejected(event) => self.rejected(event),
            OrderEventAny::Accepted(event) => self.accepted(event),
            OrderEventAny::PendingUpdate(event) => self.pending_update(event),
            OrderEventAny::PendingCancel(event) => self.pending_cancel(event),
            OrderEventAny::ModifyRejected(event) => self.modify_rejected(event),
            OrderEventAny::CancelRejected(event) => self.cancel_rejected(event),
            OrderEventAny::Updated(event) => self.updated(event),
            OrderEventAny::Triggered(event) => self.triggered(event),
            OrderEventAny::Canceled(event) => self.canceled(event),
            OrderEventAny::Expired(event) => self.expired(event),
            OrderEventAny::PartiallyFilled(event) => self.filled(event),
            OrderEventAny::Filled(event) => self.filled(event),
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
        self.account_id = Some(event.account_id);
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
        });
    }

    #[must_use]
    pub fn opposite_side(side: OrderSide) -> OrderSide {
        match side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        }
    }

    #[must_use]
    pub fn closing_side(side: PositionSide) -> OrderSide {
        match side {
            PositionSide::Long => OrderSide::Sell,
            PositionSide::Short => OrderSide::Buy,
            PositionSide::Flat => OrderSide::NoOrderSide,
            PositionSide::NoPositionSide => OrderSide::NoOrderSide,
        }
    }

    #[must_use]
    pub fn signed_decimal_qty(&self) -> Decimal {
        match self.side {
            OrderSide::Buy => self.quantity.as_decimal(),
            OrderSide::Sell => -self.quantity.as_decimal(),
            _ => panic!("Invalid order side"),
        }
    }

    #[must_use]
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

    #[must_use]
    pub fn commission(&self, currency: &Currency) -> Option<Money> {
        self.commissions.get(currency).copied()
    }

    #[must_use]
    pub fn commissions(&self) -> HashMap<Currency, Money> {
        self.commissions.clone()
    }

    #[must_use]
    pub fn init_event(&self) -> Option<OrderEventAny> {
        self.events.first().cloned()
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
        assert_eq!(result, expected_side);
    }

    #[rstest]
    #[case(PositionSide::Long, OrderSide::Sell)]
    #[case(PositionSide::Short, OrderSide::Buy)]
    #[case(PositionSide::NoPositionSide, OrderSide::NoOrderSide)]
    fn test_closing_side(#[case] position_side: PositionSide, #[case] expected_side: OrderSide) {
        let result = OrderCore::closing_side(position_side);
        assert_eq!(result, expected_side);
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
        assert_eq!(result, expected);
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
        let event = OrderEventAny::Denied(denied);

        order.apply(event.clone()).unwrap();

        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());
        assert!(!order.is_open());
        assert_eq!(order.event_count(), 2);
        assert_eq!(order.last_event(), &event);
    }

    #[rstest]
    fn test_order_life_cycle_to_filled() {
        let init = OrderInitializedBuilder::default().build().unwrap();
        let submitted = OrderSubmittedBuilder::default().build().unwrap();
        let accepted = OrderAcceptedBuilder::default().build().unwrap();
        let filled = OrderFilledBuilder::default().build().unwrap();

        let mut order: MarketOrder = init.clone().into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(filled)).unwrap();

        assert_eq!(order.client_order_id, init.client_order_id);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from(100_000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        assert_eq!(order.avg_px(), Some(1.0));
        assert!(!order.is_open());
        assert!(order.is_closed());
        assert_eq!(order.commission(&Currency::USD()), None);
        assert_eq!(order.commissions(), HashMap::new());
    }
}

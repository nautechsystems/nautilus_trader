// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Order types for the trading domain model.

pub mod any;
#[cfg(any(test, feature = "stubs"))]
pub mod builder;
pub mod limit;
pub mod limit_if_touched;
pub mod list;
pub mod market;
pub mod market_if_touched;
pub mod market_to_limit;
pub mod stop_limit;
pub mod stop_market;
pub mod trailing_stop_limit;
pub mod trailing_stop_market;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

// Re-exports
use ahash::AHashSet;
use enum_dispatch::enum_dispatch;
use indexmap::IndexMap;
use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{CorrectnessError, check_predicate_false},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

#[cfg(any(test, feature = "stubs"))]
pub use crate::orders::builder::OrderTestBuilder;
pub use crate::orders::{
    any::{LimitOrderAny, OrderAny, PassiveOrderAny, StopOrderAny},
    limit::LimitOrder,
    limit_if_touched::LimitIfTouchedOrder,
    list::OrderList,
    market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder,
    market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder,
    stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder,
    trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderSideSpecified, OrderStatus, OrderType,
        PositionSide, TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
    },
    orderbook::OwnBookOrder,
    types::{Currency, Money, Price, Quantity},
};

/// Order types that have stop/trigger prices.
pub const STOP_ORDER_TYPES: &[OrderType] = &[
    OrderType::StopMarket,
    OrderType::StopLimit,
    OrderType::MarketIfTouched,
    OrderType::LimitIfTouched,
];

/// Order types that have limit prices.
pub const LIMIT_ORDER_TYPES: &[OrderType] = &[
    OrderType::Limit,
    OrderType::StopLimit,
    OrderType::LimitIfTouched,
    OrderType::TrailingStopLimit,
];

/// Order types that support the TRIGGERED order status.
///
/// Market-style stops (`StopMarket`, `MarketIfTouched`, `TrailingStopMarket`) execute
/// immediately on trigger and have no intermediate TRIGGERED state.
pub const TRIGGERABLE_ORDER_TYPES: &[OrderType] = &[
    OrderType::StopLimit,
    OrderType::TrailingStopLimit,
    OrderType::LimitIfTouched,
];

/// Order statuses for locally active orders (pre-submission to venue).
pub const LOCAL_ACTIVE_ORDER_STATUSES: &[OrderStatus] = &[
    OrderStatus::Initialized,
    OrderStatus::Emulated,
    OrderStatus::Released,
];

/// Order statuses that are safe for cancellation queries.
///
/// These are statuses where an order is working on the venue but not already
/// in the process of being cancelled. Including `PENDING_CANCEL` in cancellation
/// filters can cause duplicate cancel attempts or incorrect open order counts.
///
/// Note: `PENDING_UPDATE` is included as orders being updated can typically still
/// be cancelled (update and cancel are independent operations on most venues).
pub const CANCELLABLE_ORDER_STATUSES: &[OrderStatus] = &[
    OrderStatus::Accepted,
    OrderStatus::Triggered,
    OrderStatus::PendingUpdate,
    OrderStatus::PartiallyFilled,
];

/// Returns a cached `AHashSet` of cancellable order statuses for O(1) lookups.
///
/// For the small set (4 elements), using `CANCELLABLE_ORDER_STATUSES.contains()` may be
/// equally fast due to better cache locality. Use this function when you need set operations
/// or are building HashSet-based filters.
///
/// Note: This is a module-level convenience function. You can also use
/// `OrderStatus::cancellable_statuses_set()` directly.
#[must_use]
pub fn cancellable_order_statuses_set() -> &'static AHashSet<OrderStatus> {
    OrderStatus::cancellable_statuses_set()
}

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
    #[error("Duplicate fill: trade_id {0} already applied to order")]
    DuplicateFill(TradeId),
    #[error("{0}")]
    Invariant(#[from] CorrectnessError),
}

/// Converts an `IndexMap` with `Ustr` keys and values to `String` keys and values.
#[must_use]
pub fn ustr_indexmap_to_str(h: IndexMap<Ustr, Ustr>) -> IndexMap<String, String> {
    h.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Converts an `IndexMap` with `String` keys and values to `Ustr` keys and values.
#[must_use]
pub fn str_indexmap_to_ustr(h: IndexMap<String, String>) -> IndexMap<Ustr, Ustr> {
    h.into_iter()
        .map(|(k, v)| (Ustr::from(&k), Ustr::from(&v)))
        .collect()
}

#[inline]
pub(crate) fn check_display_qty(
    display_qty: Option<Quantity>,
    quantity: Quantity,
) -> Result<(), OrderError> {
    if let Some(q) = display_qty {
        check_predicate_false(q > quantity, "`display_qty` may not exceed `quantity`")?;
    }
    Ok(())
}

#[inline]
pub(crate) fn check_time_in_force(
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
) -> Result<(), OrderError> {
    check_predicate_false(
        time_in_force == TimeInForce::Gtd && expire_time.unwrap_or_default() == 0,
        "`expire_time` is required for `GTD` order",
    )?;
    Ok(())
}

impl OrderStatus {
    /// Transitions the order state machine based on the given `event`.
    ///
    /// # Errors
    ///
    /// Returns an error if the state transition is invalid from the current status.
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
            (Self::Initialized, OrderEventAny::Updated(_)) => Self::Initialized, // In-place modification
            (Self::Emulated, OrderEventAny::Canceled(_)) => Self::Canceled,  // Emulated orders
            (Self::Emulated, OrderEventAny::Expired(_)) => Self::Expired,  // Emulated orders
            (Self::Emulated, OrderEventAny::Released(_)) => Self::Released,  // Emulated orders
            (Self::Released, OrderEventAny::Submitted(_)) => Self::Submitted,  // Emulated orders
            (Self::Released, OrderEventAny::Denied(_)) => Self::Denied,  // Emulated orders
            (Self::Released, OrderEventAny::Canceled(_)) => Self::Canceled,  // Execution algo
            (Self::Released, OrderEventAny::Updated(_)) => Self::Released, // In-place modification
            (Self::Submitted, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Submitted, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Submitted, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::Submitted, OrderEventAny::Canceled(_)) => Self::Canceled,  // FOK and IOC cases
            (Self::Submitted, OrderEventAny::Accepted(_)) => Self::Accepted,
            (Self::Submitted, OrderEventAny::Updated(_)) => Self::Submitted,
            (Self::Submitted, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Accepted, OrderEventAny::Rejected(_)) => Self::Rejected,  // StopLimit order
            (Self::Accepted, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Accepted, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Accepted, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::Accepted, OrderEventAny::Triggered(_)) => Self::Triggered,
            (Self::Accepted, OrderEventAny::Updated(_)) => Self::Accepted,  // Updates should preserve state
            (Self::Accepted, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::Accepted, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Canceled, OrderEventAny::Filled(_)) => Self::Filled,  // Real world possibility
            (Self::PendingUpdate, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::PendingUpdate, OrderEventAny::Accepted(_)) => Self::Accepted,
            (Self::PendingUpdate, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PendingUpdate, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PendingUpdate, OrderEventAny::Triggered(_)) => Self::Triggered,
            (Self::PendingUpdate, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,  // Allow multiple requests
            (Self::PendingUpdate, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::PendingUpdate, OrderEventAny::ModifyRejected(_)) => Self::PendingUpdate,  // Handled by modify_rejected to restore previous_status
            (Self::PendingUpdate, OrderEventAny::Updated(_)) => Self::PendingUpdate,  // Handled by updated to restore previous_status
            (Self::PendingUpdate, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::PendingCancel, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::PendingCancel, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,  // Allow multiple requests
            (Self::PendingCancel, OrderEventAny::CancelRejected(_)) => Self::PendingCancel,  // Handled by cancel_rejected to restore previous_status
            (Self::PendingCancel, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PendingCancel, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PendingCancel, OrderEventAny::Accepted(_)) => Self::Accepted,  // Allow failed cancel requests
            (Self::PendingCancel, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Triggered, OrderEventAny::Rejected(_)) => Self::Rejected,
            (Self::Triggered, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::Triggered, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::Triggered, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::Triggered, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::Triggered, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::Triggered, OrderEventAny::Updated(_)) => Self::Triggered,
            (Self::PartiallyFilled, OrderEventAny::PendingUpdate(_)) => Self::PendingUpdate,
            (Self::PartiallyFilled, OrderEventAny::PendingCancel(_)) => Self::PendingCancel,
            (Self::PartiallyFilled, OrderEventAny::Canceled(_)) => Self::Canceled,
            (Self::PartiallyFilled, OrderEventAny::Expired(_)) => Self::Expired,
            (Self::PartiallyFilled, OrderEventAny::Filled(_)) => Self::Filled,
            (Self::PartiallyFilled, OrderEventAny::Accepted(_)) => Self::Accepted,
            (Self::PartiallyFilled, OrderEventAny::Updated(_)) => Self::PartiallyFilled,
            _ => return Err(OrderError::InvalidStateTransition),
        };
        Ok(new_state)
    }
}

#[enum_dispatch]
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
    fn order_side(&self) -> OrderSide;
    fn order_type(&self) -> OrderType;
    fn quantity(&self) -> Quantity;
    fn time_in_force(&self) -> TimeInForce;
    fn expire_time(&self) -> Option<UnixNanos>;
    fn price(&self) -> Option<Price>;
    fn trigger_price(&self) -> Option<Price>;
    fn activation_price(&self) -> Option<Price> {
        None
    }
    fn trigger_type(&self) -> Option<TriggerType>;
    fn liquidity_side(&self) -> Option<LiquiditySide>;
    fn is_post_only(&self) -> bool;
    fn is_reduce_only(&self) -> bool;
    fn is_quote_quantity(&self) -> bool;
    fn display_qty(&self) -> Option<Quantity>;
    fn limit_offset(&self) -> Option<Decimal>;
    fn trailing_offset(&self) -> Option<Decimal>;
    fn trailing_offset_type(&self) -> Option<TrailingOffsetType>;
    fn emulation_trigger(&self) -> Option<TriggerType>;
    fn trigger_instrument_id(&self) -> Option<InstrumentId>;
    fn contingency_type(&self) -> Option<ContingencyType>;
    fn order_list_id(&self) -> Option<OrderListId>;
    fn linked_order_ids(&self) -> Option<&[ClientOrderId]>;
    fn parent_order_id(&self) -> Option<ClientOrderId>;
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
    fn exec_algorithm_params(&self) -> Option<&IndexMap<Ustr, Ustr>>;
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn tags(&self) -> Option<&[Ustr]>;
    fn filled_qty(&self) -> Quantity;
    fn leaves_qty(&self) -> Quantity;
    fn overfill_qty(&self) -> Quantity;

    /// Calculates potential overfill quantity without mutating order state.
    fn calculate_overfill(&self, fill_qty: Quantity) -> Quantity {
        let potential_filled = self.filled_qty() + fill_qty;
        let quantity = self.quantity();
        if potential_filled > quantity {
            potential_filled - quantity
        } else {
            Quantity::zero(fill_qty.precision)
        }
    }

    fn avg_px(&self) -> Option<f64>;
    fn slippage(&self) -> Option<f64>;
    fn init_id(&self) -> UUID4;
    fn ts_init(&self) -> UnixNanos;
    fn ts_submitted(&self) -> Option<UnixNanos>;
    fn ts_accepted(&self) -> Option<UnixNanos>;
    fn ts_closed(&self) -> Option<UnixNanos>;
    fn ts_last(&self) -> UnixNanos;

    fn order_side_specified(&self) -> OrderSideSpecified {
        self.order_side().as_specified()
    }
    fn commissions(&self) -> &IndexMap<Currency, Money>;

    /// Applies the `event` to the order.
    ///
    /// # Errors
    ///
    /// Returns an error if the event is invalid for the current order status.
    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError>;
    fn update(&mut self, event: &OrderUpdated);

    fn events(&self) -> Vec<&OrderEventAny>;

    fn last_event(&self) -> &OrderEventAny {
        self.events()
            .last()
            .expect("Order invariant violated: no events")
    }

    fn event_count(&self) -> usize {
        self.events().len()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId>;

    fn trade_ids(&self) -> Vec<&TradeId>;

    fn has_price(&self) -> bool;

    /// Returns `true` if a fill with matching `trade_id`, side, qty, and price already exists.
    fn is_duplicate_fill(&self, fill: &OrderFilled) -> bool {
        self.events().iter().any(|event| {
            if let OrderEventAny::Filled(existing) = event {
                existing.trade_id == fill.trade_id
                    && existing.order_side == fill.order_side
                    && existing.last_qty == fill.last_qty
                    && existing.last_px == fill.last_px
            } else {
                false
            }
        })
    }

    fn is_buy(&self) -> bool {
        self.order_side() == OrderSide::Buy
    }

    fn is_sell(&self) -> bool {
        self.order_side() == OrderSide::Sell
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
        self.exec_algorithm_id().is_some()
            && self
                .exec_spawn_id()
                .is_some_and(|spawn_id| self.client_order_id() == spawn_id)
    }

    fn is_spawned(&self) -> bool {
        self.exec_algorithm_id().is_some()
            && self
                .exec_spawn_id()
                .is_some_and(|spawn_id| self.client_order_id() != spawn_id)
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
        if let Some(emulation_trigger) = self.emulation_trigger()
            && emulation_trigger != TriggerType::NoTrigger
        {
            return false;
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
        if let Some(emulation_trigger) = self.emulation_trigger()
            && emulation_trigger != TriggerType::NoTrigger
        {
            return false;
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

    fn to_own_book_order(&self) -> OwnBookOrder {
        OwnBookOrder::new(
            self.trader_id(),
            self.client_order_id(),
            self.venue_order_id(),
            self.order_side().as_specified(),
            self.price().expect("`OwnBookOrder` must have a price"), // TBD
            self.quantity(),
            self.order_type(),
            self.time_in_force(),
            self.status(),
            self.ts_last(),
            self.ts_accepted().unwrap_or_default(),
            self.ts_submitted().unwrap_or_default(),
            self.ts_init(),
        )
    }

    fn is_triggered(&self) -> Option<bool>; // TODO: Temporary on trait
    fn set_position_id(&mut self, position_id: Option<PositionId>);
    fn set_quantity(&mut self, quantity: Quantity);
    fn set_leaves_qty(&mut self, leaves_qty: Quantity);
    fn set_emulation_trigger(&mut self, emulation_trigger: Option<TriggerType>);
    fn set_is_quote_quantity(&mut self, is_quote_quantity: bool);
    fn set_liquidity_side(&mut self, liquidity_side: LiquiditySide);
    fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool;
    fn previous_status(&self) -> Option<OrderStatus>;
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
            order_side: order.order_side(),
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
    pub commissions: IndexMap<Currency, Money>,
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
    pub exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<Vec<Ustr>>,
    pub filled_qty: Quantity,
    pub leaves_qty: Quantity,
    pub overfill_qty: Quantity,
    pub avg_px: Option<f64>,
    pub slippage: Option<f64>,
    pub init_id: UUID4,
    pub ts_init: UnixNanos,
    pub ts_submitted: Option<UnixNanos>,
    pub ts_accepted: Option<UnixNanos>,
    pub ts_closed: Option<UnixNanos>,
    pub ts_last: UnixNanos,
}

impl OrderCore {
    /// Creates a new [`OrderCore`] instance.
    #[must_use]
    pub fn new(init: OrderInitialized) -> Self {
        let events: Vec<OrderEventAny> = vec![OrderEventAny::Initialized(init.clone())];
        Self {
            events,
            commissions: IndexMap::new(),
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
            overfill_qty: Quantity::zero(init.quantity.precision),
            avg_px: None,
            slippage: None,
            init_id: init.event_id,
            ts_init: init.ts_event,
            ts_submitted: None,
            ts_accepted: None,
            ts_closed: None,
            ts_last: init.ts_event,
        }
    }

    /// Applies the `event` to the order.
    ///
    /// # Errors
    ///
    /// Returns an error if the event is invalid for the current order status, or if
    /// `event.client_order_id()` or `event.strategy_id()` does not match the order.
    pub fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        if self.client_order_id != event.client_order_id() {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "Event client_order_id {} does not match order client_order_id {}",
                    event.client_order_id(),
                    self.client_order_id
                ),
            }
            .into());
        }

        if self.strategy_id != event.strategy_id() {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "Event strategy_id {} does not match order strategy_id {}",
                    event.strategy_id(),
                    self.strategy_id
                ),
            }
            .into());
        }

        // Save current status as previous_status for ALL transitions except:
        // - Initialized (no prior state exists)
        // - ModifyRejected/CancelRejected (need to preserve the pre Pending state)
        // - When already in Pending* state (avoid overwriting the pre Pending state when receiving multiple pending requests)
        if !matches!(
            event,
            OrderEventAny::Initialized(_)
                | OrderEventAny::ModifyRejected(_)
                | OrderEventAny::CancelRejected(_)
        ) && !matches!(
            self.status,
            OrderStatus::PendingUpdate | OrderStatus::PendingCancel
        ) {
            self.previous_status = Some(self.status);
        }

        // Check for duplicate fill before state transition to maintain consistency
        if let OrderEventAny::Filled(fill) = &event
            && self.trade_ids.contains(&fill.trade_id)
        {
            return Err(OrderError::DuplicateFill(fill.trade_id));
        }

        if matches!(event, OrderEventAny::Triggered(_))
            && !TRIGGERABLE_ORDER_TYPES.contains(&self.order_type)
        {
            return Err(OrderError::InvalidOrderEvent);
        }

        let new_status = self.status.transition(&event)?;
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
            OrderEventAny::ModifyRejected(event) => self.modify_rejected(event)?,
            OrderEventAny::CancelRejected(event) => self.cancel_rejected(event)?,
            OrderEventAny::Updated(event) => self.updated(event),
            OrderEventAny::Triggered(event) => self.triggered(event),
            OrderEventAny::Canceled(event) => self.canceled(event),
            OrderEventAny::Expired(event) => self.expired(event),
            OrderEventAny::Filled(event) => self.filled(event),
        }

        self.ts_last = event.ts_event();
        self.events.push(event);
        Ok(())
    }

    fn denied(&mut self, event: &OrderDenied) {
        self.ts_closed = Some(event.ts_event);
    }

    fn emulated(&self, _event: &OrderEmulated) {
        // Do nothing else
    }

    fn released(&mut self, _event: &OrderReleased) {
        self.emulation_trigger = None;
    }

    fn submitted(&mut self, event: &OrderSubmitted) {
        self.account_id = Some(event.account_id);
        self.ts_submitted = Some(event.ts_event);
    }

    fn accepted(&mut self, event: &OrderAccepted) {
        self.account_id = Some(event.account_id);
        self.venue_order_id = Some(event.venue_order_id);
        self.venue_order_ids.push(event.venue_order_id);
        self.ts_accepted = Some(event.ts_event);
    }

    fn rejected(&mut self, event: &OrderRejected) {
        self.ts_closed = Some(event.ts_event);
    }

    fn pending_update(&self, _event: &OrderPendingUpdate) {
        // Do nothing else
    }

    fn pending_cancel(&self, _event: &OrderPendingCancel) {
        // Do nothing else
    }

    fn modify_rejected(&mut self, _event: &OrderModifyRejected) -> Result<(), OrderError> {
        self.status = self.previous_status.ok_or(OrderError::NoPreviousState)?;
        Ok(())
    }

    fn cancel_rejected(&mut self, _event: &OrderCancelRejected) -> Result<(), OrderError> {
        self.status = self.previous_status.ok_or(OrderError::NoPreviousState)?;
        Ok(())
    }

    fn triggered(&self, _event: &OrderTriggered) {}

    fn canceled(&mut self, event: &OrderCanceled) {
        self.ts_closed = Some(event.ts_event);
    }

    fn expired(&mut self, event: &OrderExpired) {
        self.ts_closed = Some(event.ts_event);
    }

    fn updated(&mut self, event: &OrderUpdated) {
        if self.status == OrderStatus::PendingUpdate
            && let Some(previous) = self.previous_status
        {
            self.status = previous;
        }

        if let Some(venue_order_id) = &event.venue_order_id
            && (self.venue_order_id.is_none()
                || venue_order_id != self.venue_order_id.as_ref().unwrap())
        {
            self.venue_order_id = Some(*venue_order_id);
            self.venue_order_ids.push(*venue_order_id);
        }

        self.is_quote_quantity = event.is_quote_quantity;
    }

    fn filled(&mut self, event: &OrderFilled) {
        // Use saturating arithmetic to prevent overflow
        let new_filled_qty = Quantity::from_raw(
            self.filled_qty.raw.saturating_add(event.last_qty.raw),
            self.filled_qty.precision,
        );

        // Calculate overfill if any
        if new_filled_qty > self.quantity {
            let overfill_raw = new_filled_qty.raw - self.quantity.raw;
            self.overfill_qty = Quantity::from_raw(
                self.overfill_qty.raw.saturating_add(overfill_raw),
                self.filled_qty.precision,
            );
        }

        if new_filled_qty < self.quantity {
            self.status = OrderStatus::PartiallyFilled;
        } else {
            self.status = OrderStatus::Filled;
            self.ts_closed = Some(event.ts_event);
        }

        self.venue_order_id = Some(event.venue_order_id);
        self.position_id = event.position_id;
        self.trade_ids.push(event.trade_id);
        self.last_trade_id = Some(event.trade_id);
        self.liquidity_side = Some(event.liquidity_side);
        self.filled_qty = new_filled_qty;
        self.leaves_qty = self.leaves_qty.saturating_sub(event.last_qty);
        self.ts_last = event.ts_event;

        if self.ts_accepted.is_none() {
            // Set ts_accepted to time of first fill if not previously set
            self.ts_accepted = Some(event.ts_event);
        }

        self.set_avg_px(event.last_qty, event.last_px);

        debug_assert!(
            matches!(
                self.status,
                OrderStatus::PartiallyFilled | OrderStatus::Filled
            ),
            "Invariant: status must be PartiallyFilled or Filled after fill handler (status={:?})",
            self.status
        );
        debug_assert!(
            self.venue_order_id.is_some()
                && self.last_trade_id.is_some()
                && !self.trade_ids.is_empty(),
            "Invariant: venue_order_id, last_trade_id and trade_ids must be set after fill"
        );
        debug_assert!(
            self.filled_qty.raw.saturating_add(self.leaves_qty.raw) >= self.quantity.raw,
            "Invariant: filled_qty + leaves_qty >= quantity (filled={}, leaves={}, quantity={})",
            self.filled_qty,
            self.leaves_qty,
            self.quantity
        );
    }

    fn set_avg_px(&mut self, last_qty: Quantity, last_px: Price) {
        if self.avg_px.is_none() {
            self.avg_px = Some(last_px.as_f64());
            return;
        }

        // Use previous filled quantity (before current fill) to avoid double-counting
        let prev_filled_qty = (self.filled_qty - last_qty).as_f64();
        let last_qty_f64 = last_qty.as_f64();
        let total_qty = prev_filled_qty + last_qty_f64;

        debug_assert!(
            total_qty > 0.0,
            "Invariant: avg_px calc requires positive total_qty (prev={prev_filled_qty}, last={last_qty_f64})"
        );

        let avg_px = self
            .avg_px
            .unwrap()
            .mul_add(prev_filled_qty, last_px.as_f64() * last_qty_f64)
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

    /// Returns the opposite order side.
    #[must_use]
    pub fn opposite_side(side: OrderSide) -> OrderSide {
        match side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        }
    }

    /// Returns the order side needed to close a position.
    #[must_use]
    pub fn closing_side(side: PositionSide) -> OrderSide {
        match side {
            PositionSide::Long => OrderSide::Sell,
            PositionSide::Short => OrderSide::Buy,
            PositionSide::Flat => OrderSide::NoOrderSide,
            PositionSide::NoPositionSide => OrderSide::NoOrderSide,
        }
    }

    /// # Panics
    ///
    /// Panics if the order side is neither `Buy` nor `Sell`.
    #[must_use]
    pub fn signed_decimal_qty(&self) -> Decimal {
        match self.side {
            OrderSide::Buy => self.quantity.as_decimal(),
            OrderSide::Sell => -self.quantity.as_decimal(),
            OrderSide::NoOrderSide => panic!("Invalid order side"),
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
    pub fn commissions(&self) -> IndexMap<Currency, Money> {
        self.commissions.clone()
    }

    #[must_use]
    pub fn commissions_vec(&self) -> Vec<Money> {
        self.commissions.values().copied().collect()
    }

    #[must_use]
    pub fn init_event(&self) -> Option<OrderEventAny> {
        self.events.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        enums::{OrderSide, OrderStatus, PositionSide, TriggerType},
        events::order::spec::{
            OrderAcceptedSpec, OrderCanceledSpec, OrderDeniedSpec, OrderFilledSpec,
            OrderInitializedSpec, OrderPendingUpdateSpec, OrderSubmittedSpec, OrderTriggeredSpec,
            OrderUpdatedSpec,
        },
        identifiers::InstrumentId,
        orders::{MarketOrder, builder::OrderTestBuilder},
        types::{Price, Quantity},
    };

    // TODO: WIP
    // fn test_display_market_order() {
    //     let order = MarketOrder::default();
    //     assert_eq!(order.events().len(), 1);
    //     assert_eq!(
    //         stringify!(order.events().get(0)),
    //         stringify!(OrderInitialized)
    //     );
    // }

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
        let order: MarketOrder = OrderInitializedSpec::builder()
            .order_side(order_side)
            .quantity(Quantity::from(10_000))
            .build()
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
        let order: MarketOrder = OrderInitializedSpec::builder()
            .order_side(order_side)
            .quantity(order_qty)
            .build()
            .into();

        assert_eq!(
            order.would_reduce_only(position_side, position_qty),
            expected
        );
    }

    #[rstest]
    fn test_order_state_transition_denied() {
        let mut order: MarketOrder = OrderInitializedSpec::builder().build().into();
        let denied = OrderDeniedSpec::builder().build();
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
        let init = OrderInitializedSpec::builder().build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let filled = OrderFilledSpec::builder().build();

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
        assert_eq!(order.commissions(), &IndexMap::new());
    }

    #[rstest]
    fn test_order_life_cycle_fills_with_negative_prices() {
        // Options and spreads can legitimately trade at negative prices. The
        // weighted average-price update must not panic when `last_px` or the
        // prior `avg_px` is below zero.
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let fill1 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .last_px(Price::from("-5.00000"))
            .trade_id(TradeId::from("TRADE-1"))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .last_px(Price::from("-7.00000"))
            .trade_id(TradeId::from("TRADE-2"))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(fill1)).unwrap();
        order.apply(OrderEventAny::Filled(fill2)).unwrap();

        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from(100_000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        // Weighted avg: (50_000 * -5.0 + 50_000 * -7.0) / 100_000 = -6.0
        assert_eq!(order.avg_px(), Some(-6.0));
    }

    #[rstest]
    fn test_order_state_transition_to_canceled() {
        let mut order: MarketOrder = OrderInitializedSpec::builder().build().into();
        let submitted = OrderSubmittedSpec::builder().build();
        let canceled = OrderCanceledSpec::builder().build();

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Canceled(canceled)).unwrap();

        assert_eq!(order.status(), OrderStatus::Canceled);
        assert!(order.is_closed());
        assert!(!order.is_open());
    }

    #[rstest]
    fn test_order_life_cycle_to_partially_filled() {
        let init = OrderInitializedSpec::builder().build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let filled = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .build();

        let mut order: MarketOrder = init.clone().into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(filled)).unwrap();

        assert_eq!(order.client_order_id, init.client_order_id);
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert_eq!(order.leaves_qty(), Quantity::from(50_000));
        assert!(order.is_open());
        assert!(!order.is_closed());
    }

    #[rstest]
    fn test_order_commission_calculation() {
        let mut order: MarketOrder = OrderInitializedSpec::builder().build().into();
        order
            .commissions
            .insert(Currency::USD(), Money::new(10.0, Currency::USD()));

        assert_eq!(
            order.commission(&Currency::USD()),
            Some(Money::new(10.0, Currency::USD()))
        );
        assert_eq!(
            order.commissions_vec(),
            vec![Money::new(10.0, Currency::USD())]
        );
    }

    #[rstest]
    fn test_order_is_primary() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .exec_algorithm_id(ExecAlgorithmId::from("ALGO-001"))
            .exec_spawn_id(ClientOrderId::from("O-001"))
            .client_order_id(ClientOrderId::from("O-001"))
            .build()
            .into();

        assert!(order.is_primary());
        assert!(!order.is_spawned());
    }

    #[rstest]
    fn test_order_is_spawned() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .exec_algorithm_id(ExecAlgorithmId::from("ALGO-001"))
            .exec_spawn_id(ClientOrderId::from("O-002"))
            .client_order_id(ClientOrderId::from("O-001"))
            .build()
            .into();

        assert!(!order.is_primary());
        assert!(order.is_spawned());
    }

    #[rstest]
    fn test_order_is_contingency() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .contingency_type(ContingencyType::Oto)
            .build()
            .into();

        assert!(order.is_contingency());
        assert!(order.is_parent_order());
        assert!(!order.is_child_order());
    }

    #[rstest]
    fn test_order_is_child_order() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .parent_order_id(ClientOrderId::from("PARENT-001"))
            .build()
            .into();

        assert!(order.is_child_order());
        assert!(!order.is_parent_order());
    }

    #[rstest]
    fn test_to_own_book_order_timestamp_ordering() {
        use crate::orders::limit::LimitOrder;

        // Create order with distinct timestamps to verify parameter ordering
        let init = OrderInitializedSpec::builder()
            .price(Price::from("100.00"))
            .build();
        let submitted = OrderSubmittedSpec::builder()
            .ts_event(UnixNanos::from(1_000_000))
            .build();
        let accepted = OrderAcceptedSpec::builder()
            .ts_event(UnixNanos::from(2_000_000))
            .build();

        let mut order: LimitOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let own_book_order = order.to_own_book_order();

        // Verify timestamps are in correct positions
        assert_eq!(own_book_order.ts_submitted, UnixNanos::from(1_000_000));
        assert_eq!(own_book_order.ts_accepted, UnixNanos::from(2_000_000));
        assert_eq!(own_book_order.ts_last, UnixNanos::from(2_000_000));
    }

    #[rstest]
    fn test_order_accepted_without_submitted_sets_account_id() {
        // Test external order flow: Initialized -> Accepted (no Submitted)
        let init = OrderInitializedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder()
            .account_id(AccountId::from("EXTERNAL-001"))
            .build();

        let mut order: MarketOrder = init.into();

        // Verify account_id is initially None
        assert_eq!(order.account_id(), None);

        // Apply accepted event directly (external order case)
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        // Verify account_id is now set from the accepted event
        assert_eq!(order.account_id(), Some(AccountId::from("EXTERNAL-001")));
        assert_eq!(order.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_order_accepted_after_submitted_preserves_account_id() {
        // Test normal order flow: Initialized -> Submitted -> Accepted
        let init = OrderInitializedSpec::builder().build();
        let submitted = OrderSubmittedSpec::builder()
            .account_id(AccountId::from("SUBMITTED-001"))
            .build();
        let accepted = OrderAcceptedSpec::builder()
            .account_id(AccountId::from("ACCEPTED-001"))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        // After submitted, account_id should be set
        assert_eq!(order.account_id(), Some(AccountId::from("SUBMITTED-001")));

        // Apply accepted event
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        // account_id should now be updated to the accepted event's account_id
        assert_eq!(order.account_id(), Some(AccountId::from("ACCEPTED-001")));
        assert_eq!(order.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_overfill_tracks_overfill_qty() {
        // Test that overfill is tracked on the order
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let overfill = OrderFilledSpec::builder()
            .last_qty(Quantity::from(110_000)) // Overfill: 110k > 100k
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(overfill)).unwrap();

        // Order should track overfill
        assert_eq!(order.overfill_qty(), Quantity::from(10_000));
        assert_eq!(order.filled_qty(), Quantity::from(110_000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        assert_eq!(order.status(), OrderStatus::Filled);
    }

    #[rstest]
    fn test_partial_fill_then_overfill() {
        // Test multiple fills resulting in overfill
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let fill1 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(80_000))
            .trade_id(TradeId::from("TRADE-1"))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(30_000)) // Total 110k > 100k
            .trade_id(TradeId::from("TRADE-2"))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(fill1)).unwrap();

        // After first fill, no overfill
        assert_eq!(order.overfill_qty(), Quantity::from(0));
        assert_eq!(order.filled_qty(), Quantity::from(80_000));
        assert_eq!(order.leaves_qty(), Quantity::from(20_000));

        order.apply(OrderEventAny::Filled(fill2)).unwrap();

        // After second fill, overfill detected
        assert_eq!(order.overfill_qty(), Quantity::from(10_000));
        assert_eq!(order.filled_qty(), Quantity::from(110_000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        assert_eq!(order.status(), OrderStatus::Filled);
    }

    #[rstest]
    fn test_exact_fill_no_overfill() {
        // Test that exact fill doesn't trigger overfill tracking
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let filled = OrderFilledSpec::builder()
            .last_qty(Quantity::from(100_000)) // Exact fill
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(filled)).unwrap();

        // No overfill
        assert_eq!(order.overfill_qty(), Quantity::from(0));
        assert_eq!(order.filled_qty(), Quantity::from(100_000));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
    }

    #[rstest]
    fn test_partial_fill_then_overfill_with_fractional_quantities() {
        // Simulates real exchange scenario with fractional fills:
        // Order for 2450.5 units, partially filled 1202.5, then fill of 1285.5 arrives
        // Total filled: 2488.0, overfill: 37.5
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from("2450.5"))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let fill1 = OrderFilledSpec::builder()
            .last_qty(Quantity::from("1202.5"))
            .trade_id(TradeId::from("TRADE-1"))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .last_qty(Quantity::from("1285.5")) // 1202.5 + 1285.5 = 2488 > 2450.5
            .trade_id(TradeId::from("TRADE-2"))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(fill1)).unwrap();

        // After first fill, no overfill
        assert_eq!(order.overfill_qty(), Quantity::from(0));
        assert_eq!(order.filled_qty(), Quantity::from("1202.5"));
        assert_eq!(order.leaves_qty(), Quantity::from("1248.0"));
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);

        order.apply(OrderEventAny::Filled(fill2)).unwrap();

        // After second fill, overfill detected and tracked
        assert_eq!(order.overfill_qty(), Quantity::from("37.5"));
        assert_eq!(order.filled_qty(), Quantity::from("2488.0"));
        assert_eq!(order.leaves_qty(), Quantity::from(0));
        assert_eq!(order.status(), OrderStatus::Filled);
    }

    #[rstest]
    fn test_calculate_overfill_returns_zero_when_no_overfill() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build()
            .into();

        // Fill qty less than order qty - no overfill
        let overfill = order.calculate_overfill(Quantity::from(50_000));
        assert_eq!(overfill, Quantity::from(0));

        // Fill qty equals order qty - no overfill
        let overfill = order.calculate_overfill(Quantity::from(100_000));
        assert_eq!(overfill, Quantity::from(0));
    }

    #[rstest]
    fn test_calculate_overfill_returns_overfill_amount() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build()
            .into();

        // Fill qty exceeds order qty
        let overfill = order.calculate_overfill(Quantity::from(110_000));
        assert_eq!(overfill, Quantity::from(10_000));
    }

    #[rstest]
    fn test_calculate_overfill_accounts_for_existing_fills() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let partial_fill = OrderFilledSpec::builder()
            .last_qty(Quantity::from(60_000))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(partial_fill)).unwrap();

        // Order is 60k filled, 40k remaining
        // Fill of 50k would overfill by 10k
        let overfill = order.calculate_overfill(Quantity::from(50_000));
        assert_eq!(overfill, Quantity::from(10_000));

        // Fill of 40k would not overfill
        let overfill = order.calculate_overfill(Quantity::from(40_000));
        assert_eq!(overfill, Quantity::from(0));
    }

    #[rstest]
    fn test_calculate_overfill_with_fractional_quantities() {
        let order: MarketOrder = OrderInitializedSpec::builder()
            .quantity(Quantity::from("2450.5"))
            .build()
            .into();

        // Simulates the exact scenario from user's log
        // Order for 2450.5, if fill of 2488.0 arrives
        let overfill = order.calculate_overfill(Quantity::from("2488.0"));
        assert_eq!(overfill, Quantity::from("37.5"));
    }

    #[rstest]
    fn test_calculate_overfill_zero_after_fractional_partial_fill() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from("1.000"))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let partial_fill = OrderFilledSpec::builder()
            .last_qty(Quantity::from("0.072"))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(partial_fill)).unwrap();

        // After filling 0.072 of 1.000, another 0.072 fill should not overfill
        let overfill = order.calculate_overfill(Quantity::from("0.072"));
        assert_eq!(overfill, Quantity::from("0.000"));
    }

    #[rstest]
    fn test_duplicate_fill_rejected() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let fill1 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .trade_id(TradeId::from("TRADE-001"))
            .build();
        let fill2_duplicate = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .trade_id(TradeId::from("TRADE-001")) // Same trade_id as fill1
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(fill1)).unwrap();

        // Verify first fill applied successfully
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);

        // Applying duplicate fill should return DuplicateFill error
        let result = order.apply(OrderEventAny::Filled(fill2_duplicate));
        assert!(result.is_err());
        match result.unwrap_err() {
            OrderError::DuplicateFill(trade_id) => {
                assert_eq!(trade_id, TradeId::from("TRADE-001"));
            }
            e => panic!("Expected DuplicateFill error, was: {e:?}"),
        }

        // Order state should be unchanged after rejected duplicate
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    }

    #[rstest]
    fn test_check_display_qty_returns_typed_invariant_with_stable_display() {
        let error = check_display_qty(Some(Quantity::from(2)), Quantity::from(1)).unwrap_err();

        match error {
            OrderError::Invariant(CorrectnessError::PredicateViolation { ref message }) => {
                assert_eq!(message, "`display_qty` may not exceed `quantity`");
            }
            other => panic!("Expected typed invariant error, was: {other:?}"),
        }

        assert_eq!(error.to_string(), "`display_qty` may not exceed `quantity`");
    }

    #[rstest]
    fn test_check_time_in_force_returns_typed_invariant_with_stable_display() {
        let error = check_time_in_force(TimeInForce::Gtd, None).unwrap_err();

        match error {
            OrderError::Invariant(CorrectnessError::PredicateViolation { ref message }) => {
                assert_eq!(message, "`expire_time` is required for `GTD` order");
            }
            other => panic!("Expected typed invariant error, was: {other:?}"),
        }

        assert_eq!(
            error.to_string(),
            "`expire_time` is required for `GTD` order"
        );
    }

    #[rstest]
    fn test_different_trade_ids_allowed() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let fill1 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .trade_id(TradeId::from("TRADE-001"))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .trade_id(TradeId::from("TRADE-002")) // Different trade_id
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(fill1)).unwrap();
        order.apply(OrderEventAny::Filled(fill2)).unwrap();

        // Both fills should be applied
        assert_eq!(order.filled_qty(), Quantity::from(100_000));
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.trade_ids.len(), 2);
    }

    #[rstest]
    fn test_pending_update_order_restores_status_on_updated() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let pending_update = OrderPendingUpdateSpec::builder().build();
        let updated = OrderUpdatedSpec::builder()
            .quantity(Quantity::from(50_000))
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        assert_eq!(order.status(), OrderStatus::Accepted);

        order
            .apply(OrderEventAny::PendingUpdate(pending_update))
            .unwrap();
        assert_eq!(order.status(), OrderStatus::PendingUpdate);

        order.apply(OrderEventAny::Updated(updated)).unwrap();

        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.quantity(), Quantity::from(50_000));
    }

    #[rstest]
    fn test_partially_filled_order_can_be_updated() {
        // Test that a partially filled order can receive an Updated event
        // and remain in PartiallyFilled status
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100_000))
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let partial_fill = OrderFilledSpec::builder()
            .last_qty(Quantity::from(40_000))
            .build();
        let updated = OrderUpdatedSpec::builder()
            .quantity(Quantity::from(80_000)) // Reduce to 80k (still > 40k filled)
            .build();

        let mut order: MarketOrder = init.into();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Filled(partial_fill)).unwrap();

        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty(), Quantity::from(40_000));

        order.apply(OrderEventAny::Updated(updated)).unwrap();

        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.quantity(), Quantity::from(80_000));
        assert_eq!(order.leaves_qty(), Quantity::from(40_000)); // 80k - 40k filled
    }

    #[rstest]
    fn test_triggered_order_can_be_updated() {
        // Test that a triggered order can receive an Updated event
        // and remain in Triggered status
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let triggered = OrderTriggeredSpec::builder().build();
        let updated = OrderUpdatedSpec::builder()
            .quantity(Quantity::from(80_000))
            .build();

        let mut order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument_id)
            .quantity(Quantity::from(100_000))
            .price(Price::from("0.99500"))
            .trigger_price(Price::from("1.00000"))
            .trigger_type(TriggerType::LastPrice)
            .build();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Triggered(triggered)).unwrap();

        assert_eq!(order.status(), OrderStatus::Triggered);

        order.apply(OrderEventAny::Updated(updated)).unwrap();

        assert_eq!(order.status(), OrderStatus::Triggered);
        assert_eq!(order.quantity(), Quantity::from(80_000));
    }

    #[rstest]
    fn test_order_updated_with_is_quote_quantity_clears_flag() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::new(10.0, 6))
            .quote_quantity(true)
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let updated = OrderUpdatedSpec::builder()
            .quantity(Quantity::new(47.393_365, 6))
            .is_quote_quantity(false)
            .build();

        let mut order: MarketOrder = init.into();
        assert!(order.is_quote_quantity());

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Updated(updated)).unwrap();

        assert!(!order.is_quote_quantity());
        assert_eq!(order.quantity(), Quantity::new(47.393_365, 6));
        assert_eq!(order.leaves_qty(), Quantity::new(47.393_365, 6));
    }

    #[rstest]
    fn test_order_updated_default_is_quote_quantity_clears_flag() {
        let init = OrderInitializedSpec::builder()
            .quantity(Quantity::new(10.0, 6))
            .quote_quantity(true)
            .build();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        // Builder defaults is_quote_quantity to false
        let updated = OrderUpdatedSpec::builder()
            .quantity(Quantity::new(8.0, 6))
            .build();

        let mut order: MarketOrder = init.into();
        assert!(order.is_quote_quantity());

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Updated(updated)).unwrap();

        assert!(!order.is_quote_quantity());
        assert_eq!(order.quantity(), Quantity::new(8.0, 6));
    }

    #[rstest]
    fn test_canceled_then_partial_fill_then_canceled() {
        let mut order: MarketOrder = OrderInitializedSpec::builder().build().into();
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let canceled1 = OrderCanceledSpec::builder().build();
        let fill = OrderFilledSpec::builder()
            .last_qty(Quantity::from(50_000))
            .trade_id(TradeId::from("FILL-1"))
            .build();
        let canceled2 = OrderCanceledSpec::builder().build();

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Canceled(canceled1)).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert!(order.is_closed());

        // Fill arrives after cancel (real-world race condition)
        order.apply(OrderEventAny::Filled(fill)).unwrap();
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert!(order.is_open());

        // Re-emitted cancel restores terminal state
        order.apply(OrderEventAny::Canceled(canceled2)).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert!(order.is_closed());
    }

    #[rstest]
    fn test_apply_triggered_to_stop_market_order_returns_error() {
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let triggered = OrderTriggeredSpec::builder().build();

        let mut order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .quantity(Quantity::from(1))
            .trigger_price(Price::from("1.00000"))
            .trigger_type(TriggerType::LastPrice)
            .build();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let result = order.apply(OrderEventAny::Triggered(triggered));
        assert!(result.is_err());
        assert_eq!(order.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_apply_triggered_to_stop_limit_order_succeeds() {
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let submitted = OrderSubmittedSpec::builder().build();
        let accepted = OrderAcceptedSpec::builder().build();
        let triggered = OrderTriggeredSpec::builder().build();

        let mut order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument_id)
            .quantity(Quantity::from(1))
            .price(Price::from("0.99500"))
            .trigger_price(Price::from("1.00000"))
            .trigger_type(TriggerType::LastPrice)
            .build();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        order.apply(OrderEventAny::Triggered(triggered)).unwrap();

        assert_eq!(order.status(), OrderStatus::Triggered);
    }
}

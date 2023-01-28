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

use crate::enums::{
    ContingencyType, CurrencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TriggerType,
};
use crate::identifiers::account_id::AccountId;
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::order_list_id::OrderListId;
use crate::identifiers::position_id::PositionId;
use crate::identifiers::strategy_id::StrategyId;
use crate::identifiers::trade_id::TradeId;
use crate::identifiers::trader_id::TraderId;
use crate::identifiers::venue_order_id::VenueOrderId;
use crate::types::currency::Currency;
use crate::types::money::Money;
use crate::types::price::Price;
use crate::types::quantity::Quantity;
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum OrderEvent {
    OrderInitialized(OrderInitialized),
    OrderDenied(OrderDenied),
    OrderSubmitted(OrderSubmitted),
    OrderAccepted(OrderAccepted),
    OrderRejected(OrderRejected),
    OrderCanceled(OrderCanceled),
    OrderExpired(OrderExpired),
    OrderTriggered(OrderTriggered),
    OrderPendingUpdate(OrderPendingUpdate),
    OrderPendingCancel(OrderPendingCancel),
    OrderModifyRejected(OrderModifyRejected),
    OrderCancelRejected(OrderCancelRejected),
    OrderUpdated(OrderUpdated),
    OrderPartiallyFilled(OrderFilled),
    OrderFilled(OrderFilled),
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderInitialized {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub time_in_force: TimeInForce,
    pub expire_time: Option<UnixNanos>,
    pub post_only: bool,
    pub reduce_only: bool,
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
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderDenied {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderSubmitted {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderAccepted {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderCanceled {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderExpired {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderTriggered {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderPendingUpdate {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderPendingCancel {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderModifyRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderCancelRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderUpdated {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct OrderFilled {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub trade_id: TradeId,
    pub position_id: Option<PositionId>,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub commission: Money,
    pub liquidity_side: LiquiditySide,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

////////////////////////////////////////////////////////////////////////////////
// Builders
////////////////////////////////////////////////////////////////////////////////
pub struct OrderInitializedBuilder {
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    order_side: OrderSide,
    order_type: OrderType,
    quantity: Quantity,
    price: Option<Price>,
    trigger_price: Option<Price>,
    trigger_type: Option<TriggerType>,
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
    post_only: bool,
    reduce_only: bool,
    display_qty: Option<Quantity>,
    limit_offset: Option<Price>,
    trailing_offset: Option<Price>,
    trailing_offset_type: Option<TriggerType>,
    emulation_trigger: Option<TriggerType>,
    contingency_type: Option<ContingencyType>,
    order_list_id: Option<OrderListId>,
    linked_order_ids: Option<Vec<ClientOrderId>>,
    parent_order_id: Option<ClientOrderId>,
    tags: Option<String>,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    reconciliation: bool,
}

impl Default for OrderInitializedBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderInitializedBuilder {
    pub fn new() -> Self {
        OrderInitializedBuilder {
            trader_id: TraderId::new("TRADER-001"),
            strategy_id: StrategyId::new("S-001"),
            instrument_id: InstrumentId::from("AUD/USD.SIM"),
            client_order_id: ClientOrderId::new("O-123456789"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(100_000.0, 0),
            price: None,
            trigger_price: None,
            trigger_type: None,
            time_in_force: TimeInForce::Day,
            expire_time: None,
            post_only: false,
            reduce_only: false,
            display_qty: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
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
        }
    }
    pub fn build(self) -> OrderInitialized {
        OrderInitialized {
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            client_order_id: self.client_order_id,
            order_side: self.order_side,
            order_type: self.order_type,
            quantity: self.quantity,
            price: self.price,
            trigger_price: self.trigger_price,
            trigger_type: self.trigger_type,
            time_in_force: self.time_in_force,
            expire_time: self.expire_time,
            post_only: self.post_only,
            reduce_only: self.reduce_only,
            display_qty: self.display_qty,
            limit_offset: self.limit_offset,
            trailing_offset: self.trailing_offset,
            trailing_offset_type: self.trailing_offset_type,
            emulation_trigger: self.emulation_trigger,
            contingency_type: self.contingency_type,
            order_list_id: self.order_list_id,
            linked_order_ids: self.linked_order_ids,
            parent_order_id: self.parent_order_id,
            tags: self.tags,
            event_id: self.event_id,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
            reconciliation: self.reconciliation,
        }
    }
    pub fn trader_id(mut self, trader_id: TraderId) -> Self {
        self.trader_id = trader_id;
        self
    }
    pub fn strategy_id(mut self, strategy_id: StrategyId) -> Self {
        self.strategy_id = strategy_id;
        self
    }
    pub fn instrument_id(mut self, instrument_id: InstrumentId) -> Self {
        self.instrument_id = instrument_id;
        self
    }
    pub fn order_side(mut self, order_side: OrderSide) -> Self {
        self.order_side = order_side;
        self
    }
    pub fn quantity(mut self, quantity: Quantity) -> Self {
        self.quantity = quantity;
        self
    }
    pub fn time_in_force(mut self, time_if_force: TimeInForce) -> Self {
        self.time_in_force = time_if_force;
        self
    }
    pub fn ts_event(mut self, ts_event: UnixNanos) -> Self {
        self.ts_event = ts_event;
        self
    }
    pub fn ts_init(mut self, ts_init: UnixNanos) -> Self {
        self.ts_init = ts_init;
        self
    }
}

pub struct OrderDeniedBuilder {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderDeniedBuilder {
    pub fn new(init: &OrderInitialized) -> Self {
        OrderDeniedBuilder {
            trader_id: init.trader_id.clone(),
            strategy_id: init.strategy_id.clone(),
            instrument_id: init.instrument_id.clone(),
            client_order_id: init.client_order_id.clone(),
            reason: String::from(""),
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
        }
    }
    pub fn build(self) -> OrderDenied {
        OrderDenied {
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            client_order_id: self.client_order_id,
            reason: self.reason,
            event_id: self.event_id,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
        }
    }
    pub fn reason(mut self, reason: &str) -> Self {
        self.reason = reason.to_string();
        self
    }
    pub fn ts_event(mut self, ts_event: UnixNanos) -> Self {
        self.ts_event = ts_event;
        self
    }
    pub fn ts_init(mut self, ts_init: UnixNanos) -> Self {
        self.ts_init = ts_init;
        self
    }
}

pub struct OrderSubmittedBuilder {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderSubmittedBuilder {
    pub fn new(init: &OrderInitialized) -> Self {
        OrderSubmittedBuilder {
            trader_id: init.trader_id.clone(),
            strategy_id: init.strategy_id.clone(),
            instrument_id: init.instrument_id.clone(),
            client_order_id: init.client_order_id.clone(),
            account_id: AccountId::new("SIM-001"),
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
        }
    }
    pub fn build(self) -> OrderSubmitted {
        OrderSubmitted {
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            client_order_id: self.client_order_id,
            account_id: self.account_id,
            event_id: self.event_id,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
        }
    }
    pub fn ts_event(mut self, ts_event: UnixNanos) -> Self {
        self.ts_event = ts_event;
        self
    }
    pub fn ts_init(mut self, ts_init: UnixNanos) -> Self {
        self.ts_init = ts_init;
        self
    }
}

pub struct OrderAcceptedBuilder {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderAcceptedBuilder {
    pub fn new(event: &OrderSubmitted) -> Self {
        OrderAcceptedBuilder {
            trader_id: event.trader_id.clone(),
            strategy_id: event.strategy_id.clone(),
            instrument_id: event.instrument_id.clone(),
            client_order_id: event.client_order_id.clone(),
            venue_order_id: VenueOrderId::new("001"),
            account_id: event.account_id.clone(),
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
        }
    }
    pub fn build(self) -> OrderAccepted {
        OrderAccepted {
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            client_order_id: self.client_order_id,
            venue_order_id: self.venue_order_id,
            account_id: self.account_id,
            event_id: self.event_id,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
            reconciliation: false,
        }
    }
    pub fn venue_order_id(mut self, venue_order_id: VenueOrderId) -> Self {
        self.venue_order_id = venue_order_id;
        self
    }
    pub fn account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = account_id;
        self
    }
    pub fn ts_event(mut self, ts_event: UnixNanos) -> Self {
        self.ts_event = ts_event;
        self
    }
    pub fn ts_init(mut self, ts_init: UnixNanos) -> Self {
        self.ts_init = ts_init;
        self
    }
}
pub struct OrderFilledBuilder {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub trade_id: TradeId,
    pub position_id: Option<PositionId>,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub commission: Money,
    pub liquidity_side: LiquiditySide,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

impl OrderFilledBuilder {
    pub fn new(init: &OrderInitialized, accepted: &OrderAccepted) -> Self {
        let usd = Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat);
        OrderFilledBuilder {
            trader_id: init.trader_id.clone(),
            strategy_id: init.strategy_id.clone(),
            instrument_id: init.instrument_id.clone(),
            client_order_id: init.client_order_id.clone(),
            venue_order_id: accepted.venue_order_id.clone(),
            account_id: accepted.account_id.clone(),
            trade_id: TradeId::new("001"),
            position_id: None,
            order_side: OrderSide::Buy,
            order_type: init.order_type,
            last_qty: init.quantity.clone(),
            last_px: Price::new(1.0, 0),
            currency: usd.clone(),
            commission: Money::new(0.0, usd),
            liquidity_side: LiquiditySide::Taker,
            event_id: Default::default(),
            ts_event: 0,
            ts_init: 0,
            reconciliation: false,
        }
    }
    pub fn build(self) -> OrderFilled {
        OrderFilled {
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            client_order_id: self.client_order_id,
            venue_order_id: self.venue_order_id,
            account_id: self.account_id,
            trade_id: self.trade_id,
            position_id: self.position_id,
            order_side: self.order_side,
            order_type: self.order_type,
            last_qty: self.last_qty,
            last_px: self.last_px,
            currency: self.currency,
            commission: self.commission,
            liquidity_side: self.liquidity_side,
            event_id: self.event_id,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
            reconciliation: self.reconciliation,
        }
    }
    pub fn trade_id(mut self, trade_id: TradeId) -> Self {
        self.trade_id = trade_id;
        self
    }
    pub fn last_qty(mut self, last_qty: Quantity) -> Self {
        self.last_qty = last_qty;
        self
    }
    pub fn last_px(mut self, last_px: Price) -> Self {
        self.last_px = last_px;
        self
    }
    pub fn commission(mut self, commission: Money) -> Self {
        self.commission = commission;
        self
    }
    pub fn liquidity_side(mut self, liquidity_side: LiquiditySide) -> Self {
        self.liquidity_side = liquidity_side;
        self
    }
    pub fn ts_event(mut self, ts_event: UnixNanos) -> Self {
        self.ts_event = ts_event;
        self
    }
    pub fn ts_init(mut self, ts_init: UnixNanos) -> Self {
        self.ts_init = ts_init;
        self
    }
}

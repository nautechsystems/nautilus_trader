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
    ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TriggerType,
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
use derive_builder::{self, Builder};
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;
use std::ops::Deref;
use std::rc::Rc;

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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
#[builder(default)]
pub struct OrderMetadata {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
}

impl Default for OrderMetadata {
    fn default() -> Self {
        Self {
            trader_id: TraderId::new("TRADER-001"),
            strategy_id: StrategyId::new("S-001"),
            instrument_id: InstrumentId::from("AUD/USD.SIM"),
            client_order_id: ClientOrderId::new("O-123456789"),
        }
    }
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
#[builder(default)]
pub struct OrderInitialized {
    pub metadata: Rc<OrderMetadata>,
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

impl Default for OrderInitialized {
    fn default() -> Self {
        Self {
            metadata: Default::default(),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(100_000.0, 0),
            price: Default::default(),
            trigger_price: Default::default(),
            trigger_type: Default::default(),
            time_in_force: TimeInForce::Day,
            expire_time: Default::default(),
            post_only: Default::default(),
            reduce_only: Default::default(),
            display_qty: Default::default(),
            limit_offset: Default::default(),
            trailing_offset: Default::default(),
            trailing_offset_type: Default::default(),
            emulation_trigger: Default::default(),
            contingency_type: Default::default(),
            order_list_id: Default::default(),
            linked_order_ids: Default::default(),
            parent_order_id: Default::default(),
            tags: Default::default(),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: Default::default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Builder)]
#[builder(default)]
pub struct OrderDenied {
    pub metadata: Rc<OrderMetadata>,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderSubmitted {
    pub metadata: Rc<OrderMetadata>,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderAccepted {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderRejected {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderCanceled {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderExpired {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderTriggered {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderPendingUpdate {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderPendingCancel {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderModifyRejected {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderCancelRejected {
    pub metadata: Rc<OrderMetadata>,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: String,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderUpdated {
    pub metadata: Rc<OrderMetadata>,
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder)]
pub struct OrderFilled {
    pub metadata: Rc<OrderMetadata>,
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

macro_rules! impl_derefs_for_order {
    ($struct: ident) => {
        impl Deref for $struct {
            type Target = OrderMetadata;

            fn deref(&self) -> &Self::Target {
                &self.metadata
            }
        }
    };
}

impl_derefs_for_order!(OrderTriggered);
impl_derefs_for_order!(OrderPendingUpdate);
impl_derefs_for_order!(OrderExpired);
impl_derefs_for_order!(OrderCanceled);
impl_derefs_for_order!(OrderRejected);
impl_derefs_for_order!(OrderAccepted);
impl_derefs_for_order!(OrderSubmitted);
impl_derefs_for_order!(OrderDenied);
impl_derefs_for_order!(OrderInitialized);
impl_derefs_for_order!(OrderPendingCancel);
impl_derefs_for_order!(OrderModifyRejected);
impl_derefs_for_order!(OrderCancelRejected);
impl_derefs_for_order!(OrderUpdated);
impl_derefs_for_order!(OrderFilled);

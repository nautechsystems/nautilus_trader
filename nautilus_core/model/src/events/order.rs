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

use std::ffi::{c_char, CString};

use derive_builder::{self, Builder};
use nautilus_core::string::cstr_to_string;
use nautilus_core::time::UnixNanos;
use nautilus_core::uuid::UUID4;
use serde::{Deserialize, Serialize};
use serde_json;

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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Builder, Serialize, Deserialize)]
#[builder(default)]
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

impl Default for OrderInitialized {
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            strategy_id: StrategyId::default(),
            instrument_id: InstrumentId::default(),
            client_order_id: ClientOrderId::default(),
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
pub struct OrderDenied {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub reason: Box<String>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
pub struct OrderModifyRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: Box<String>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
pub struct OrderCancelRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub reason: Box<String>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
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
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize, Builder)]
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
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_denied_new(
    trader_id: &TraderId,
    strategy_id: &StrategyId,
    instrument_id: &InstrumentId,
    client_order_id: &ClientOrderId,
    reason_ptr: *const c_char,
    event_id: &UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderDenied {
    OrderDenied {
        trader_id: trader_id.clone(),
        strategy_id: strategy_id.clone(),
        instrument_id: instrument_id.clone(),
        client_order_id: client_order_id.clone(),
        reason: Box::new(cstr_to_string(reason_ptr)),
        event_id: event_id.clone(),
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn order_denied_clone(event: &OrderDenied) -> OrderDenied {
    event.clone()
}

/// Frees the memory for the given `account_id` by dropping.
#[no_mangle]
pub extern "C" fn order_denied_drop(event: OrderDenied) {
    drop(event); // Memory freed here
}
#[no_mangle]
pub extern "C" fn order_denied_to_json(event: &OrderDenied) -> *const c_char {
    let json = serde_json::to_string(event).expect("Failed to serialize OrderDenied to JSON");
    let c_string = CString::new(json).expect("Failed to create CString from JSON string");
    c_string.into_raw()
}

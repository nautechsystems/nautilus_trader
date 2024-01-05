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

use std::ffi::c_char;

use nautilus_core::{ffi::string::cstr_to_ustr, time::UnixNanos, uuid::UUID4};

use crate::{
    events::order::{
        accepted::OrderAccepted, denied::OrderDenied, emulated::OrderEmulated,
        rejected::OrderRejected, released::OrderReleased, submitted::OrderSubmitted,
    },
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
    },
    types::price::Price,
};

/// # Safety
///
/// - Assumes valid C string pointers.
// #[no_mangle]
// #[allow(improper_ctypes_definitions)]
// pub unsafe extern "C" fn order_initialized_new(
//     trader_id: TraderId,
//     strategy_id: StrategyId,
//     instrument_id: InstrumentId,
//     client_order_id: ClientOrderId,
//     order_side: OrderSide,
//     order_type: OrderType,
//     quantity: Quantity,
//     price: *const Price,
//     trigger_price: *const Price,
//     trigger_type: TriggerType,
//     limit_offset: *const Price,
//     trailing_offset: *const Price,
//     trailing_offset_type: TrailingOffsetType,
//     time_in_force: TimeInForce,
//     expire_time: *const UnixNanos,
//     post_only: u8,
//     reduce_only: u8,
//     quote_quantity: u8,
//     display_qty: *const Quantity,
//     emulation_trigger: TriggerType,
//     trigger_instrument_id: *const InstrumentId,
//     contingency_type: ContingencyType,
//     order_list_id: *const OrderListId,
//     linked_order_ids: *const c_char,
//     parent_order_id: *const ClientOrderId,
//     exec_algorithm_id: *const ExecAlgorithmId,
//     exec_algorithm_params: *const c_char,
//     exec_spawn_id: *const ClientOrderId,
//     tags: *const c_char,
//     event_id: UUID4,
//     ts_event: UnixNanos,
//     ts_init: UnixNanos,
//     reconciliation: u8,
// ) -> OrderInitialized {
//     OrderInitialized {
//         trader_id,
//         strategy_id,
//         instrument_id,
//         client_order_id,
//         order_side,
//         order_type,
//         quantity,
//         price: if price.is_null() { None } else { Some(*price) },
//         trigger_price: if trigger_price.is_null() {
//             None
//         } else {
//             Some(*trigger_price)
//         },
//         trigger_type: if trigger_type == TriggerType::NoTrigger {
//             None
//         } else {
//             Some(trigger_type)
//         },
//         limit_offset: if limit_offset.is_null() {
//             None
//         } else {
//             Some(*limit_offset)
//         },
//         trailing_offset: if trailing_offset.is_null() {
//             None
//         } else {
//             Some(*trailing_offset)
//         },
//         trailing_offset_type: if trailing_offset_type == TrailingOffsetType::NoTrailingOffset {
//             None
//         } else {
//             Some(trailing_offset_type)
//         },
//         time_in_force,
//         expire_time: if expire_time.is_null() {
//             None
//         } else {
//             Some(*expire_time)
//         },
//         post_only,
//         reduce_only,
//         quote_quantity,
//         display_qty: if display_qty.is_null() {
//             None
//         } else {
//             Some(*display_qty)
//         },
//         emulation_trigger: if emulation_trigger == TriggerType::NoTrigger {
//             None
//         } else {
//             Some(emulation_trigger)
//         },
//         trigger_instrument_id: if trigger_instrument_id.is_null() {
//             None
//         } else {
//             Some(*trigger_instrument_id)
//         },
//         contingency_type: if contingency_type == ContingencyType::NoContingency {
//             None
//         } else {
//             Some(contingency_type)
//         },
//         order_list_id: if order_list_id.is_null() {
//             None
//         } else {
//             Some(*order_list_id)
//         },
//         linked_order_ids: optional_ustr_to_vec_client_order_ids(optional_cstr_to_ustr(
//             linked_order_ids,
//         )),
//         parent_order_id: if parent_order_id.is_null() {
//             None
//         } else {
//             Some(*parent_order_id)
//         },
//         exec_algorithm_id: if exec_algorithm_id.is_null() {
//             None
//         } else {
//             Some(*exec_algorithm_id)
//         },
//         exec_algorithm_params: optional_bytes_to_str_map(exec_algorithm_params),
//         exec_spawn_id: if exec_spawn_id.is_null() {
//             None
//         } else {
//             Some(*exec_spawn_id)
//         },
//         tags: optional_cstr_to_ustr(tags).into(),
//         event_id,
//         ts_event,
//         ts_init,
//         reconciliation,
//     }
// }

/// # Safety
///
/// - Assumes `reason_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_denied_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    reason_ptr: *const c_char,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderDenied {
    OrderDenied {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        reason: cstr_to_ustr(reason_ptr),
        event_id,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn order_emulated_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderEmulated {
    OrderEmulated {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        event_id,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn order_released_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    released_price: Price,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderReleased {
    OrderReleased {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        released_price,
        event_id,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn order_submitted_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderSubmitted {
    OrderSubmitted {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        event_id,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn order_accepted_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    reconciliation: u8,
) -> OrderAccepted {
    OrderAccepted {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        event_id,
        ts_event,
        ts_init,
        reconciliation,
    }
}

/// # Safety
///
/// - Assumes `reason_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_rejected_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    reason_ptr: *const c_char,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    reconciliation: u8,
) -> OrderRejected {
    OrderRejected {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        reason: cstr_to_ustr(reason_ptr),
        event_id,
        ts_event,
        ts_init,
        reconciliation,
    }
}

// #[no_mangle]
// pub unsafe extern "C" fn order_canceled_new(
//     trader_id: TraderId,
//     strategy_id: StrategyId,
//     instrument_id: InstrumentId,
//     client_order_id: ClientOrderId,
//     venue_order_id: VenueOrderId,
//     account_id: AccountId,
//     reconciliation: u8,
//     event_id: UUID4,
//     ts_event: UnixNanos,
//     ts_init: UnixNanos,
// ) -> OrderCanceled {
//     OrderCanceled {
//         trader_id,
//         strategy_id,
//         instrument_id,
//         client_order_id,
//         venue_order_id,
//         account_id,
//         reconciliation,
//         event_id,
//         ts_event,
//         ts_init,
//     }
// }

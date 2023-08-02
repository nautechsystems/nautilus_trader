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

use std::ffi::{c_char, CStr};

use nautilus_core::{string::SerializableUstr, time::UnixNanos, uuid::UUID4};
use ustr::Ustr;

// use crate::types::price::Price;
// use crate::types::quantity::Quantity;
use super::order::OrderDenied;
// use crate::enums::{OrderSide, OrderType, TimeInForce, TriggerType};
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::{
    instrument_id::InstrumentId, strategy_id::StrategyId, trader_id::TraderId,
};

// #[no_mangle]
// pub unsafe extern "C" fn order_initialized_new(
//     trader_id: &TraderId,
//     strategy_id: &StrategyId,
//     instrument_id: &InstrumentId,
//     client_order_id: &ClientOrderId,
//     order_side: OrderSide,
//     order_type: OrderType,
//     quantity: Quantity,
//     price: *const Price,
//     trigger_price: *const Price,
//     trigger_type: TriggerType,
//     time_in_force: TimeInForce,
//     expire_time: *const UnixNanos,
//     post_only: u8,
//     reduce_only: u8,
//     quote_quantity: u8,
//     display_qty: *const Quantity,
//     limit_offset: *const Price,
//     trailing_offset: *const Price,
//     trailing_offset_type: *const TriggerType,
//     event_id: UUID4,
//     ts_event: UnixNanos,
//     ts_init: UnixNanos,
// ) -> OrderInitialized {
//     OrderInitialized {
//         trader_id: trader_id.clone(),
//         strategy_id: strategy_id.clone(),
//         instrument_id: instrument_id.clone(),
//         client_order_id: client_order_id.clone(),
//         order_side,
//         order_type,
//         quantity,
//         price: if price.is_null() {
//             None
//         } else {
//             Some(*price.clone())
//         },
//         trigger_price: if trigger_price.is_null() {
//             None
//         } else {
//             Some(*trigger_price.clone())
//         },
//         trigger_type,
//         time_in_force,
//         expire_time: if expire_time.is_null() {
//             None
//         } else {
//             Some(*expire_time.clone())
//         },
//         post_only: post_only != 0,
//         reduce_only: reduce_only != 0,
//         quote_quantity: quote_quantity != 0,
//         event_id,
//         ts_event,
//         ts_init,
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
    assert!(!reason_ptr.is_null(), "`readon_ptr` was NULL");
    let reason = SerializableUstr(Ustr::from(
        CStr::from_ptr(reason_ptr)
            .to_str()
            .expect("CStr::from_ptr failed"),
    ));
    OrderDenied {
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        reason,
        event_id,
        ts_event,
        ts_init,
    }
}

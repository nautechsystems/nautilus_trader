// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{UUID4, UnixNanos, ffi::string::cstr_to_ustr};

use crate::{
    events::{
        OrderAccepted, OrderDenied, OrderEmulated, OrderRejected, OrderReleased, OrderSubmitted,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    types::Price,
};

/// # Safety
///
/// - Assumes `reason_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
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
        reason: unsafe { cstr_to_ustr(reason_ptr) },
        event_id,
        ts_event,
        ts_init,
    }
}

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
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

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
        reason: unsafe { cstr_to_ustr(reason_ptr) },
        event_id,
        ts_event,
        ts_init,
        reconciliation,
    }
}

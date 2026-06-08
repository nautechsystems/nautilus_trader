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

use std::ffi::c_char;

use nautilus_core::{UUID4, UnixNanos, ffi::string::cstr_to_ustr};
use ustr::Ustr;

use crate::{
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    types::Price,
};

/// Represents an event where an order has been denied by the Nautilus system.
///
/// This could be due an unsupported feature, a risk limit exceedance, or for
/// any other reason that an otherwise valid order is not able to be submitted.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderDeniedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The reason the order was denied.
    pub reason: Ustr,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

/// Represents an event where an order has become emulated by the Nautilus system.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderEmulatedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

/// Represents an event where an order was released from the `OrderEmulated` by the Nautilus system.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderReleasedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    pub released_price: Price,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

/// Represents an event where an order has been submitted by the system to the
/// trading venue.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderSubmittedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

/// Represents an event where an order has been accepted by the trading venue.
///
/// This event often corresponds to a `NEW` `OrdStatus` <39> field in FIX execution reports.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderAcceptedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The venue order ID associated with the event.
    pub venue_order_id: VenueOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// If the event was generated during reconciliation.
    pub reconciliation: u8,
}

/// Represents an event where an order has been rejected by the trading venue.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderRejectedFfi {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The reason the order was rejected.
    pub reason: Ustr,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// If the event was generated during reconciliation.
    pub reconciliation: u8,
    /// If the order was rejected because it was post-only and would execute immediately as a taker.
    pub due_post_only: u8,
}

/// # Safety
///
/// Assumes `reason_ptr` is a valid C string pointer.
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
) -> OrderDeniedFfi {
    OrderDeniedFfi {
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
) -> OrderEmulatedFfi {
    OrderEmulatedFfi {
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
pub extern "C" fn order_released_new(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    released_price: Price,
    event_id: UUID4,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderReleasedFfi {
    OrderReleasedFfi {
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
) -> OrderSubmittedFfi {
    OrderSubmittedFfi {
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
) -> OrderAcceptedFfi {
    OrderAcceptedFfi {
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
/// Assumes `reason_ptr` is a valid C string pointer.
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
    due_post_only: u8,
) -> OrderRejectedFfi {
    OrderRejectedFfi {
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
        due_post_only,
    }
}

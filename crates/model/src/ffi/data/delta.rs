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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;

use crate::{
    data::OrderBookDelta, enums::BookAction, ffi::data::order::BookOrderFfi,
    identifiers::InstrumentId,
};

/// The stable C representation of an [`OrderBookDelta`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderBookDeltaFfi {
    pub instrument_id: InstrumentId,
    pub action: BookAction,
    pub order: BookOrderFfi,
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl From<OrderBookDeltaFfi> for OrderBookDelta {
    fn from(value: OrderBookDeltaFfi) -> Self {
        Self {
            instrument_id: value.instrument_id,
            action: value.action,
            order: value.order.into(),
            flags: value.flags,
            sequence: value.sequence,
            ts_event: value.ts_event,
            ts_init: value.ts_init,
        }
    }
}

impl From<OrderBookDelta> for OrderBookDeltaFfi {
    fn from(value: OrderBookDelta) -> Self {
        Self {
            instrument_id: value.instrument_id,
            action: value.action,
            order: value.order.into(),
            flags: value.flags,
            sequence: value.sequence,
            ts_event: value.ts_event,
            ts_init: value.ts_init,
        }
    }
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_delta_new(
    instrument_id: InstrumentId,
    action: BookAction,
    order: BookOrderFfi,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDeltaFfi {
    OrderBookDelta::new(
        instrument_id,
        action,
        order.into(),
        flags,
        sequence,
        ts_event,
        ts_init,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_eq(lhs: &OrderBookDeltaFfi, rhs: &OrderBookDeltaFfi) -> u8 {
    u8::from(OrderBookDelta::from(*lhs) == OrderBookDelta::from(*rhs))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_hash(delta: &OrderBookDeltaFfi) -> u64 {
    let mut hasher = DefaultHasher::new();
    OrderBookDelta::from(*delta).hash(&mut hasher);
    hasher.finish()
}

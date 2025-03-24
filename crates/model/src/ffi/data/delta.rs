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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;

use crate::{
    data::{BookOrder, OrderBookDelta},
    enums::BookAction,
    identifiers::InstrumentId,
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_delta_new(
    instrument_id: InstrumentId,
    action: BookAction,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_eq(lhs: &OrderBookDelta, rhs: &OrderBookDelta) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_hash(delta: &OrderBookDelta) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}

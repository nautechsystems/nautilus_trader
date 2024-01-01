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

use nautilus_core::{ffi::cvec::CVec, time::UnixNanos};

use crate::{data::deltas::OrderBookDeltas, identifiers::instrument_id::InstrumentId};

#[no_mangle]
pub extern "C" fn orderbook_deltas_new(
    instrument_id: InstrumentId,
    deltas: CVec,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDeltas {
    OrderBookDeltas {
        instrument_id,
        deltas,
        flags,
        sequence,
        ts_event,
        ts_init,
    }
}

// TODO: This struct implementation potentially leaks memory
// TODO: Skip clippy check for now since it requires large modification
#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_deltas_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<OrderBookDeltas> =
        unsafe { Vec::from_raw_parts(ptr as *mut OrderBookDeltas, len, cap) };
    drop(data); // Memory freed here
}

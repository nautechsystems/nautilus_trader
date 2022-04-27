// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::symbol::Symbol;
use crate::identifiers::venue::Venue;
use nautilus_core::buffer::{Buffer16, Buffer32};

////////////////////////////////////////////////////////////////////////////////
// Symbol
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn symbol_free(symbol: Symbol) {
    drop(symbol); // Memory freed here
}

#[no_mangle]
pub extern "C" fn symbol_from_bytes(value: Buffer32) -> Symbol {
    Symbol { value }
}

#[no_mangle]
pub extern "C" fn symbol_to_bytes(symbol: Symbol) -> Buffer32 {
    symbol.value
}

////////////////////////////////////////////////////////////////////////////////
// Venue
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn venue_free(venue: Venue) {
    drop(venue); // Memory freed here
}

#[no_mangle]
pub extern "C" fn venue_from_bytes(value: Buffer16) -> Venue {
    Venue { value }
}

#[no_mangle]
pub extern "C" fn venue_to_bytes(venue: Venue) -> Buffer16 {
    venue.value
}

////////////////////////////////////////////////////////////////////////////////
// InstrumentId
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn instrument_id_free(instrument_id: InstrumentId) {
    drop(instrument_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn instrument_id_from_bytes(
    symbol_value: Buffer32,
    venue_value: Buffer16,
) -> InstrumentId {
    let symbol = Symbol {
        value: symbol_value,
    };
    let venue = Venue { value: venue_value };
    InstrumentId::new(symbol, venue)
}

#[no_mangle]
pub extern "C" fn instrument_id_to_bytes(instrument_id: InstrumentId) -> (Buffer32, Buffer16) {
    (instrument_id.symbol.value, instrument_id.venue.value)
}

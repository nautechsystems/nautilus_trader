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

//! Shared primitives and utilities for the Kraken adapter.

use ahash::AHashMap;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::InstrumentAny,
};

pub mod consts;
pub mod credential;
pub mod enums;
pub mod models;
pub mod order_params;
pub mod parse;
pub mod urls;

/// Looks up a Kraken instrument from a preloaded map snapshot by raw exchange symbol.
pub(crate) fn lookup_instrument_in_snapshot<'a>(
    instruments: &'a AHashMap<InstrumentId, InstrumentAny>,
    raw_symbol: &str,
) -> Option<&'a InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(raw_symbol), *consts::KRAKEN_VENUE);
    instruments.get(&instrument_id)
}

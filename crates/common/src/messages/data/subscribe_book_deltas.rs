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

use std::num::NonZeroUsize;

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
};

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
pub struct SubscribeBookDeltas {
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub managed: bool,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeBookDeltas {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            book_type,
            client_id,
            venue,
            command_id,
            ts_init,
            depth,
            managed,
            params,
        }
    }
}

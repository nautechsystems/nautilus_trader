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
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
};

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
pub struct SubscribeCustomData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeCustomData {
    /// Creates a new [`SubscribeCustomData`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        data_type: DataType,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            client_id,
            venue,
            data_type,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeInstrument {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeInstrument {
    /// Creates a new [`SubscribeInstrument`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeInstruments {
    pub client_id: Option<ClientId>,
    pub venue: Venue,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeInstruments {
    /// Creates a new [`SubscribeInstruments`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Venue,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

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
    /// Creates a new [`SubscribeBookDeltas`] instance.
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

#[derive(Clone, Debug)]
pub struct SubscribeBookDepth10 {
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

impl SubscribeBookDepth10 {
    /// Creates a new [`SubscribeBookDepth10`] instance.
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

#[derive(Clone, Debug)]
pub struct SubscribeBookSnapshots {
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub interval_ms: NonZeroUsize,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeBookSnapshots {
    /// Creates a new [`SubscribeBookSnapshots`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
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
            interval_ms,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeQuotes {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeQuotes {
    /// Creates a new [`SubscribeQuotes`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeTrades {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeTrades {
    /// Creates a new [`SubscribeTrades`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeBars {
    pub bar_type: BarType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeBars {
    /// Creates a new [`SubscribeBars`] instance.
    pub fn new(
        bar_type: BarType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_type,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeMarkPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeMarkPrices {
    /// Creates a new [`SubscribeMarkPrices`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeIndexPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeIndexPrices {
    /// Creates a new [`SubscribeIndexPrices`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeFundingRates {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeFundingRates {
    /// Creates a new [`SubscribeFundingRates`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeInstrumentStatus {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeInstrumentStatus {
    /// Creates a new [`SubscribeInstrumentStatus`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeInstrumentClose {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeInstrumentClose {
    /// Creates a new [`SubscribeInstrumentClose`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
        }
    }
}

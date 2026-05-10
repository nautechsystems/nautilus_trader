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

use std::num::NonZeroUsize;

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, DataType, option_chain::StrikeRange},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Venue},
};

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
pub struct SubscribeCustomData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeCustomData {
    /// Creates a new [`SubscribeCustomData`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        data_type: DataType,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            client_id,
            venue,
            data_type,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrument {
    /// Creates a new [`SubscribeInstrument`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstruments {
    /// Creates a new [`SubscribeInstruments`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Venue,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookDeltas {
    /// Creates a new [`SubscribeBookDeltas`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        managed: bool,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
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
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookDepth10 {
    /// Creates a new [`SubscribeBookDepth10`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        managed: bool,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
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
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookSnapshots {
    /// Creates a new [`SubscribeBookSnapshots`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
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
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeQuotes {
    /// Creates a new [`SubscribeQuotes`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeTrades {
    /// Creates a new [`SubscribeTrades`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBars {
    /// Creates a new [`SubscribeBars`] instance.
    pub fn new(
        bar_type: BarType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_type,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeMarkPrices {
    /// Creates a new [`SubscribeMarkPrices`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeIndexPrices {
    /// Creates a new [`SubscribeIndexPrices`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeFundingRates {
    /// Creates a new [`SubscribeFundingRates`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrumentStatus {
    /// Creates a new [`SubscribeInstrumentStatus`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeOptionGreeks {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeOptionGreeks {
    /// Creates a new [`SubscribeOptionGreeks`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
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
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrumentClose {
    /// Creates a new [`SubscribeInstrumentClose`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeOptionChain {
    pub series_id: OptionSeriesId,
    pub strike_range: StrikeRange,
    pub snapshot_interval_ms: Option<u64>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub params: Option<Params>,
}

impl SubscribeOptionChain {
    /// Creates a new [`SubscribeOptionChain`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        snapshot_interval_ms: Option<u64>,
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            series_id,
            strike_range,
            snapshot_interval_ms,
            command_id,
            ts_init,
            client_id,
            venue,
            params,
        }
    }
}

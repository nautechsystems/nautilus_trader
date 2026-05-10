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

use chrono::{DateTime, Utc};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientId, InstrumentId, Venue},
};
use ustr::Ustr;

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
pub struct RequestCustomData {
    pub client_id: ClientId,
    pub data_type: DataType,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestCustomData {
    /// Creates a new [`RequestCustomData`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        client_id: ClientId,
        data_type: DataType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_id,
            data_type,
            start,
            end,
            limit,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestInstrument {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestInstrument {
    /// Creates a new [`RequestInstrument`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            start,
            end,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestInstruments {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestInstruments {
    /// Creates a new [`RequestInstruments`] instance.
    pub fn new(
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            start,
            end,
            client_id,
            venue,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestBookSnapshot {
    pub instrument_id: InstrumentId,
    pub depth: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBookSnapshot {
    /// Creates a new [`RequestBookSnapshot`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            depth,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestQuotes {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestQuotes {
    /// Creates a new [`RequestQuotes`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestTrades {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestTrades {
    /// Creates a new [`RequestTrades`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestFundingRates {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestFundingRates {
    /// Creates a new [`RequestFundingRates`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestForwardPrices {
    pub venue: Venue,
    pub underlying: Ustr,
    pub instrument_id: Option<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestForwardPrices {
    /// Creates a new [`RequestForwardPrices`] instance.
    pub fn new(
        venue: Venue,
        underlying: Ustr,
        instrument_id: Option<InstrumentId>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            venue,
            underlying,
            instrument_id,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestBookDepth {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub depth: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBookDepth {
    /// Creates a new [`RequestBookDepth`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            start,
            end,
            limit,
            depth,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestBars {
    pub bar_type: BarType,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBars {
    /// Creates a new [`RequestBars`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            bar_type,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

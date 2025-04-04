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

use std::{any::Any, collections::HashMap, num::NonZeroUsize, sync::Arc};

use chrono::{DateTime, Utc};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
};

#[derive(Clone, Debug)]
pub enum DataCommand {
    Request(DataRequest),
    Subscribe(SubscribeCommand),
    Unsubscribe(UnsubscribeCommand),
}

impl DataCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Debug)]
pub enum SubscribeCommand {
    Data(SubscribeData),
    Instruments(SubscribeInstruments),
    Instrument(SubscribeInstrument),
    BookDeltas(SubscribeBookDeltas),
    BookDepth10(SubscribeBookDepth10),
    BookSnapshots(SubscribeBookSnapshots),
    Quotes(SubscribeQuotes),
    Trades(SubscribeTrades),
    Bars(SubscribeBars),
    MarkPrices(SubscribeMarkPrices),
    IndexPrices(SubscribeIndexPrices),
    InstrumentStatus(SubscribeInstrumentStatus),
    InstrumentClose(SubscribeInstrumentClose),
}

impl SubscribeCommand {
    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth10(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth10(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum UnsubscribeCommand {
    Data(UnsubscribeData),
    Instruments(UnsubscribeInstruments),
    Instrument(UnsubscribeInstrument),
    BookDeltas(UnsubscribeBookDeltas),
    BookDepth10(UnsubscribeBookDepth10),
    BookSnapshots(UnsubscribeBookSnapshots),
    Quotes(UnsubscribeQuotes),
    Trades(UnsubscribeTrades),
    Bars(UnsubscribeBars),
    MarkPrices(UnsubscribeMarkPrices),
    IndexPrices(UnsubscribeIndexPrices),
    InstrumentStatus(UnsubscribeInstrumentStatus),
    InstrumentClose(UnsubscribeInstrumentClose),
}

impl UnsubscribeCommand {
    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth10(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth10(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
        }
    }
}

fn check_client_id_or_venue(client_id: &Option<ClientId>, venue: &Option<Venue>) {
    assert!(
        client_id.is_some() || venue.is_some(),
        "Both `client_id` and `venue` were None"
    );
}

#[derive(Clone, Debug)]
pub struct SubscribeData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeData {
    pub fn new(
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        data_type: DataType,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct SubscribeInstruments {
    pub client_id: Option<ClientId>,
    pub venue: Venue,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeInstruments {
    pub fn new(
        client_id: Option<ClientId>,
        venue: Venue,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct SubscribeInstrument {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeInstrument {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct SubscribeBookDeltas {
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub managed: bool,
    pub params: Option<HashMap<String, String>>,
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
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeBookDepth10 {
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
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeBookSnapshots {
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
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeQuotes {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeTrades {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
    pub await_partial: bool,
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeBars {
    pub fn new(
        bar_type: BarType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        await_partial: bool,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_type,
            client_id,
            venue,
            command_id,
            ts_init,
            await_partial,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeMarkPrices {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeIndexPrices {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeInstrumentStatus {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
    pub params: Option<HashMap<String, String>>,
}

impl SubscribeInstrumentClose {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeData {
    pub fn new(
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        data_type: DataType,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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

// Unsubscribe commands
#[derive(Clone, Debug)]
pub struct UnsubscribeInstruments {
    pub client_id: Option<ClientId>,
    pub venue: Venue,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeInstruments {
    pub fn new(
        client_id: Option<ClientId>,
        venue: Venue,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeInstrument {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeInstrument {
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeBookDeltas {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeBookDeltas {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeBookDepth10 {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeBookDepth10 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeBookSnapshots {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeBookSnapshots {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeQuotes {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeQuotes {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeTrades {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeTrades {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeBars {
    pub bar_type: BarType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeBars {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeMarkPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeMarkPrices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeIndexPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeIndexPrices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeInstrumentStatus {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeInstrumentStatus {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct UnsubscribeInstrumentClose {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl UnsubscribeInstrumentClose {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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

// Request data structures
#[derive(Clone, Debug)]
pub struct RequestInstrument {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestInstrument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
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
pub struct RequestInstruments {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestInstruments {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
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
pub struct RequestOrderBookSnapshot {
    pub instrument_id: InstrumentId,
    pub limit: usize,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestOrderBookSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        limit: usize,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            limit,
            client_id,
            venue,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestQuoteTicks {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: usize,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestQuoteTicks {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: usize,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            venue,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestTradeTicks {
    pub instrument_id: InstrumentId,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: usize,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestTradeTicks {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: usize,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            venue,
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
    pub limit: usize,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl RequestBars {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: usize,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_type,
            start,
            end,
            limit,
            client_id,
            venue,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DataRequest {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

pub type Payload = Arc<dyn Any + Send + Sync>;

#[derive(Clone, Debug)]
pub struct DataResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub data: Payload,
    pub ts_init: UnixNanos,
    pub params: Option<HashMap<String, String>>,
}

impl DataResponse {
    pub fn new<T: Any + Send + Sync>(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        data: T,
        ts_init: UnixNanos,
        params: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data_type,
            data: Arc::new(data),
            ts_init,
            params,
        }
    }
}

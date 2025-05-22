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

use std::{any::Any, sync::Arc};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::BarType,
    identifiers::{ClientId, Venue},
};

pub mod request_bars;
pub mod request_book_snapshot;
pub mod request_data;
pub mod request_instrument;
pub mod request_instruments;
pub mod request_quotes;
pub mod request_trades;
pub mod response_bars;
pub mod response_book_snapshot;
pub mod response_data;
pub mod response_instrument;
pub mod response_instruments;
pub mod response_quotes;
pub mod response_trades;
pub mod subscribe_bars;
pub mod subscribe_book_deltas;
pub mod subscribe_book_depth10;
pub mod subscribe_book_snapshots;
pub mod subscribe_close;
pub mod subscribe_data;
pub mod subscribe_index_prices;
pub mod subscribe_instrument;
pub mod subscribe_instruments;
pub mod subscribe_mark_prices;
pub mod subscribe_quotes;
pub mod subscribe_status;
pub mod subscribe_trades;
pub mod unsubscribe_bars;
pub mod unsubscribe_book_deltas;
pub mod unsubscribe_book_depth10;
pub mod unsubscribe_book_snapshots;
pub mod unsubscribe_close;
pub mod unsubscribe_data;
pub mod unsubscribe_index_prices;
pub mod unsubscribe_instrument;
pub mod unsubscribe_instruments;
pub mod unsubscribe_mark_prices;
pub mod unsubscribe_quotes;
pub mod unsubscribe_status;
pub mod unsubscribe_trades;

// Re-exports
pub use request_bars::RequestBars;
pub use request_book_snapshot::RequestBookSnapshot;
pub use request_data::RequestData;
pub use request_instrument::RequestInstrument;
pub use request_instruments::RequestInstruments;
pub use request_quotes::RequestQuotes;
pub use request_trades::RequestTrades;
pub use response_bars::BarsResponse;
pub use response_book_snapshot::BookResponse;
pub use response_data::CustomDataResponse;
pub use response_instrument::InstrumentResponse;
pub use response_instruments::InstrumentsResponse;
pub use response_quotes::QuotesResponse;
pub use response_trades::TradesResponse;
pub use subscribe_bars::SubscribeBars;
pub use subscribe_book_deltas::SubscribeBookDeltas;
pub use subscribe_book_depth10::SubscribeBookDepth10;
pub use subscribe_book_snapshots::SubscribeBookSnapshots;
pub use subscribe_close::SubscribeInstrumentClose;
pub use subscribe_data::SubscribeData;
pub use subscribe_index_prices::SubscribeIndexPrices;
pub use subscribe_instrument::SubscribeInstrument;
pub use subscribe_instruments::SubscribeInstruments;
pub use subscribe_mark_prices::SubscribeMarkPrices;
pub use subscribe_quotes::SubscribeQuotes;
pub use subscribe_status::SubscribeInstrumentStatus;
pub use subscribe_trades::SubscribeTrades;
pub use unsubscribe_bars::UnsubscribeBars;
pub use unsubscribe_book_deltas::UnsubscribeBookDeltas;
pub use unsubscribe_book_depth10::UnsubscribeBookDepth10;
pub use unsubscribe_book_snapshots::UnsubscribeBookSnapshots;
pub use unsubscribe_close::UnsubscribeInstrumentClose;
pub use unsubscribe_data::UnsubscribeData;
pub use unsubscribe_index_prices::UnsubscribeIndexPrices;
pub use unsubscribe_instrument::UnsubscribeInstrument;
pub use unsubscribe_instruments::UnsubscribeInstruments;
pub use unsubscribe_mark_prices::UnsubscribeMarkPrices;
pub use unsubscribe_quotes::UnsubscribeQuotes;
pub use unsubscribe_status::UnsubscribeInstrumentStatus;
pub use unsubscribe_trades::UnsubscribeTrades;

#[derive(Clone, Debug, PartialEq)]
pub enum DataCommand {
    Request(RequestCommand),
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

impl PartialEq for SubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl SubscribeCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Data(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::Instrument(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth10(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
        }
    }

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

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth10(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::InstrumentStatus(cmd) => cmd.ts_init,
            Self::InstrumentClose(cmd) => cmd.ts_init,
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

impl PartialEq for UnsubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl UnsubscribeCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Data(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::Instrument(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth10(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
        }
    }

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

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth10(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::InstrumentStatus(cmd) => cmd.ts_init,
            Self::InstrumentClose(cmd) => cmd.ts_init,
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
pub enum RequestCommand {
    Data(RequestData),
    Instrument(RequestInstrument),
    Instruments(RequestInstruments),
    BookSnapshot(RequestBookSnapshot),
    Quotes(RequestQuotes),
    Trades(RequestTrades),
    Bars(RequestBars),
}

impl PartialEq for RequestCommand {
    fn eq(&self, other: &Self) -> bool {
        self.request_id() == other.request_id()
    }
}

impl RequestCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn request_id(&self) -> &UUID4 {
        match self {
            Self::Data(cmd) => &cmd.request_id,
            Self::Instruments(cmd) => &cmd.request_id,
            Self::Instrument(cmd) => &cmd.request_id,
            Self::BookSnapshot(cmd) => &cmd.request_id,
            Self::Quotes(cmd) => &cmd.request_id,
            Self::Trades(cmd) => &cmd.request_id,
            Self::Bars(cmd) => &cmd.request_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => Some(&cmd.client_id),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshot(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(_) => None,
            Self::Instruments(cmd) => cmd.venue.as_ref(),
            Self::Instrument(cmd) => Some(&cmd.instrument_id.venue),
            Self::BookSnapshot(cmd) => Some(&cmd.instrument_id.venue),
            Self::Quotes(cmd) => Some(&cmd.instrument_id.venue),
            Self::Trades(cmd) => Some(&cmd.instrument_id.venue),
            // TODO: Extract the below somewhere
            Self::Bars(cmd) => match &cmd.bar_type {
                BarType::Standard { instrument_id, .. } => Some(&instrument_id.venue),
                BarType::Composite { instrument_id, .. } => Some(&instrument_id.venue),
            },
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::BookSnapshot(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DataResponse {
    Data(CustomDataResponse),
    Instrument(Box<InstrumentResponse>),
    Instruments(InstrumentsResponse),
    Book(BookResponse),
    Quotes(QuotesResponse),
    Trades(TradesResponse),
    Bars(BarsResponse),
}

impl DataResponse {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn correlation_id(&self) -> &UUID4 {
        match self {
            Self::Data(resp) => &resp.correlation_id,
            Self::Instrument(resp) => &resp.correlation_id,
            Self::Instruments(resp) => &resp.correlation_id,
            Self::Book(resp) => &resp.correlation_id,
            Self::Quotes(resp) => &resp.correlation_id,
            Self::Trades(resp) => &resp.correlation_id,
            Self::Bars(resp) => &resp.correlation_id,
        }
    }
}

pub type Payload = Arc<dyn Any + Send + Sync>;

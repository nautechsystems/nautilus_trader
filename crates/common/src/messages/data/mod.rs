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

//! Data specific messages such as subscriptions and requests.

use std::{any::Any, sync::Arc};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::BarType,
    identifiers::{ClientId, Venue},
};

pub mod request;
pub mod response;
pub mod subscribe;
pub mod unsubscribe;

// Re-exports
pub use request::{
    RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestInstrument,
    RequestInstruments, RequestQuotes, RequestTrades,
};
pub use response::{
    BarsResponse, BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
    QuotesResponse, TradesResponse,
};
pub use subscribe::{
    SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots,
    SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
    SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
    SubscribeQuotes, SubscribeTrades,
};
pub use unsubscribe::{
    UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
    UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
    UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
    UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
};

#[cfg(feature = "defi")]
use crate::messages::defi::{DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand};

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum DataCommand {
    Request(RequestCommand),
    Subscribe(SubscribeCommand),
    Unsubscribe(UnsubscribeCommand),
    #[cfg(feature = "defi")]
    DefiRequest(DefiRequestCommand),
    #[cfg(feature = "defi")]
    DefiSubscribe(DefiSubscribeCommand),
    #[cfg(feature = "defi")]
    DefiUnsubscribe(DefiUnsubscribeCommand),
}

impl DataCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Debug)]
pub enum SubscribeCommand {
    Data(SubscribeCustomData),
    Instrument(SubscribeInstrument),
    Instruments(SubscribeInstruments),
    BookDeltas(SubscribeBookDeltas),
    BookDepth10(SubscribeBookDepth10),
    BookSnapshots(SubscribeBookSnapshots),
    Quotes(SubscribeQuotes),
    Trades(SubscribeTrades),
    Bars(SubscribeBars),
    MarkPrices(SubscribeMarkPrices),
    IndexPrices(SubscribeIndexPrices),
    FundingRates(SubscribeFundingRates),
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
            Self::Instrument(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth10(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::FundingRates(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth10(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::FundingRates(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth10(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::FundingRates(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth10(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::FundingRates(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::InstrumentStatus(cmd) => cmd.ts_init,
            Self::InstrumentClose(cmd) => cmd.ts_init,
        }
    }
}

#[derive(Clone, Debug)]
pub enum UnsubscribeCommand {
    Data(UnsubscribeCustomData),
    Instrument(UnsubscribeInstrument),
    Instruments(UnsubscribeInstruments),
    BookDeltas(UnsubscribeBookDeltas),
    BookDepth10(UnsubscribeBookDepth10),
    BookSnapshots(UnsubscribeBookSnapshots),
    Quotes(UnsubscribeQuotes),
    Trades(UnsubscribeTrades),
    Bars(UnsubscribeBars),
    MarkPrices(UnsubscribeMarkPrices),
    IndexPrices(UnsubscribeIndexPrices),
    FundingRates(UnsubscribeFundingRates),
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
            Self::Instrument(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth10(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::FundingRates(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth10(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::FundingRates(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth10(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::FundingRates(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth10(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::FundingRates(cmd) => cmd.ts_init,
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
    Data(RequestCustomData),
    Instrument(RequestInstrument),
    Instruments(RequestInstruments),
    BookSnapshot(RequestBookSnapshot),
    BookDepth(RequestBookDepth),
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
            Self::Instrument(cmd) => &cmd.request_id,
            Self::Instruments(cmd) => &cmd.request_id,
            Self::BookSnapshot(cmd) => &cmd.request_id,
            Self::BookDepth(cmd) => &cmd.request_id,
            Self::Quotes(cmd) => &cmd.request_id,
            Self::Trades(cmd) => &cmd.request_id,
            Self::Bars(cmd) => &cmd.request_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => Some(&cmd.client_id),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshot(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(_) => None,
            Self::Instrument(cmd) => Some(&cmd.instrument_id.venue),
            Self::Instruments(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshot(cmd) => Some(&cmd.instrument_id.venue),
            Self::BookDepth(cmd) => Some(&cmd.instrument_id.venue),
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
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookSnapshot(cmd) => cmd.ts_init,
            Self::BookDepth(cmd) => cmd.ts_init,
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

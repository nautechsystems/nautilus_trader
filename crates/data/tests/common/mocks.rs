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

//! Mock data client implementations.
//!
//! Provides a `MockDataClient` for testing scenarios with an in-memory cache.

// Under development
#![allow(dead_code)]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::data::{
        DataCommand, RequestBars, RequestBookSnapshot, RequestCommand, RequestCustomData,
        RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars,
        SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand,
        SubscribeCustomData, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
        SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
        UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeCustomData,
        UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentClose,
        UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices,
        UnsubscribeQuotes, UnsubscribeTrades,
    },
};
use nautilus_data::client::DataClient;
use nautilus_model::identifiers::{ClientId, Venue};

/// A mock implementation of [`DataClient`] for testing, with optional generic recorder.
pub struct MockDataClient {
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub recorder: Option<Rc<RefCell<Vec<DataCommand>>>>,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
}

impl MockDataClient {
    /// Creates a new [`MockDataClient`] instance with the given cache, client ID, and venue.
    #[must_use]
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        client_id: ClientId,
        venue: Option<Venue>,
    ) -> Self {
        Self {
            clock,
            cache,
            client_id,
            venue,
            recorder: None,
        }
    }

    /// Creates a new [`MockDataClient`] that records all `DataCommands` into the given recorder.
    #[must_use]
    pub fn new_with_recorder(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        client_id: ClientId,
        venue: Option<Venue>,
        recorder: Option<Rc<RefCell<Vec<DataCommand>>>>,
    ) -> Self {
        Self {
            client_id,
            venue,
            recorder,
            clock,
            cache,
        }
    }
}

#[async_trait::async_trait]
impl DataClient for MockDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        self.venue
    }

    fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn connect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn disconnect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn is_disconnected(&self) -> bool {
        false
    }

    // -- SUBSCRIPTION HANDLERS -------------------------------------------------------------------

    fn subscribe(&mut self, cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Data(cmd.clone())));
        }
        Ok(())
    }

    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Instruments(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Instrument(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::BookDepth10(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Quotes(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Trades(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::Bars(cmd.clone())));
        }
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::MarkPrices(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::IndexPrices(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Subscribe(SubscribeCommand::InstrumentClose(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Data(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Instruments(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::BookDepth10(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Unsubscribe(UnsubscribeCommand::IndexPrices(
                    cmd.clone(),
                )));
        }
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut().push(DataCommand::Unsubscribe(
                UnsubscribeCommand::InstrumentStatus(cmd.clone()),
            ));
        }
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut().push(DataCommand::Unsubscribe(
                UnsubscribeCommand::InstrumentClose(cmd.clone()),
            ));
        }
        Ok(())
    }

    // -- REQUEST HANDLERS ------------------------------------------------------------------------

    fn request_data(&self, request: &RequestCustomData) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Data(request.clone())));
        }
        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Instruments(
                    request.clone(),
                )));
        }
        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Instrument(
                    request.clone(),
                )));
        }
        Ok(())
    }

    fn request_book_snapshot(&self, request: &RequestBookSnapshot) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::BookSnapshot(
                    request.clone(),
                )));
        }
        Ok(())
    }

    fn request_quotes(&self, request: &RequestQuotes) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Quotes(
                    request.clone(),
                )));
        }
        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Trades(
                    request.clone(),
                )));
        }
        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        if let Some(rec) = &self.recorder {
            rec.borrow_mut()
                .push(DataCommand::Request(RequestCommand::Bars(request.clone())));
        }
        Ok(())
    }
}

// SAFETY: Cannot be sent across thread boundaries
#[allow(unsafe_code)]
unsafe impl Send for MockDataClient {}
#[allow(unsafe_code)]
unsafe impl Sync for MockDataClient {}

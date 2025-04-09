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

//! Provides a `BacktestDataClient` implementation for backtesting.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    messages::data::{
        RequestBars, RequestBookSnapshot, RequestData, RequestInstrument, RequestInstruments,
        RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
        SubscribeBookSnapshots, SubscribeData, SubscribeIndexPrices, SubscribeInstrument,
        SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
        SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots, UnsubscribeData,
        UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentClose,
        UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices,
        UnsubscribeQuotes, UnsubscribeTrades,
    },
};
use nautilus_data::client::DataClient;
use nautilus_model::identifiers::{ClientId, Venue};

pub struct BacktestDataClient {
    pub client_id: ClientId,
    pub venue: Venue,
    cache: Rc<RefCell<Cache>>,
}

impl BacktestDataClient {
    pub fn new(client_id: ClientId, venue: Venue, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            client_id,
            venue,
            cache,
        }
    }
}

impl DataClient for BacktestDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }
    fn venue(&self) -> Option<Venue> {
        Some(self.venue)
    }

    fn start(&self) {}
    fn stop(&self) {}
    fn reset(&self) {}
    fn dispose(&self) {}
    fn is_connected(&self) -> bool {
        true
    }
    fn is_disconnected(&self) -> bool {
        false
    }

    // -- COMMAND HANDLERS ---------------------------------------------------------------------------

    fn subscribe(&mut self, _cmd: SubscribeData) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, _cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_book_depth10(&mut self, _cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, _cmd: SubscribeBookSnapshots) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_quotes(&mut self, _cmd: SubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_trades(&mut self, _cmd: SubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_bars(&mut self, _cmd: SubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, _cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_index_prices(&mut self, _cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        _cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_close(&mut self, _cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe(&mut self, _cmd: UnsubscribeData) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, _cmd: UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, _cmd: UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, _cmd: UnsubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, _cmd: UnsubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, _cmd: UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        _cmd: UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        _cmd: UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    // -- DATA REQUEST HANDLERS ---------------------------------------------------------------------------

    fn request_data(&self, request: RequestData) -> anyhow::Result<()> {
        todo!()
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        todo!()
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        todo!()
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        todo!()
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        todo!()
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        todo!()
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        todo!()
    }
}

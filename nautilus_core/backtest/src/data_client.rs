// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    messages::data::{DataRequest, Payload},
    msgbus::MessageBus,
};
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
        DataType,
    },
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::any::InstrumentAny,
};

pub struct BacktestDataClient {
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    pub client_id: ClientId,
    pub venue: Venue,
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

    /// Parse command and call specific function
    fn subscribe(&mut self, _data_type: &DataType) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instruments(&mut self, _venue: Option<&Venue>) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument(&mut self, _instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_order_book_deltas(
        &mut self,
        _instrument_id: &InstrumentId,
        _book_type: BookType,
        _depth: Option<usize>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_order_book_snapshots(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_quote_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_trade_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_bars(&mut self, bar_type: &BarType) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_status(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_close(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe(&mut self, data_type: &DataType) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, venue: Option<&Venue>) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_order_book_deltas(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_order_book_snapshots(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_quote_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_trade_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, bar_type: &BarType) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_close(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()> {
        Ok(())
    }

    // -- DATA REQUEST HANDLERS ---------------------------------------------------------------------------

    fn request_data(&self, request: DataRequest) {
        todo!()
    }

    fn request_instruments(
        &self,
        correlation_id: UUID4,
        venue: Venue,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Vec<InstrumentAny> {
        todo!()
    }

    fn request_instrument(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> InstrumentAny {
        todo!()
    }

    // TODO: figure out where to call this and it's return type
    fn request_order_book_snapshot(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        depth: Option<usize>,
    ) -> Payload {
        todo!()
    }

    fn request_quote_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<QuoteTick> {
        todo!()
    }

    fn request_trade_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<TradeTick> {
        todo!()
    }

    fn request_bars(
        &self,
        correlation_id: UUID4,
        bar_type: BarType,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<Bar> {
        todo!()
    }
}

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

//! Base data client functionality.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use indexmap::IndexMap;
use nautilus_common::{
    clock::Clock,
    messages::data::{Action, DataRequest, DataResponse, Payload, SubscriptionCommand},
};
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
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

pub trait DataClient {
    fn client_id(&self) -> ClientId;
    fn venue(&self) -> Option<Venue>;
    fn start(&self);
    fn stop(&self);
    fn reset(&self);
    fn dispose(&self);
    fn is_connected(&self) -> bool;
    fn is_disconnected(&self) -> bool;

    // TODO: Move to separate trait
    // A [`LiveDataClient`] must have two channels to send back data and data responses
    // fn get_response_data_channel(&self) -> tokio::sync::mpsc::UnboundedSender<DataResponse>;
    // fn get_subscriber_data_channel(&self) -> tokio::sync::mpsc::UnboundedSender<Data>;

    // -- COMMAND HANDLERS ---------------------------------------------------------------------------

    /// Parse command and call specific function
    fn subscribe(&mut self, data_type: &DataType) -> anyhow::Result<()>;
    fn subscribe_instruments(&mut self, venue: Option<&Venue>) -> anyhow::Result<()>;
    fn subscribe_instrument(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn subscribe_order_book_deltas(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> anyhow::Result<()>;
    fn subscribe_order_book_snapshots(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> anyhow::Result<()>;
    fn subscribe_quote_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn subscribe_trade_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn subscribe_bars(&mut self, bar_type: &BarType) -> anyhow::Result<()>;
    fn subscribe_instrument_status(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn subscribe_instrument_close(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe(&mut self, data_type: &DataType) -> anyhow::Result<()>;
    fn unsubscribe_instruments(&mut self, venue: Option<&Venue>) -> anyhow::Result<()>;
    fn unsubscribe_instrument(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_order_book_deltas(&mut self, instrument_id: &InstrumentId)
        -> anyhow::Result<()>;
    fn unsubscribe_order_book_snapshots(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()>;
    fn unsubscribe_quote_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_trade_ticks(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_bars(&mut self, bar_type: &BarType) -> anyhow::Result<()>;
    fn unsubscribe_instrument_status(&mut self, instrument_id: &InstrumentId)
        -> anyhow::Result<()>;
    fn unsubscribe_instrument_close(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<()>;

    // -- DATA REQUEST HANDLERS ---------------------------------------------------------------------------

    fn request_data(&self, request: DataRequest);
    fn request_instruments(
        &self,
        correlation_id: UUID4,
        venue: Venue,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Vec<InstrumentAny>;
    fn request_instrument(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> InstrumentAny;
    // TODO: figure out where to call this and it's return type
    fn request_order_book_snapshot(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        depth: Option<usize>,
    ) -> Payload;
    fn request_quote_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<QuoteTick>;
    fn request_trade_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<TradeTick>;
    fn request_bars(
        &self,
        correlation_id: UUID4,
        bar_type: BarType,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    ) -> Vec<Bar>;
}

pub struct DataClientAdapter {
    client: Box<dyn DataClient>,
    clock: Box<dyn Clock>,
    pub client_id: ClientId,
    pub venue: Venue,
    pub handles_order_book_deltas: bool,
    pub handles_order_book_snapshots: bool,
    pub subscriptions_generic: HashSet<DataType>,
    pub subscriptions_order_book_delta: HashSet<InstrumentId>,
    pub subscriptions_order_book_snapshot: HashSet<InstrumentId>,
    pub subscriptions_quote_tick: HashSet<InstrumentId>,
    pub subscriptions_trade_tick: HashSet<InstrumentId>,
    pub subscriptions_bar: HashSet<BarType>,
    pub subscriptions_instrument_status: HashSet<InstrumentId>,
    pub subscriptions_instrument_close: HashSet<InstrumentId>,
    pub subscriptions_instrument: HashSet<InstrumentId>,
    pub subscriptions_instrument_venue: HashSet<Venue>,
}

impl Deref for DataClientAdapter {
    type Target = Box<dyn DataClient>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for DataClientAdapter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl DataClientAdapter {
    #[must_use]
    pub fn new(
        client_id: ClientId,
        venue: Venue,
        handles_order_book_deltas: bool,
        handles_order_book_snapshots: bool,
        client: Box<dyn DataClient>,
        clock: Box<dyn Clock>,
    ) -> Self {
        Self {
            client,
            clock,
            client_id,
            venue,
            handles_order_book_deltas,
            handles_order_book_snapshots,
            subscriptions_generic: HashSet::new(),
            subscriptions_order_book_delta: HashSet::new(),
            subscriptions_order_book_snapshot: HashSet::new(),
            subscriptions_quote_tick: HashSet::new(),
            subscriptions_trade_tick: HashSet::new(),
            subscriptions_bar: HashSet::new(),
            subscriptions_instrument_status: HashSet::new(),
            subscriptions_instrument_close: HashSet::new(),
            subscriptions_instrument: HashSet::new(),
            subscriptions_instrument_venue: HashSet::new(),
        }
    }

    /// TODO: Decide whether to use mut references for subscription commands
    pub fn through_execute(&self, command: SubscriptionCommand) {}

    pub fn execute(&mut self, command: SubscriptionCommand) {
        match command.action {
            Action::Subscribe => self.execute_subscribe_command(command),
            Action::Unsubscribe => self.execute_unsubscribe_command(command),
        }
    }

    #[inline]
    fn execute_subscribe_command(&mut self, command: SubscriptionCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => Self::subscribe_instrument(self, command),
            stringify!(OrderBookDelta) => Self::subscribe_order_book_deltas(self, command),
            stringify!(OrderBookDeltas) | stringify!(OrderBookDepth10) => {
                Self::subscribe_snapshots(self, command);
            }
            stringify!(QuoteTick) => Self::subscribe_quote_ticks(self, command),
            stringify!(TradeTick) => Self::subscribe_trade_ticks(self, command),
            stringify!(Bar) => Self::subscribe_bars(self, command),
            _ => Self::subscribe(self, command),
        }
    }

    #[inline]
    fn execute_unsubscribe_command(&mut self, command: SubscriptionCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => Self::unsubscribe_instrument(self, command),
            stringify!(OrderBookDelta) => Self::unsubscribe_order_book_deltas(self, command),
            stringify!(OrderBookDeltas) | stringify!(OrderBookDepth10) => {
                Self::unsubscribe_snapshots(self, command);
            }
            stringify!(QuoteTick) => Self::unsubscribe_quote_ticks(self, command),
            stringify!(TradeTick) => Self::unsubscribe_trade_ticks(self, command),
            stringify!(Bar) => Self::unsubscribe_bars(self, command),
            _ => Self::unsubscribe(self, command),
        }
    }

    fn subscribe_instrument(&mut self, command: SubscriptionCommand) {
        let instrument_id = command.data_type.instrument_id();
        let venue = command.data_type.venue();

        if let Some(instrument_id) = instrument_id {
            // TODO: consider using insert_with once it stabilizes
            // https://github.com/rust-lang/rust/issues/60896
            if !self.subscriptions_instrument.contains(&instrument_id) {
                self.client
                    .subscribe_instrument(&instrument_id)
                    .expect("Error on subscribe");
            }

            self.subscriptions_instrument.insert(instrument_id);
        }

        if let Some(venue) = venue {
            if !self.subscriptions_instrument_venue.contains(&venue) {
                self.client
                    .subscribe_instruments(Some(&venue))
                    .expect("Error on subscribe");
            }

            self.subscriptions_instrument_venue.insert(venue);
        }
    }

    fn unsubscribe_instrument(&mut self, command: SubscriptionCommand) {
        let instrument_id = command.data_type.instrument_id();
        let venue = command.data_type.venue();

        if let Some(instrument_id) = instrument_id {
            if self.subscriptions_instrument.contains(&instrument_id) {
                self.client
                    .unsubscribe_instrument(&instrument_id)
                    .expect("Error on subscribe");
            }

            self.subscriptions_instrument.remove(&instrument_id);
        }

        if let Some(venue) = venue {
            if self.subscriptions_instrument_venue.contains(&venue) {
                self.client
                    .unsubscribe_instruments(Some(&venue))
                    .expect("Error on subscribe");
            }

            self.subscriptions_instrument_venue.remove(&venue);
        }
    }

    fn subscribe_order_book_deltas(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        let book_type = command.data_type.book_type();
        let depth = command.data_type.depth();

        if !self.subscriptions_order_book_delta.contains(&instrument_id) {
            self.client
                .subscribe_order_book_deltas(&instrument_id, book_type, depth)
                .expect("Error on subscribe");
        }

        self.subscriptions_order_book_delta.insert(instrument_id);
    }

    fn unsubscribe_order_book_deltas(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if self.subscriptions_order_book_delta.contains(&instrument_id) {
            self.client
                .unsubscribe_order_book_deltas(&instrument_id)
                .expect("Error on subscribe");
        }

        self.subscriptions_order_book_delta.remove(&instrument_id);
    }

    fn subscribe_snapshots(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        let book_type = command.data_type.book_type();
        let depth = command.data_type.depth();

        if !self
            .subscriptions_order_book_snapshot
            .contains(&instrument_id)
        {
            self.client
                .subscribe_order_book_snapshots(&instrument_id, book_type, depth)
                .expect("Error on subscribe");
        }

        self.subscriptions_order_book_snapshot.insert(instrument_id);
    }

    fn unsubscribe_snapshots(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if self
            .subscriptions_order_book_snapshot
            .contains(&instrument_id)
        {
            self.client
                .unsubscribe_order_book_snapshots(&instrument_id)
                .expect("Error on subscribe");
        }

        self.subscriptions_order_book_snapshot
            .remove(&instrument_id);
    }

    fn subscribe_quote_ticks(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if !self.subscriptions_quote_tick.contains(&instrument_id) {
            self.client
                .subscribe_quote_ticks(&instrument_id)
                .expect("Error on subscribe");
        }
        self.subscriptions_quote_tick.insert(instrument_id);
    }

    fn unsubscribe_quote_ticks(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if self.subscriptions_quote_tick.contains(&instrument_id) {
            self.client
                .unsubscribe_quote_ticks(&instrument_id)
                .expect("Error on subscribe");
        }
        self.subscriptions_quote_tick.remove(&instrument_id);
    }

    fn unsubscribe_trade_ticks(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if self.subscriptions_trade_tick.contains(&instrument_id) {
            self.client
                .unsubscribe_trade_ticks(&instrument_id)
                .expect("Error on subscribe");
        }
        self.subscriptions_trade_tick.remove(&instrument_id);
    }

    fn subscribe_trade_ticks(&mut self, command: SubscriptionCommand) {
        let instrument_id = command
            .data_type
            .instrument_id()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        if !self.subscriptions_trade_tick.contains(&instrument_id) {
            self.client
                .subscribe_trade_ticks(&instrument_id)
                .expect("Error on subscribe");
        }
        self.subscriptions_trade_tick.insert(instrument_id);
    }

    fn subscribe_bars(&mut self, command: SubscriptionCommand) {
        let bar_type = command.data_type.bar_type();

        if !self.subscriptions_bar.contains(&bar_type) {
            self.client
                .subscribe_bars(&bar_type)
                .expect("Error on subscribe");
        }
        self.subscriptions_bar.insert(bar_type);
    }

    fn unsubscribe_bars(&mut self, command: SubscriptionCommand) {
        let bar_type = command.data_type.bar_type();

        if self.subscriptions_bar.contains(&bar_type) {
            self.client
                .subscribe_bars(&bar_type)
                .expect("Error on subscribe");
        }
        self.subscriptions_bar.remove(&bar_type);
    }

    pub fn subscribe(&mut self, command: SubscriptionCommand) {
        let data_type = command.data_type;
        if !self.subscriptions_generic.contains(&data_type) {
            self.client
                .subscribe(&data_type)
                .expect("Error on subscribe");
        }
        self.subscriptions_generic.insert(data_type);
    }

    pub fn unsubscribe(&mut self, command: SubscriptionCommand) {
        let data_type = command.data_type;
        if self.subscriptions_generic.contains(&data_type) {
            self.client
                .unsubscribe(&data_type)
                .expect("Error on unsubscribe");
        }
        self.subscriptions_generic.remove(&data_type);
    }

    // -- DATA REQUEST HANDLERS IMPLEMENTATION ---------------------------------------------------------------------------

    /// TODO: New clients implement a request pattern
    /// that does not return a `DataResponse` directly
    /// it internally uses a queue/channel to send
    /// back response
    pub fn through_request(&self, req: DataRequest) {
        self.client.request_data(req);
    }

    #[must_use]
    pub fn request(&self, req: DataRequest) -> DataResponse {
        let instrument_id = req.data_type.instrument_id();
        let venue = req.data_type.venue();
        let start = req.data_type.start();
        let end = req.data_type.end();
        let limit = req.data_type.limit();

        match req.data_type.type_name() {
            stringify!(InstrumentAny) => match (instrument_id, venue) {
                (None, Some(venue)) => {
                    let instruments =
                        self.client
                            .request_instruments(req.correlation_id, venue, start, end);
                    self.handle_instruments(venue, instruments, req.correlation_id)
                }
                (Some(instrument_id), None) => {
                    let instrument = self.client.request_instrument(
                        req.correlation_id,
                        instrument_id,
                        start,
                        end,
                    );
                    self.handle_instrument(instrument, req.correlation_id)
                }
                _ => {
                    todo!()
                }
            },
            stringify!(QuoteTick) => {
                let instrument_id =
                    instrument_id.expect("Error on request: no 'instrument_id' found in metadata");
                let quotes = self.client.request_quote_ticks(
                    req.correlation_id,
                    instrument_id,
                    start,
                    end,
                    limit,
                );
                self.handle_quote_ticks(&instrument_id, quotes, req.correlation_id)
            }
            stringify!(TradeTick) => {
                let instrument_id =
                    instrument_id.expect("Error on request: no 'instrument_id' found in metadata");
                let trades = self.client.request_trade_ticks(
                    req.correlation_id,
                    instrument_id,
                    start,
                    end,
                    limit,
                );
                self.handle_trade_ticks(&instrument_id, trades, req.correlation_id)
            }
            stringify!(Bar) => {
                let bar_type = req.data_type.bar_type();
                let bars =
                    self.client
                        .request_bars(req.correlation_id, bar_type, start, end, limit);
                self.handle_bars(&bar_type, bars, req.correlation_id)
            }
            _ => {
                todo!()
            }
        }
    }

    #[must_use]
    pub fn handle_instrument(
        &self,
        instrument: InstrumentAny,
        correlation_id: UUID4,
    ) -> DataResponse {
        let instrument_id = instrument.id();
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instrument);

        DataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        )
    }

    #[must_use]
    pub fn handle_instruments(
        &self,
        venue: Venue,
        instruments: Vec<InstrumentAny>,
        correlation_id: UUID4,
    ) -> DataResponse {
        let metadata = IndexMap::from([("venue".to_string(), venue.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instruments);

        DataResponse::new(
            correlation_id,
            self.client_id,
            venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        )
    }

    #[must_use]
    pub fn handle_quote_ticks(
        &self,
        instrument_id: &InstrumentId,
        quotes: Vec<QuoteTick>,
        correlation_id: UUID4,
    ) -> DataResponse {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let data = Arc::new(quotes);

        DataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        )
    }

    #[must_use]
    pub fn handle_trade_ticks(
        &self,
        instrument_id: &InstrumentId,
        trades: Vec<TradeTick>,
        correlation_id: UUID4,
    ) -> DataResponse {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
        let data = Arc::new(trades);

        DataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        )
    }

    #[must_use]
    pub fn handle_bars(
        &self,
        bar_type: &BarType,
        bars: Vec<Bar>,
        correlation_id: UUID4,
    ) -> DataResponse {
        let metadata = IndexMap::from([("bar_type".to_string(), bar_type.to_string())]);
        let data_type = DataType::new(stringify!(Bar), Some(metadata));
        let data = Arc::new(bars);

        DataResponse::new(
            correlation_id,
            self.client_id,
            bar_type.instrument_id().venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        )
    }
}

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

//! Base data client functionality.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;
use nautilus_common::{
    clock::Clock,
    messages::data::{
        DataRequest, DataResponse, Payload, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, SubscribeData,
        SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
        SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
        UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeData, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, DataType, QuoteTick, TradeTick},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
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

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    fn subscribe(&mut self, cmd: SubscribeData) -> anyhow::Result<()>;
    fn subscribe_instruments(&mut self, cmd: SubscribeInstruments) -> anyhow::Result<()>;
    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()>;
    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()>;
    fn subscribe_book_depth10(&mut self, cmd: SubscribeBookDepth10) -> anyhow::Result<()>;
    fn subscribe_book_snapshots(&mut self, cmd: SubscribeBookSnapshots) -> anyhow::Result<()>;
    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()>;
    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()>;
    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()>;
    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()>;
    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()>;
    fn subscribe_instrument_status(&mut self, cmd: SubscribeInstrumentStatus)
    -> anyhow::Result<()>;
    fn subscribe_instrument_close(&mut self, cmd: SubscribeInstrumentClose) -> anyhow::Result<()>;
    fn unsubscribe(&mut self, cmd: UnsubscribeData) -> anyhow::Result<()>;
    fn unsubscribe_instruments(&mut self, cmd: UnsubscribeInstruments) -> anyhow::Result<()>;
    fn unsubscribe_instrument(&mut self, cmd: UnsubscribeInstrument) -> anyhow::Result<()>;
    fn unsubscribe_book_deltas(&mut self, cmd: UnsubscribeBookDeltas) -> anyhow::Result<()>;
    fn unsubscribe_book_depth10(&mut self, cmd: UnsubscribeBookDepth10) -> anyhow::Result<()>;
    fn unsubscribe_book_snapshots(&mut self, cmd: UnsubscribeBookSnapshots) -> anyhow::Result<()>;
    fn unsubscribe_quotes(&mut self, cmd: UnsubscribeQuotes) -> anyhow::Result<()>;
    fn unsubscribe_trades(&mut self, cmd: UnsubscribeTrades) -> anyhow::Result<()>;
    fn unsubscribe_mark_prices(&mut self, cmd: UnsubscribeMarkPrices) -> anyhow::Result<()>;
    fn unsubscribe_index_prices(&mut self, cmd: UnsubscribeIndexPrices) -> anyhow::Result<()>;
    fn unsubscribe_bars(&mut self, cmd: UnsubscribeBars) -> anyhow::Result<()>;
    fn unsubscribe_instrument_status(
        &mut self,
        cmd: UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()>;
    fn unsubscribe_instrument_close(
        &mut self,
        cmd: UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()>;

    // -- DATA REQUEST HANDLERS -------------------------------------------------------------------

    fn request_data(&self, request: DataRequest);
    fn request_instruments(
        &self,
        correlation_id: UUID4,
        venue: Venue,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: &Option<HashMap<String, String>>,
    ) -> Vec<InstrumentAny>;
    fn request_instrument(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: &Option<HashMap<String, String>>,
    ) -> InstrumentAny;
    // TODO: figure out where to call this and it's return type
    fn request_order_book_snapshot(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        depth: Option<NonZeroUsize>,
        params: &Option<HashMap<String, String>>,
    ) -> Payload;
    fn request_quote_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<NonZeroUsize>,
        params: &Option<HashMap<String, String>>,
    ) -> Vec<QuoteTick>;
    fn request_trade_ticks(
        &self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<NonZeroUsize>,
        params: &Option<HashMap<String, String>>,
    ) -> Vec<TradeTick>;
    fn request_bars(
        &self,
        correlation_id: UUID4,
        bar_type: BarType,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<NonZeroUsize>,
        params: &Option<HashMap<String, String>>,
    ) -> Vec<Bar>;
}

pub struct DataClientAdapter {
    client: Box<dyn DataClient>,
    clock: Rc<RefCell<dyn Clock>>,
    pub client_id: ClientId,
    pub venue: Venue,
    pub handles_book_deltas: bool,
    pub handles_book_snapshots: bool,
    pub subscriptions_generic: HashSet<DataType>,
    pub subscriptions_book_deltas: HashSet<InstrumentId>,
    pub subscriptions_book_depth10: HashSet<InstrumentId>,
    pub subscriptions_book_snapshots: HashSet<InstrumentId>,
    pub subscriptions_quotes: HashSet<InstrumentId>,
    pub subscriptions_trades: HashSet<InstrumentId>,
    pub subscriptions_bars: HashSet<BarType>,
    pub subscriptions_instrument_status: HashSet<InstrumentId>,
    pub subscriptions_instrument_close: HashSet<InstrumentId>,
    pub subscriptions_instrument: HashSet<InstrumentId>,
    pub subscriptions_instrument_venue: HashSet<Venue>,
    pub subscriptions_mark_prices: HashSet<InstrumentId>,
    pub subscriptions_index_prices: HashSet<InstrumentId>,
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

impl Debug for DataClientAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataClientAdapter")
            .field("client_id", &self.client_id)
            .field("venue", &self.venue)
            .field("handles_book_deltas", &self.handles_book_deltas)
            .field("handles_book_snapshots", &self.handles_book_snapshots)
            .field("subscriptions_generic", &self.subscriptions_generic)
            .field("subscriptions_book_deltas", &self.subscriptions_book_deltas)
            .field(
                "subscriptions_book_depth10",
                &self.subscriptions_book_depth10,
            )
            .field(
                "subscriptions_book_snapshot",
                &self.subscriptions_book_snapshots,
            )
            .field("subscriptions_quotes", &self.subscriptions_quotes)
            .field("subscriptions_trades", &self.subscriptions_trades)
            .field("subscriptions_mark_prices", &self.subscriptions_mark_prices)
            .field(
                "subscriptions_index_prices",
                &self.subscriptions_index_prices,
            )
            .field("subscriptions_bars", &self.subscriptions_bars)
            .field(
                "subscriptions_instrument_status",
                &self.subscriptions_instrument_status,
            )
            .field(
                "subscriptions_instrument_close",
                &self.subscriptions_instrument_close,
            )
            .field("subscriptions_instrument", &self.subscriptions_instrument)
            .field(
                "subscriptions_instrument_venue",
                &self.subscriptions_instrument_venue,
            )
            .finish()
    }
}

impl DataClientAdapter {
    /// Creates a new [`DataClientAdapter`] instance.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        venue: Venue,
        handles_order_book_deltas: bool,
        handles_order_book_snapshots: bool,
        client: Box<dyn DataClient>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        Self {
            client,
            clock,
            client_id,
            venue,
            handles_book_deltas: handles_order_book_deltas,
            handles_book_snapshots: handles_order_book_snapshots,
            subscriptions_generic: HashSet::new(),
            subscriptions_book_deltas: HashSet::new(),
            subscriptions_book_depth10: HashSet::new(),
            subscriptions_book_snapshots: HashSet::new(),
            subscriptions_quotes: HashSet::new(),
            subscriptions_trades: HashSet::new(),
            subscriptions_mark_prices: HashSet::new(),
            subscriptions_index_prices: HashSet::new(),
            subscriptions_bars: HashSet::new(),
            subscriptions_instrument_status: HashSet::new(),
            subscriptions_instrument_close: HashSet::new(),
            subscriptions_instrument: HashSet::new(),
            subscriptions_instrument_venue: HashSet::new(),
        }
    }

    /// TODO: Decide whether to use mut references for subscription commands
    pub fn through_execute(&self, command: SubscribeCommand) {}

    // // TODO: Deprecated
    // pub fn execute(&mut self, command: SubscribeCommand) {
    //     match command.action {
    //         Action::Subscribe => self.execute_subscribe_command(command),
    //         Action::Unsubscribe => self.execute_unsubscribe_command(command),
    //     }
    // }

    #[inline]
    pub fn execute_subscribe_command(&mut self, cmd: SubscribeCommand) {
        let result = match cmd.clone() {
            SubscribeCommand::Data(cmd) => self.subscribe(cmd),
            SubscribeCommand::Instrument(cmd) => self.subscribe_instrument(cmd),
            SubscribeCommand::Instruments(cmd) => self.subscribe_instruments(cmd),
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(cmd),
            SubscribeCommand::BookDepth10(cmd) => self.subscribe_book_depth10(cmd),
            SubscribeCommand::BookSnapshots(cmd) => self.subscribe_book_snapshots(cmd),
            SubscribeCommand::Quotes(cmd) => self.subscribe_quotes(cmd),
            SubscribeCommand::Trades(cmd) => self.subscribe_trades(cmd),
            SubscribeCommand::MarkPrices(cmd) => self.subscribe_mark_prices(cmd),
            SubscribeCommand::IndexPrices(cmd) => self.subscribe_index_prices(cmd),
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(cmd),
            SubscribeCommand::InstrumentStatus(cmd) => todo!(),
            SubscribeCommand::InstrumentClose(cmd) => todo!(),
        };

        if let Err(e) = result {
            log::debug!("Error on subscribe: {cmd:?}");
        }
    }

    #[inline]
    pub fn execute_unsubscribe_command(&mut self, cmd: UnsubscribeCommand) {
        let result = match cmd.clone() {
            UnsubscribeCommand::Data(cmd) => self.unsubscribe(cmd),
            UnsubscribeCommand::Instrument(cmd) => self.unsubscribe_instrument(cmd),
            UnsubscribeCommand::Instruments(cmd) => self.unsubscribe_instruments(cmd),
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd),
            UnsubscribeCommand::BookDepth10(cmd) => self.unsubscribe_book_depths10(cmd),
            UnsubscribeCommand::BookSnapshots(cmd) => self.unsubscribe_book_snapshots(cmd),
            UnsubscribeCommand::Quotes(cmd) => self.unsubscribe_quotes(cmd),
            UnsubscribeCommand::Trades(cmd) => self.unsubscribe_trades(cmd),
            UnsubscribeCommand::Bars(cmd) => self.unsubscribe_bars(cmd),
            UnsubscribeCommand::MarkPrices(cmd) => self.unsubscribe_mark_prices(cmd),
            UnsubscribeCommand::IndexPrices(cmd) => self.unsubscribe_index_prices(cmd),
            UnsubscribeCommand::InstrumentStatus(cmd) => todo!(),
            UnsubscribeCommand::InstrumentClose(cmd) => todo!(),
        };

        if let Err(e) = result {
            log::debug!("Error on unsubscribe: {cmd:?}");
        }
    }

    fn subscribe_instruments(&mut self, cmd: SubscribeInstruments) -> anyhow::Result<()> {
        if !self.subscriptions_instrument_venue.contains(&cmd.venue) {
            self.subscriptions_instrument_venue.insert(cmd.venue);
            self.client.subscribe_instruments(cmd)?;
        }

        Ok(())
    }

    fn unsubscribe_instruments(&mut self, cmd: UnsubscribeInstruments) -> anyhow::Result<()> {
        if self.subscriptions_instrument_venue.contains(&cmd.venue) {
            self.subscriptions_instrument_venue.remove(&cmd.venue);
            self.client.unsubscribe_instruments(cmd)?;
        }

        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        if !self.subscriptions_instrument.contains(&cmd.instrument_id) {
            self.subscriptions_instrument.insert(cmd.instrument_id);
            self.client.subscribe_instrument(cmd)?;
        }

        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: UnsubscribeInstrument) -> anyhow::Result<()> {
        if self.subscriptions_instrument.contains(&cmd.instrument_id) {
            self.subscriptions_instrument.remove(&cmd.instrument_id);
            self.client.unsubscribe_instrument(cmd)?;
        }

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if !self.subscriptions_book_deltas.contains(&cmd.instrument_id) {
            self.subscriptions_book_deltas.insert(cmd.instrument_id);
            self.client.subscribe_book_deltas(cmd)?;
        }

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: UnsubscribeBookDeltas) -> anyhow::Result<()> {
        if self.subscriptions_book_deltas.contains(&cmd.instrument_id) {
            self.subscriptions_book_deltas.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_deltas(cmd)?;
        }

        Ok(())
    }

    fn subscribe_order_book_depths10(&mut self, cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        if !self.subscriptions_book_depth10.contains(&cmd.instrument_id) {
            self.subscriptions_book_depth10.insert(cmd.instrument_id);
            self.client.subscribe_book_depth10(cmd)?;
        }

        Ok(())
    }

    fn unsubscribe_book_depths10(&mut self, cmd: UnsubscribeBookDepth10) -> anyhow::Result<()> {
        if self.subscriptions_book_depth10.contains(&cmd.instrument_id) {
            self.subscriptions_book_depth10.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_depth10(cmd)?;
        }

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: SubscribeBookSnapshots) -> anyhow::Result<()> {
        if !self
            .subscriptions_book_snapshots
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_book_snapshots.insert(cmd.instrument_id);
            self.client.subscribe_book_snapshots(cmd)?;
        }

        Ok(())
    }

    fn unsubscribe_snapshots(&mut self, cmd: UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        if self
            .subscriptions_book_snapshots
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_book_snapshots.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_snapshots(cmd)?;
        }

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        if !self.subscriptions_quotes.contains(&cmd.instrument_id) {
            self.subscriptions_quotes.insert(cmd.instrument_id);
            self.client.subscribe_quotes(cmd)?;
        }
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: UnsubscribeQuotes) -> anyhow::Result<()> {
        if self.subscriptions_quotes.contains(&cmd.instrument_id) {
            self.subscriptions_quotes.remove(&cmd.instrument_id);
            self.client.unsubscribe_quotes(cmd)?;
        }
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        if !self.subscriptions_trades.contains(&cmd.instrument_id) {
            self.subscriptions_trades.insert(cmd.instrument_id);
            self.client.subscribe_trades(cmd)?;
        }
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: UnsubscribeTrades) -> anyhow::Result<()> {
        if self.subscriptions_trades.contains(&cmd.instrument_id) {
            self.subscriptions_trades.remove(&cmd.instrument_id);
            self.client.unsubscribe_trades(cmd)?;
        }
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        if !self.subscriptions_mark_prices.contains(&cmd.instrument_id) {
            self.subscriptions_mark_prices.insert(cmd.instrument_id);
            self.client.subscribe_mark_prices(cmd)?;
        }
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: UnsubscribeMarkPrices) -> anyhow::Result<()> {
        if self.subscriptions_mark_prices.contains(&cmd.instrument_id) {
            self.subscriptions_mark_prices.remove(&cmd.instrument_id);
            self.client.unsubscribe_mark_prices(cmd)?;
        }
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        if !self.subscriptions_index_prices.contains(&cmd.instrument_id) {
            self.subscriptions_index_prices.insert(cmd.instrument_id);
            self.client.subscribe_index_prices(cmd)?;
        }
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: UnsubscribeIndexPrices) -> anyhow::Result<()> {
        if self.subscriptions_index_prices.contains(&cmd.instrument_id) {
            self.subscriptions_index_prices.remove(&cmd.instrument_id);
            self.client.unsubscribe_index_prices(cmd)?;
        }
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        if !self.subscriptions_bars.contains(&cmd.bar_type) {
            self.subscriptions_bars.insert(cmd.bar_type);
            self.client.subscribe_bars(cmd)?;
        }
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: UnsubscribeBars) -> anyhow::Result<()> {
        if self.subscriptions_bars.contains(&cmd.bar_type) {
            self.subscriptions_bars.remove(&cmd.bar_type);
            self.client.unsubscribe_bars(cmd)?;
        }
        Ok(())
    }

    pub fn subscribe(&mut self, cmd: SubscribeData) -> anyhow::Result<()> {
        if !self.subscriptions_generic.contains(&cmd.data_type) {
            self.subscriptions_generic.insert(cmd.data_type.clone());
            self.client.subscribe(cmd)?;
        }
        Ok(())
    }

    pub fn unsubscribe(&mut self, cmd: UnsubscribeData) -> anyhow::Result<()> {
        if self.subscriptions_generic.contains(&cmd.data_type) {
            self.subscriptions_generic.remove(&cmd.data_type);
            self.client.unsubscribe(cmd)?;
        }
        Ok(())
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
        let limit = req.data_type.limit().map(|l| NonZeroUsize::new(l).unwrap()); // TODO

        match req.data_type.type_name() {
            stringify!(InstrumentAny) => match (instrument_id, venue) {
                (None, Some(venue)) => {
                    let instruments = self.client.request_instruments(
                        req.correlation_id,
                        venue,
                        start,
                        end,
                        &req.params,
                    );
                    self.handle_instruments(venue, instruments, req.correlation_id)
                }
                (Some(instrument_id), None) => {
                    let instrument = self.client.request_instrument(
                        req.correlation_id,
                        instrument_id,
                        start,
                        end,
                        &req.params,
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
                    &req.params,
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
                    &req.params,
                );
                self.handle_trade_ticks(&instrument_id, trades, req.correlation_id)
            }
            stringify!(Bar) => {
                let bar_type = req.data_type.bar_type();
                let bars = self.client.request_bars(
                    req.correlation_id,
                    bar_type,
                    start,
                    end,
                    limit,
                    &req.params,
                );
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
            self.clock.borrow().timestamp_ns(),
            None,
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
            self.clock.borrow().timestamp_ns(),
            None,
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
            self.clock.borrow().timestamp_ns(),
            None,
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
            self.clock.borrow().timestamp_ns(),
            None,
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
            self.clock.borrow().timestamp_ns(),
            None,
        )
    }
}

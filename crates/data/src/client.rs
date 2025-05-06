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
//!
//! Defines the `DataClient` trait, the `DataClientAdapter` for managing subscriptions and requests,
//! and utilities for constructing data responses.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use ahash::AHashSet;
use indexmap::IndexMap;
use nautilus_common::{
    clock::Clock,
    messages::data::{
        CustomDataResponse, RequestBars, RequestBookSnapshot, RequestData, RequestInstrument,
        RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, SubscribeData,
        SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
        SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
        UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeData, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    data::{Bar, BarType, DataType, QuoteTick, TradeTick},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};

/// Defines the interface for a data client, managing connections, subscriptions, and requests.
pub trait DataClient {
    /// Returns the unique identifier for this data client.
    fn client_id(&self) -> ClientId;

    /// Returns the optional venue this client is associated with.
    fn venue(&self) -> Option<Venue>;

    /// Starts the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn start(&self) -> anyhow::Result<()>;

    /// Stops the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn stop(&self) -> anyhow::Result<()>;

    /// Resets the data client to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn reset(&self) -> anyhow::Result<()>;

    /// Disposes of client resources and cleans up.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn dispose(&self) -> anyhow::Result<()>;

    /// Connects external API's if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn connect(&self) -> anyhow::Result<()>;

    /// Disconnects external API's if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn disconnect(&self) -> anyhow::Result<()>;

    /// Returns `true` if the client is currently connected.
    fn is_connected(&self) -> bool;

    /// Returns `true` if the client is currently disconnected.
    fn is_disconnected(&self) -> bool;

    // TODO: Move to separate trait
    // A [`LiveDataClient`] must have two channels to send back data and data responses
    // fn get_response_data_channel(&self) -> tokio::sync::mpsc::UnboundedSender<DataResponse>;
    // fn get_subscriber_data_channel(&self) -> tokio::sync::mpsc::UnboundedSender<Data>;

    /// Subscribes to generic data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe(&mut self, cmd: &SubscribeData) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to periodic order book snapshots for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from generic data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe(&mut self, cmd: &UnsubscribeData) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from data for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from periodic order book snapshots for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from instrument status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Unsubscribes from instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Sends a generic data request to the provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the data request fails.
    fn request_data(&self, request: &RequestData) -> anyhow::Result<()>;

    /// Requests a list of instruments from the provider for a given venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the instruments request fails.
    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    /// Requests detailed data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument request fails.
    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    /// Requests a snapshot of the order book for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the book snapshot request fails.
    fn request_book_snapshot(&self, request: &RequestBookSnapshot) -> anyhow::Result<()> {
        Ok(())
    }

    /// Requests historical or streaming quote data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the quotes request fails.
    fn request_quotes(&self, request: &RequestQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    /// Requests historical or streaming trade data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the trades request fails.
    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        Ok(())
    }

    /// Requests historical or streaming bar data for a specified instrument and bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bars request fails.
    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Wraps a [`DataClient`], managing subscription state and forwarding commands.
pub struct DataClientAdapter {
    client: Box<dyn DataClient>,
    clock: Rc<RefCell<dyn Clock>>,
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub handles_book_deltas: bool,
    pub handles_book_snapshots: bool,
    pub subscriptions_generic: AHashSet<DataType>,
    pub subscriptions_book_deltas: AHashSet<InstrumentId>,
    pub subscriptions_book_depth10: AHashSet<InstrumentId>,
    pub subscriptions_book_snapshots: AHashSet<InstrumentId>,
    pub subscriptions_quotes: AHashSet<InstrumentId>,
    pub subscriptions_trades: AHashSet<InstrumentId>,
    pub subscriptions_bars: AHashSet<BarType>,
    pub subscriptions_instrument_status: AHashSet<InstrumentId>,
    pub subscriptions_instrument_close: AHashSet<InstrumentId>,
    pub subscriptions_instrument: AHashSet<InstrumentId>,
    pub subscriptions_instrument_venue: AHashSet<Venue>,
    pub subscriptions_mark_prices: AHashSet<InstrumentId>,
    pub subscriptions_index_prices: AHashSet<InstrumentId>,
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
        f.debug_struct(stringify!(DataClientAdapter))
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
            .field("subscriptions_bars", &self.subscriptions_bars)
            .field("subscriptions_mark_prices", &self.subscriptions_mark_prices)
            .field(
                "subscriptions_index_prices",
                &self.subscriptions_index_prices,
            )
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
    /// Creates a new [`DataClientAdapter`] with the given client and clock, initializing empty subscriptions.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        venue: Option<Venue>,
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
            subscriptions_generic: AHashSet::new(),
            subscriptions_book_deltas: AHashSet::new(),
            subscriptions_book_depth10: AHashSet::new(),
            subscriptions_book_snapshots: AHashSet::new(),
            subscriptions_quotes: AHashSet::new(),
            subscriptions_trades: AHashSet::new(),
            subscriptions_mark_prices: AHashSet::new(),
            subscriptions_index_prices: AHashSet::new(),
            subscriptions_bars: AHashSet::new(),
            subscriptions_instrument_status: AHashSet::new(),
            subscriptions_instrument_close: AHashSet::new(),
            subscriptions_instrument: AHashSet::new(),
            subscriptions_instrument_venue: AHashSet::new(),
        }
    }

    #[inline]
    pub fn execute_subscribe_command(&mut self, cmd: &SubscribeCommand) {
        let result = match cmd {
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
    pub fn execute_unsubscribe_command(&mut self, cmd: &UnsubscribeCommand) {
        let result = match cmd {
            UnsubscribeCommand::Data(cmd) => self.unsubscribe(cmd),
            UnsubscribeCommand::Instrument(cmd) => self.unsubscribe_instrument(cmd),
            UnsubscribeCommand::Instruments(cmd) => self.unsubscribe_instruments(cmd),
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd),
            UnsubscribeCommand::BookDepth10(cmd) => self.unsubscribe_book_depth10(cmd),
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

    /// Subscribes to instrument definitions for a venue, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        if !self.subscriptions_instrument_venue.contains(&cmd.venue) {
            self.subscriptions_instrument_venue.insert(cmd.venue);
            self.client.subscribe_instruments(cmd)?;
        }

        Ok(())
    }

    /// Unsubscribes from instrument definition updates for a venue, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        if self.subscriptions_instrument_venue.contains(&cmd.venue) {
            self.subscriptions_instrument_venue.remove(&cmd.venue);
            self.client.unsubscribe_instruments(cmd)?;
        }

        Ok(())
    }

    /// Subscribes to instrument definitions for a single instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        if !self.subscriptions_instrument.contains(&cmd.instrument_id) {
            self.subscriptions_instrument.insert(cmd.instrument_id);
            self.client.subscribe_instrument(cmd)?;
        }

        Ok(())
    }

    /// Unsubscribes from instrument definition updates for a single instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        if self.subscriptions_instrument.contains(&cmd.instrument_id) {
            self.subscriptions_instrument.remove(&cmd.instrument_id);
            self.client.unsubscribe_instrument(cmd)?;
        }

        Ok(())
    }

    /// Subscribes to book deltas updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if !self.subscriptions_book_deltas.contains(&cmd.instrument_id) {
            self.subscriptions_book_deltas.insert(cmd.instrument_id);
            self.client.subscribe_book_deltas(cmd)?;
        }

        Ok(())
    }

    /// Unsubscribes from book deltas for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        if self.subscriptions_book_deltas.contains(&cmd.instrument_id) {
            self.subscriptions_book_deltas.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_deltas(cmd)?;
        }

        Ok(())
    }

    /// Subscribes to book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if !self.subscriptions_book_depth10.contains(&cmd.instrument_id) {
            self.subscriptions_book_depth10.insert(cmd.instrument_id);
            self.client.subscribe_book_depth10(cmd)?;
        }

        Ok(())
    }

    /// Unsubscribes from book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        if self.subscriptions_book_depth10.contains(&cmd.instrument_id) {
            self.subscriptions_book_depth10.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_depth10(cmd)?;
        }

        Ok(())
    }

    /// Subscribes to book snapshots for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if !self
            .subscriptions_book_snapshots
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_book_snapshots.insert(cmd.instrument_id);
            self.client.subscribe_book_snapshots(cmd)?;
        }

        Ok(())
    }

    /// Unsubscribes from book snapshots for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        if self
            .subscriptions_book_snapshots
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_book_snapshots.remove(&cmd.instrument_id);
            self.client.unsubscribe_book_snapshots(cmd)?;
        }

        Ok(())
    }

    /// Subscribes to quotes for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        if !self.subscriptions_quotes.contains(&cmd.instrument_id) {
            self.subscriptions_quotes.insert(cmd.instrument_id);
            self.client.subscribe_quotes(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from quotes for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        if self.subscriptions_quotes.contains(&cmd.instrument_id) {
            self.subscriptions_quotes.remove(&cmd.instrument_id);
            self.client.unsubscribe_quotes(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to trades for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        if !self.subscriptions_trades.contains(&cmd.instrument_id) {
            self.subscriptions_trades.insert(cmd.instrument_id);
            self.client.subscribe_trades(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from trades for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        if self.subscriptions_trades.contains(&cmd.instrument_id) {
            self.subscriptions_trades.remove(&cmd.instrument_id);
            self.client.unsubscribe_trades(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        if !self.subscriptions_mark_prices.contains(&cmd.instrument_id) {
            self.subscriptions_mark_prices.insert(cmd.instrument_id);
            self.client.subscribe_mark_prices(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        if self.subscriptions_mark_prices.contains(&cmd.instrument_id) {
            self.subscriptions_mark_prices.remove(&cmd.instrument_id);
            self.client.unsubscribe_mark_prices(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        if !self.subscriptions_index_prices.contains(&cmd.instrument_id) {
            self.subscriptions_index_prices.insert(cmd.instrument_id);
            self.client.subscribe_index_prices(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        if self.subscriptions_index_prices.contains(&cmd.instrument_id) {
            self.subscriptions_index_prices.remove(&cmd.instrument_id);
            self.client.unsubscribe_index_prices(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        if !self.subscriptions_bars.contains(&cmd.bar_type) {
            self.subscriptions_bars.insert(cmd.bar_type);
            self.client.subscribe_bars(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        if self.subscriptions_bars.contains(&cmd.bar_type) {
            self.subscriptions_bars.remove(&cmd.bar_type);
            self.client.unsubscribe_bars(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to a generic data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    pub fn subscribe(&mut self, cmd: &SubscribeData) -> anyhow::Result<()> {
        if !self.subscriptions_generic.contains(&cmd.data_type) {
            self.subscriptions_generic.insert(cmd.data_type.clone());
            self.client.subscribe(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from a generic data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    pub fn unsubscribe(&mut self, cmd: &UnsubscribeData) -> anyhow::Result<()> {
        if self.subscriptions_generic.contains(&cmd.data_type) {
            self.subscriptions_generic.remove(&cmd.data_type);
            self.client.unsubscribe(cmd)?;
        }
        Ok(())
    }

    // -- DATA REQUEST HANDLERS IMPLEMENTATION ---------------------------------------------------------------------------

    /// Sends a data request to the underlying client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client request fails.
    pub fn request_data(&self, req: &RequestData) -> anyhow::Result<()> {
        self.client.request_data(req)
    }

    /// Sends a single instrument request to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the request.
    pub fn request_instrument(&self, req: &RequestInstrument) -> anyhow::Result<()> {
        self.client.request_instrument(req)
    }

    /// Sends a batch instruments request to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the request.
    pub fn request_instruments(&self, req: &RequestInstruments) -> anyhow::Result<()> {
        self.client.request_instruments(req)
    }

    /// Sends a quotes request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the quotes request.
    pub fn request_quotes(&self, req: &RequestQuotes) -> anyhow::Result<()> {
        self.client.request_quotes(req)
    }

    /// Sends a trades request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the trades request.
    pub fn request_trades(&self, req: &RequestTrades) -> anyhow::Result<()> {
        self.client.request_trades(req)
    }

    /// Sends a bars request for a given instrument and bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the bars request.
    pub fn request_bars(&self, req: &RequestBars) -> anyhow::Result<()> {
        self.client.request_bars(req)
    }

    ///////////////////////////////////////////////////////////////////////////////////////////////
    // TODO: Below handler style is deprecated (need to update incorrect CustomDataResponse)
    ///////////////////////////////////////////////////////////////////////////////////////////////

    /// Constructs a `CustomDataResponse` wrapping a single instrument.
    #[must_use]
    pub fn handle_instrument(
        &self,
        instrument: InstrumentAny,
        correlation_id: UUID4,
    ) -> CustomDataResponse {
        let instrument_id = instrument.id();
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instrument);

        CustomDataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.borrow().timestamp_ns(),
            None,
        )
    }

    /// Constructs a `CustomDataResponse` wrapping multiple instruments for a venue.
    #[must_use]
    pub fn handle_instruments(
        &self,
        venue: Venue,
        instruments: Vec<InstrumentAny>,
        correlation_id: UUID4,
    ) -> CustomDataResponse {
        let metadata = IndexMap::from([("venue".to_string(), venue.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instruments);

        CustomDataResponse::new(
            correlation_id,
            self.client_id,
            venue,
            data_type,
            data,
            self.clock.borrow().timestamp_ns(),
            None,
        )
    }

    /// Constructs a `CustomDataResponse` carrying quote ticks for the specified instrument.
    #[must_use]
    pub fn handle_quotes(
        &self,
        instrument_id: &InstrumentId,
        quotes: Vec<QuoteTick>,
        correlation_id: UUID4,
    ) -> CustomDataResponse {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let data = Arc::new(quotes);

        CustomDataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.borrow().timestamp_ns(),
            None,
        )
    }

    /// Constructs a `CustomDataResponse` carrying trade ticks for the specified instrument.
    #[must_use]
    pub fn handle_trades(
        &self,
        instrument_id: &InstrumentId,
        trades: Vec<TradeTick>,
        correlation_id: UUID4,
    ) -> CustomDataResponse {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
        let data = Arc::new(trades);

        CustomDataResponse::new(
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.borrow().timestamp_ns(),
            None,
        )
    }

    /// Constructs a `CustomDataResponse` carrying bar data for the specified bar type.
    #[must_use]
    pub fn handle_bars(
        &self,
        bar_type: &BarType,
        bars: Vec<Bar>,
        correlation_id: UUID4,
    ) -> CustomDataResponse {
        let metadata = IndexMap::from([("bar_type".to_string(), bar_type.to_string())]);
        let data_type = DataType::new(stringify!(Bar), Some(metadata));
        let data = Arc::new(bars);

        CustomDataResponse::new(
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

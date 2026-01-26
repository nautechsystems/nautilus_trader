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

//! Base data client functionality.
//!
//! Provides the `DataClientAdapter` for managing subscriptions and requests,
//! and utilities for constructing data responses.

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use ahash::AHashSet;
use nautilus_common::{
    clients::{DataClient, log_command_error},
    messages::data::{
        RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestInstrument,
        RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeCommand, SubscribeCustomData, SubscribeFundingRates,
        SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
        SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
        UnsubscribeCommand, UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
    },
};
#[cfg(feature = "defi")]
use nautilus_model::defi::Blockchain;
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientId, InstrumentId, Venue},
};

#[cfg(feature = "defi")]
#[allow(unused_imports)] // Brings DeFi impl blocks into scope
use crate::defi::client as _;

/// Wraps a [`DataClient`], managing subscription state and forwarding commands.
pub struct DataClientAdapter {
    pub(crate) client: Box<dyn DataClient>,
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub handles_book_deltas: bool,
    pub handles_book_snapshots: bool,
    pub subscriptions_custom: AHashSet<DataType>,
    pub subscriptions_book_deltas: AHashSet<InstrumentId>,
    pub subscriptions_book_depth10: AHashSet<InstrumentId>,
    pub subscriptions_quotes: AHashSet<InstrumentId>,
    pub subscriptions_trades: AHashSet<InstrumentId>,
    pub subscriptions_bars: AHashSet<BarType>,
    pub subscriptions_instrument_status: AHashSet<InstrumentId>,
    pub subscriptions_instrument_close: AHashSet<InstrumentId>,
    pub subscriptions_instrument: AHashSet<InstrumentId>,
    pub subscriptions_instrument_venue: AHashSet<Venue>,
    pub subscriptions_mark_prices: AHashSet<InstrumentId>,
    pub subscriptions_index_prices: AHashSet<InstrumentId>,
    pub subscriptions_funding_rates: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub subscriptions_blocks: AHashSet<Blockchain>,
    #[cfg(feature = "defi")]
    pub subscriptions_pools: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub subscriptions_pool_swaps: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub subscriptions_pool_liquidity_updates: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub subscriptions_pool_fee_collects: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub subscriptions_pool_flash: AHashSet<InstrumentId>,
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
    #[rustfmt::skip]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DataClientAdapter))
            .field("client_id", &self.client_id)
            .field("venue", &self.venue)
            .field("handles_book_deltas", &self.handles_book_deltas)
            .field("handles_book_snapshots", &self.handles_book_snapshots)
            .field("subscriptions_custom", &self.subscriptions_custom)
            .field("subscriptions_book_deltas", &self.subscriptions_book_deltas)
            .field("subscriptions_book_depth10", &self.subscriptions_book_depth10)
            .field("subscriptions_quotes", &self.subscriptions_quotes)
            .field("subscriptions_trades", &self.subscriptions_trades)
            .field("subscriptions_bars", &self.subscriptions_bars)
            .field("subscriptions_mark_prices", &self.subscriptions_mark_prices)
            .field("subscriptions_index_prices", &self.subscriptions_index_prices)
            .field("subscriptions_instrument_status", &self.subscriptions_instrument_status)
            .field("subscriptions_instrument_close", &self.subscriptions_instrument_close)
            .field("subscriptions_instrument", &self.subscriptions_instrument)
            .field("subscriptions_instrument_venue", &self.subscriptions_instrument_venue)
            .finish()
    }
}

impl DataClientAdapter {
    /// Creates a new [`DataClientAdapter`] with the given client and clock.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        venue: Option<Venue>,
        handles_order_book_deltas: bool,
        handles_order_book_snapshots: bool,
        client: Box<dyn DataClient>,
    ) -> Self {
        Self {
            client,
            client_id,
            venue,
            handles_book_deltas: handles_order_book_deltas,
            handles_book_snapshots: handles_order_book_snapshots,
            subscriptions_custom: AHashSet::new(),
            subscriptions_book_deltas: AHashSet::new(),
            subscriptions_book_depth10: AHashSet::new(),
            subscriptions_quotes: AHashSet::new(),
            subscriptions_trades: AHashSet::new(),
            subscriptions_mark_prices: AHashSet::new(),
            subscriptions_index_prices: AHashSet::new(),
            subscriptions_funding_rates: AHashSet::new(),
            subscriptions_bars: AHashSet::new(),
            subscriptions_instrument_status: AHashSet::new(),
            subscriptions_instrument_close: AHashSet::new(),
            subscriptions_instrument: AHashSet::new(),
            subscriptions_instrument_venue: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_blocks: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_pools: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_pool_swaps: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_pool_liquidity_updates: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_pool_fee_collects: AHashSet::new(),
            #[cfg(feature = "defi")]
            subscriptions_pool_flash: AHashSet::new(),
        }
    }

    #[allow(clippy::borrowed_box)]
    #[must_use]
    pub fn get_client(&self) -> &Box<dyn DataClient> {
        &self.client
    }

    /// Connects the underlying client to the data provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.connect().await
    }

    /// Disconnects the underlying client from the data provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the disconnection fails.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.disconnect().await
    }

    #[inline]
    pub fn execute_subscribe(&mut self, cmd: &SubscribeCommand) {
        if let Err(e) = match cmd {
            SubscribeCommand::Data(cmd) => self.subscribe(cmd),
            SubscribeCommand::Instrument(cmd) => self.subscribe_instrument(cmd),
            SubscribeCommand::Instruments(cmd) => self.subscribe_instruments(cmd),
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(cmd),
            SubscribeCommand::BookDepth10(cmd) => self.subscribe_book_depth10(cmd),
            SubscribeCommand::BookSnapshots(_) => Ok(()), // Handled internally by engine
            SubscribeCommand::Quotes(cmd) => self.subscribe_quotes(cmd),
            SubscribeCommand::Trades(cmd) => self.subscribe_trades(cmd),
            SubscribeCommand::MarkPrices(cmd) => self.subscribe_mark_prices(cmd),
            SubscribeCommand::IndexPrices(cmd) => self.subscribe_index_prices(cmd),
            SubscribeCommand::FundingRates(cmd) => self.subscribe_funding_rates(cmd),
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(cmd),
            SubscribeCommand::InstrumentStatus(cmd) => self.subscribe_instrument_status(cmd),
            SubscribeCommand::InstrumentClose(cmd) => self.subscribe_instrument_close(cmd),
        } {
            log_command_error(&cmd, &e);
        }
    }

    #[inline]
    pub fn execute_unsubscribe(&mut self, cmd: &UnsubscribeCommand) {
        if let Err(e) = match cmd {
            UnsubscribeCommand::Data(cmd) => self.unsubscribe(cmd),
            UnsubscribeCommand::Instrument(cmd) => self.unsubscribe_instrument(cmd),
            UnsubscribeCommand::Instruments(cmd) => self.unsubscribe_instruments(cmd),
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd),
            UnsubscribeCommand::BookDepth10(cmd) => self.unsubscribe_book_depth10(cmd),
            UnsubscribeCommand::BookSnapshots(_) => Ok(()), // Handled internally by engine
            UnsubscribeCommand::Quotes(cmd) => self.unsubscribe_quotes(cmd),
            UnsubscribeCommand::Trades(cmd) => self.unsubscribe_trades(cmd),
            UnsubscribeCommand::Bars(cmd) => self.unsubscribe_bars(cmd),
            UnsubscribeCommand::MarkPrices(cmd) => self.unsubscribe_mark_prices(cmd),
            UnsubscribeCommand::IndexPrices(cmd) => self.unsubscribe_index_prices(cmd),
            UnsubscribeCommand::FundingRates(cmd) => self.unsubscribe_funding_rates(cmd),
            UnsubscribeCommand::InstrumentStatus(cmd) => self.unsubscribe_instrument_status(cmd),
            UnsubscribeCommand::InstrumentClose(cmd) => self.unsubscribe_instrument_close(cmd),
        } {
            log_command_error(&cmd, &e);
        }
    }

    // -- SUBSCRIPTION HANDLERS -------------------------------------------------------------------

    /// Subscribes to a custom data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    pub fn subscribe(&mut self, cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        if !self.subscriptions_custom.contains(&cmd.data_type) {
            self.subscriptions_custom.insert(cmd.data_type.clone());
            self.client.subscribe(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from a custom data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    pub fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if self.subscriptions_custom.contains(&cmd.data_type) {
            self.subscriptions_custom.remove(&cmd.data_type);
            self.client.unsubscribe(cmd)?;
        }
        Ok(())
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

    /// Subscribes to funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        if !self
            .subscriptions_funding_rates
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_funding_rates.insert(cmd.instrument_id);
            self.client.subscribe_funding_rates(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        if self
            .subscriptions_funding_rates
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_funding_rates.remove(&cmd.instrument_id);
            self.client.unsubscribe_funding_rates(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to instrument status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        if !self
            .subscriptions_instrument_status
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_instrument_status
                .insert(cmd.instrument_id);
            self.client.subscribe_instrument_status(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from instrument status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        if self
            .subscriptions_instrument_status
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_instrument_status
                .remove(&cmd.instrument_id);
            self.client.unsubscribe_instrument_status(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        if !self
            .subscriptions_instrument_close
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_instrument_close
                .insert(cmd.instrument_id);
            self.client.subscribe_instrument_close(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        if self
            .subscriptions_instrument_close
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_instrument_close
                .remove(&cmd.instrument_id);
            self.client.unsubscribe_instrument_close(cmd)?;
        }
        Ok(())
    }

    // -- REQUEST HANDLERS ------------------------------------------------------------------------

    /// Sends a data request to the underlying client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client request fails.
    pub fn request_data(&self, req: RequestCustomData) -> anyhow::Result<()> {
        self.client.request_data(req)
    }

    /// Sends a single instrument request to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the request.
    pub fn request_instrument(&self, req: RequestInstrument) -> anyhow::Result<()> {
        self.client.request_instrument(req)
    }

    /// Sends a batch instruments request to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the request.
    pub fn request_instruments(&self, req: RequestInstruments) -> anyhow::Result<()> {
        self.client.request_instruments(req)
    }

    /// Sends a book snapshot request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the book snapshot request.
    pub fn request_book_snapshot(&self, req: RequestBookSnapshot) -> anyhow::Result<()> {
        self.client.request_book_snapshot(req)
    }

    /// Sends a quotes request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the quotes request.
    pub fn request_quotes(&self, req: RequestQuotes) -> anyhow::Result<()> {
        self.client.request_quotes(req)
    }

    /// Sends a trades request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the trades request.
    pub fn request_trades(&self, req: RequestTrades) -> anyhow::Result<()> {
        self.client.request_trades(req)
    }

    /// Sends a bars request for a given instrument and bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the bars request.
    pub fn request_bars(&self, req: RequestBars) -> anyhow::Result<()> {
        self.client.request_bars(req)
    }

    /// Sends an order book depths request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the order book depths request.
    pub fn request_book_depth(&self, req: RequestBookDepth) -> anyhow::Result<()> {
        self.client.request_book_depth(req)
    }
}

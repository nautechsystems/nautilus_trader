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

use std::{
    any::Any,
    fmt::{Debug, Display},
    ops::{Deref, DerefMut},
};

use ahash::AHashSet;
use nautilus_common::messages::data::{
    RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestInstrument,
    RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
    SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, SubscribeCustomData,
    SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
    SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
    SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
    UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeCustomData, UnsubscribeFundingRates,
    UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentClose,
    UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes,
    UnsubscribeTrades,
};
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::{
    RequestPoolSnapshot, SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects,
    SubscribePoolFlashEvents, SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
    UnsubscribePool, UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents,
    UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
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

/// Defines the interface for a data client, managing connections, subscriptions, and requests.
#[async_trait::async_trait]
pub trait DataClient: Any + Sync + Send {
    /// Returns the unique identifier for this data client.
    fn client_id(&self) -> ClientId;

    /// Returns the optional venue this client is associated with.
    fn venue(&self) -> Option<Venue>;

    /// Starts the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Resets the data client to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn reset(&mut self) -> anyhow::Result<()>;

    /// Disposes of client resources and cleans up.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn dispose(&mut self) -> anyhow::Result<()>;

    /// Connects external API's if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Disconnects external API's if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    async fn disconnect(&mut self) -> anyhow::Result<()>;

    /// Returns `true` if the client is currently connected.
    fn is_connected(&self) -> bool;

    /// Returns `true` if the client is currently disconnected.
    fn is_disconnected(&self) -> bool;

    /// Subscribes to custom data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe(&mut self, cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to periodic order book snapshots for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
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
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to blocks for a specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_blocks(&mut self, cmd: &SubscribeBlocks) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool definition updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool(&mut self, cmd: &SubscribePool) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool swaps for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_swaps(&mut self, cmd: &SubscribePoolSwaps) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool liquidity updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: &SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool fee collects for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_fee_collects(
        &mut self,
        cmd: &SubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool flash loan events for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_flash_events(
        &mut self,
        cmd: &SubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from custom data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from data for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from periodic order book snapshots for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
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
        log_not_implemented(&cmd);
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
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from blocks for a specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool definition updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from swaps for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool liquidity updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool fee collects for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_fee_collects(
        &mut self,
        cmd: &UnsubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool flash loan events for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_flash_events(
        &mut self,
        cmd: &UnsubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Sends a custom data request to the provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the data request fails.
    fn request_data(&self, request: &RequestCustomData) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests a list of instruments from the provider for a given venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the instruments request fails.
    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests detailed data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument request fails.
    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests a snapshot of the order book for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the book snapshot request fails.
    fn request_book_snapshot(&self, request: &RequestBookSnapshot) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming quote data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the quotes request fails.
    fn request_quotes(&self, request: &RequestQuotes) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming trade data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the trades request fails.
    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming bar data for a specified instrument and bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bars request fails.
    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical order book depth data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the order book depths request fails.
    fn request_book_depth(&self, request: &RequestBookDepth) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Requests a snapshot of a specific AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool snapshot request fails.
    fn request_pool_snapshot(&self, request: &RequestPoolSnapshot) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }
}

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
            .field("subscriptions_book_snapshot", &self.subscriptions_book_snapshots)
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
            subscriptions_book_snapshots: AHashSet::new(),
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

    #[inline]
    pub fn execute_subscribe(&mut self, cmd: &SubscribeCommand) {
        if let Err(e) = match cmd {
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
            UnsubscribeCommand::BookSnapshots(cmd) => self.unsubscribe_book_snapshots(cmd),
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
    pub fn request_data(&self, req: &RequestCustomData) -> anyhow::Result<()> {
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

    /// Sends an order book depths request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the order book depths request.
    pub fn request_book_depth(&self, req: &RequestBookDepth) -> anyhow::Result<()> {
        self.client.request_book_depth(req)
    }
}

#[inline(always)]
fn log_not_implemented<T: Debug>(msg: &T) {
    log::warn!("{msg:?} – handler not implemented");
}

#[inline(always)]
fn log_command_error<C: Debug, E: Display>(cmd: &C, e: &E) {
    log::error!("Error on {cmd:?}: {e}");
}

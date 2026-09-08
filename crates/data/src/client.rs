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
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Deref, DerefMut},
};

use ahash::AHashSet;
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::DefiSubscribeCommand;
use nautilus_common::{
    clients::{DataClient, log_command_error},
    enums::LogColor,
    log_info,
    messages::data::{
        RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestFundingRates,
        RequestInstrument, RequestInstruments, RequestOptionChainReferencePrice, RequestQuotes,
        RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeCommand,
        SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
        SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
        SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
        UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeCommand,
        UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes,
        UnsubscribeTrades,
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
#[cfg(feature = "defi")]
use crate::subscription::DefiSubscriptionKey;
use crate::subscription::{SubscriptionKey, SubscriptionRegistry, SubscriptionRelease};

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
    pub subscriptions_option_greeks: AHashSet<InstrumentId>,
    subscriptions_active: SubscriptionRegistry<SubscriptionKey, SubscribeCommand>,
    #[cfg(feature = "defi")]
    pub(crate) subscriptions_active_defi:
        SubscriptionRegistry<DefiSubscriptionKey, DefiSubscribeCommand>,
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
            subscriptions_option_greeks: AHashSet::new(),
            subscriptions_bars: AHashSet::new(),
            subscriptions_instrument_status: AHashSet::new(),
            subscriptions_instrument_close: AHashSet::new(),
            subscriptions_instrument: AHashSet::new(),
            subscriptions_instrument_venue: AHashSet::new(),
            subscriptions_active: SubscriptionRegistry::default(),
            #[cfg(feature = "defi")]
            subscriptions_active_defi: SubscriptionRegistry::default(),
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

    #[expect(clippy::borrowed_box)]
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
    pub fn execute_subscribe(&mut self, cmd: SubscribeCommand) {
        self.execute_subscribe_with_retained(cmd.clone(), cmd, false);
    }

    pub(crate) fn execute_subscribe_intent(&mut self, cmd: SubscribeCommand) {
        self.execute_subscribe_with_retained(cmd.clone(), cmd, true);
    }

    pub(crate) fn execute_subscribe_with_retained(
        &mut self,
        cmd: SubscribeCommand,
        retained: SubscribeCommand,
        retain_on_failure: bool,
    ) {
        let key = SubscriptionKey::from_subscribe(&retained);
        if self.has_active_subscription(&retained) {
            self.subscriptions_active
                .retain(key, retained.command_id(), retained);
            return;
        }

        let cmd_debug = format!("{cmd:?}");
        let result = match cmd {
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
            SubscribeCommand::OptionGreeks(cmd) => self.subscribe_option_greeks(cmd),
            SubscribeCommand::OptionChain(_) => Ok(()), // Handled internally by engine
        };

        if let Err(e) = result {
            // Engine-owned intent survives failure until its matching release
            if retain_on_failure {
                self.subscriptions_active
                    .retain(key, retained.command_id(), retained);
            }

            log_command_error(&cmd_debug, &e);
            return;
        }

        // A retry can establish a different physical identity from the failed attempt
        if let Some(subscription) = self.subscriptions_active.get_mut(&key) {
            subscription.command = retained.clone();
        }
        self.subscriptions_active
            .retain(key, retained.command_id(), retained);
    }

    #[must_use]
    pub(crate) fn has_active_subscription(&self, cmd: &SubscribeCommand) -> bool {
        match cmd {
            SubscribeCommand::Data(cmd) => self.subscriptions_custom.contains(&cmd.data_type),
            SubscribeCommand::Instrument(cmd) => {
                self.subscriptions_instrument.contains(&cmd.instrument_id)
            }
            SubscribeCommand::Instruments(cmd) => {
                self.subscriptions_instrument_venue.contains(&cmd.venue)
            }
            SubscribeCommand::BookDeltas(cmd) => {
                self.subscriptions_book_deltas.contains(&cmd.instrument_id)
            }
            SubscribeCommand::BookDepth10(cmd) => {
                self.subscriptions_book_depth10.contains(&cmd.instrument_id)
            }
            SubscribeCommand::Quotes(cmd) => self.subscriptions_quotes.contains(&cmd.instrument_id),
            SubscribeCommand::Trades(cmd) => self.subscriptions_trades.contains(&cmd.instrument_id),
            SubscribeCommand::Bars(cmd) => self.subscriptions_bars.contains(&cmd.bar_type),
            SubscribeCommand::MarkPrices(cmd) => {
                self.subscriptions_mark_prices.contains(&cmd.instrument_id)
            }
            SubscribeCommand::IndexPrices(cmd) => {
                self.subscriptions_index_prices.contains(&cmd.instrument_id)
            }
            SubscribeCommand::FundingRates(cmd) => self
                .subscriptions_funding_rates
                .contains(&cmd.instrument_id),
            SubscribeCommand::InstrumentStatus(cmd) => self
                .subscriptions_instrument_status
                .contains(&cmd.instrument_id),
            SubscribeCommand::InstrumentClose(cmd) => self
                .subscriptions_instrument_close
                .contains(&cmd.instrument_id),
            SubscribeCommand::OptionGreeks(cmd) => self
                .subscriptions_option_greeks
                .contains(&cmd.instrument_id),
            SubscribeCommand::BookSnapshots(_) | SubscribeCommand::OptionChain(_) => self
                .subscriptions_active
                .contains(&SubscriptionKey::from_subscribe(cmd)),
        }
    }

    pub(crate) fn clear_subscription_state(&mut self) {
        self.subscriptions_custom.clear();
        self.subscriptions_book_deltas.clear();
        self.subscriptions_book_depth10.clear();
        self.subscriptions_quotes.clear();
        self.subscriptions_trades.clear();
        self.subscriptions_bars.clear();
        self.subscriptions_instrument_status.clear();
        self.subscriptions_instrument_close.clear();
        self.subscriptions_instrument.clear();
        self.subscriptions_instrument_venue.clear();
        self.subscriptions_mark_prices.clear();
        self.subscriptions_index_prices.clear();
        self.subscriptions_funding_rates.clear();
        self.subscriptions_option_greeks.clear();
        self.subscriptions_active.clear();

        #[cfg(feature = "defi")]
        {
            self.subscriptions_active_defi.clear();
            self.subscriptions_blocks.clear();
            self.subscriptions_pools.clear();
            self.subscriptions_pool_swaps.clear();
            self.subscriptions_pool_liquidity_updates.clear();
            self.subscriptions_pool_fee_collects.clear();
            self.subscriptions_pool_flash.clear();
        }
    }

    #[inline]
    pub fn execute_unsubscribe(&mut self, cmd: &UnsubscribeCommand) {
        let key = SubscriptionKey::from_unsubscribe(cmd);
        let command = match self.subscriptions_active.release(&key) {
            SubscriptionRelease::Retained => return,
            SubscriptionRelease::Final(subscribe) => {
                subscribe.into_unsubscribe(cmd.command_id(), cmd.ts_init(), cmd.correlation_id())
            }
            SubscriptionRelease::Untracked => cmd.clone(),
        };

        if let Err(e) = match &command {
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
            UnsubscribeCommand::OptionGreeks(cmd) => self.unsubscribe_option_greeks(cmd),
            UnsubscribeCommand::OptionChain(_) => Ok(()), // Handled internally by engine
        } {
            log_command_error(&command, &e);
        } else {
            self.subscriptions_active.remove(&key);
        }
    }

    /// Subscribes to a custom data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    pub fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        let data_type = cmd.data_type.clone();
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_custom,
            data_type,
            "",
            |client| client.subscribe(cmd),
        )
    }

    /// Unsubscribes from a custom data type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    pub fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_custom,
            &cmd.data_type,
            "",
            |client| client.unsubscribe(cmd),
        )
    }

    /// Subscribes to instrument definitions for a venue, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instruments(&mut self, cmd: SubscribeInstruments) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_venue,
            cmd.venue,
            "instruments",
            |client| client.subscribe_instruments(cmd),
        )
    }

    /// Unsubscribes from instrument definition updates for a venue, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_venue,
            &cmd.venue,
            "instruments",
            |client| client.unsubscribe_instruments(cmd),
        )
    }

    /// Subscribes to instrument definitions for a single instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument,
            cmd.instrument_id,
            "instrument",
            |client| client.subscribe_instrument(cmd),
        )
    }

    /// Unsubscribes from instrument definition updates for a single instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument,
            &cmd.instrument_id,
            "instrument",
            |client| client.unsubscribe_instrument(cmd),
        )
    }

    /// Subscribes to book deltas updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_book_deltas,
            cmd.instrument_id,
            "order book deltas",
            |client| client.subscribe_book_deltas(cmd),
        )
    }

    /// Unsubscribes from book deltas for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_book_deltas,
            &cmd.instrument_id,
            "order book deltas",
            |client| client.unsubscribe_book_deltas(cmd),
        )
    }

    /// Subscribes to book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_depth10(&mut self, cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_book_depth10,
            cmd.instrument_id,
            "order book depth",
            |client| client.subscribe_book_depth10(cmd),
        )
    }

    /// Unsubscribes from book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_book_depth10,
            &cmd.instrument_id,
            "order book depth",
            |client| client.unsubscribe_book_depth10(cmd),
        )
    }

    /// Subscribes to quotes for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_quotes,
            cmd.instrument_id,
            "quotes",
            |client| client.subscribe_quotes(cmd),
        )
    }

    /// Unsubscribes from quotes for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_quotes,
            &cmd.instrument_id,
            "quotes",
            |client| client.unsubscribe_quotes(cmd),
        )
    }

    /// Subscribes to trades for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_trades,
            cmd.instrument_id,
            "trades",
            |client| client.subscribe_trades(cmd),
        )
    }

    /// Unsubscribes from trades for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_trades,
            &cmd.instrument_id,
            "trades",
            |client| client.unsubscribe_trades(cmd),
        )
    }

    /// Subscribes to bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_bars,
            cmd.bar_type,
            "bars",
            |client| client.subscribe_bars(cmd),
        )
    }

    /// Unsubscribes from bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_bars,
            &cmd.bar_type,
            "bars",
            |client| client.unsubscribe_bars(cmd),
        )
    }

    /// Subscribes to mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_mark_prices,
            cmd.instrument_id,
            "mark prices",
            |client| client.subscribe_mark_prices(cmd),
        )
    }

    /// Unsubscribes from mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_mark_prices,
            &cmd.instrument_id,
            "mark prices",
            |client| client.unsubscribe_mark_prices(cmd),
        )
    }

    /// Subscribes to index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_index_prices,
            cmd.instrument_id,
            "index prices",
            |client| client.subscribe_index_prices(cmd),
        )
    }

    /// Unsubscribes from index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_index_prices,
            &cmd.instrument_id,
            "index prices",
            |client| client.unsubscribe_index_prices(cmd),
        )
    }

    /// Subscribes to funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_funding_rates,
            cmd.instrument_id,
            "funding rates",
            |client| client.subscribe_funding_rates(cmd),
        )
    }

    /// Unsubscribes from funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_funding_rates,
            &cmd.instrument_id,
            "funding rates",
            |client| client.unsubscribe_funding_rates(cmd),
        )
    }

    /// Subscribes to instrument status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_status,
            cmd.instrument_id,
            "instrument status",
            |client| client.subscribe_instrument_status(cmd),
        )
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
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_status,
            &cmd.instrument_id,
            "instrument status",
            |client| client.unsubscribe_instrument_status(cmd),
        )
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument_close(&mut self, cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_close,
            cmd.instrument_id,
            "instrument close",
            |client| client.subscribe_instrument_close(cmd),
        )
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
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_instrument_close,
            &cmd.instrument_id,
            "instrument close",
            |client| client.unsubscribe_instrument_close(cmd),
        )
    }

    /// Subscribes to option greeks for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_option_greeks,
            cmd.instrument_id,
            "option greeks",
            |client| client.subscribe_option_greeks(cmd),
        )
    }

    /// Unsubscribes from option greeks for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_option_greeks,
            &cmd.instrument_id,
            "option greeks",
            |client| client.unsubscribe_option_greeks(cmd),
        )
    }

    pub(crate) fn execute_tracked_subscribe<T>(
        client: &mut dyn DataClient,
        set: &mut AHashSet<T>,
        key: T,
        data_type: &str,
        subscribe: impl FnOnce(&mut dyn DataClient) -> anyhow::Result<()>,
    ) -> anyhow::Result<()>
    where
        T: Eq + Hash + Display,
    {
        if set.contains(&key) {
            return Ok(());
        }

        subscribe(client)?;

        if data_type.is_empty() {
            log_info!("Subscribed {key}", color = LogColor::Blue);
        } else {
            log_info!("Subscribed {key} {data_type}", color = LogColor::Blue);
        }
        set.insert(key);
        Ok(())
    }

    pub(crate) fn execute_tracked_unsubscribe<T>(
        client: &mut dyn DataClient,
        set: &mut AHashSet<T>,
        key: &T,
        data_type: &str,
        unsubscribe: impl FnOnce(&mut dyn DataClient) -> anyhow::Result<()>,
    ) -> anyhow::Result<()>
    where
        T: Eq + Hash + Display,
    {
        if !set.contains(key) {
            return Ok(());
        }

        unsubscribe(client)?;
        set.remove(key);
        if data_type.is_empty() {
            log_info!("Unsubscribed {key}", color = LogColor::Blue);
        } else {
            log_info!("Unsubscribed {key} {data_type}", color = LogColor::Blue);
        }
        Ok(())
    }

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

    /// Sends a funding rates request for a given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the trades request.
    pub fn request_funding_rates(&self, req: RequestFundingRates) -> anyhow::Result<()> {
        self.client.request_funding_rates(req)
    }

    /// Sends an option-chain reference price request.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the option-chain reference price request.
    pub fn request_option_chain_reference_price(
        &self,
        req: RequestOptionChainReferencePrice,
    ) -> anyhow::Result<()> {
        self.client.request_option_chain_reference_price(req)
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

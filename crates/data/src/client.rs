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
use indexmap::IndexMap;
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::DefiSubscribeCommand;
use nautilus_common::{
    clients::{DataClient, log_command_error},
    enums::LogColor,
    log_info,
    messages::data::{
        RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestFundingRates,
        RequestInstrument, RequestInstruments, RequestOptionChainReferencePrice, RequestQuotes,
        RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeCommand,
        SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
        SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
        SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
        SubscriptionError, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth,
        UnsubscribeCommand, UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes,
        UnsubscribeTrades, ensure_routable_bar_types, ensure_routable_instrument_ids,
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
use crate::subscription::{
    SubscriptionKey, SubscriptionRegistry, SubscriptionRelease, split_subscribe, split_unsubscribe,
};

/// Wraps a [`DataClient`], managing subscription state and forwarding commands.
pub struct DataClientAdapter {
    pub(crate) client: Box<dyn DataClient>,
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub handles_book_deltas: bool,
    pub handles_book_snapshots: bool,
    pub subscriptions_custom: AHashSet<DataType>,
    pub subscriptions_book_deltas: AHashSet<InstrumentId>,
    pub subscriptions_book_depth: AHashSet<InstrumentId>,
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
            .field("subscriptions_book_depth", &self.subscriptions_book_depth)
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
            subscriptions_book_depth: AHashSet::new(),
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
        for command in [&cmd, &retained] {
            if let Some(bar_types) = command.bar_types()
                && let Err(e) = ensure_routable_bar_types(bar_types, command.client_id().is_some())
            {
                log_command_error(
                    &format_args!(
                        "Subscribe {} command {}",
                        self.client_id(),
                        command.command_id()
                    ),
                    &e,
                );
                return;
            }

            if let Some(ids) = command.instrument_ids()
                && let Err(e) = ensure_routable_instrument_ids(ids, command.client_id().is_some())
            {
                log_command_error(
                    &format_args!(
                        "Subscribe {} command {}",
                        self.client_id(),
                        command.command_id()
                    ),
                    &e,
                );
                return;
            }
        }

        self.subscribe_retained(cmd, retained, retain_on_failure);
    }

    pub(crate) fn retain_intent(&mut self, command: &SubscribeCommand) {
        for member in split_subscribe(command.clone()) {
            self.subscriptions_active.retain(
                SubscriptionKey::from_subscribe(&member),
                member.command_id(),
                member,
            );
        }
    }

    fn subscribe_retained(
        &mut self,
        cmd: SubscribeCommand,
        retained: SubscribeCommand,
        retain_on_failure: bool,
    ) {
        let mut pending = Vec::new();

        for member in split_subscribe(retained) {
            let key = SubscriptionKey::from_subscribe(&member);
            if self.has_active_subscription(&member) {
                self.subscriptions_active
                    .retain(key, member.command_id(), member);
            } else {
                pending.push((key, member));
            }
        }

        if pending.is_empty() {
            return;
        }

        let command_id = cmd.command_id();

        let result = match cmd {
            SubscribeCommand::Data(cmd) => self.subscribe(cmd),
            SubscribeCommand::Instrument(cmd) => self.subscribe_instrument(&cmd),
            SubscribeCommand::Instruments(cmd) => self.subscribe_instruments(cmd),
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(&cmd),
            SubscribeCommand::BookDepth(cmd) => self.subscribe_book_depth(&cmd),
            SubscribeCommand::BookSnapshots(_) => Ok(()), // Handled internally by engine
            SubscribeCommand::Quotes(cmd) => self.subscribe_quotes(&cmd),
            SubscribeCommand::Trades(cmd) => self.subscribe_trades(&cmd),
            SubscribeCommand::MarkPrices(cmd) => self.subscribe_mark_prices(&cmd),
            SubscribeCommand::IndexPrices(cmd) => self.subscribe_index_prices(&cmd),
            SubscribeCommand::FundingRates(cmd) => self.subscribe_funding_rates(&cmd),
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(&cmd),
            SubscribeCommand::InstrumentStatus(cmd) => self.subscribe_instrument_status(&cmd),
            SubscribeCommand::InstrumentClose(cmd) => self.subscribe_instrument_close(&cmd),
            SubscribeCommand::OptionGreeks(cmd) => self.subscribe_option_greeks(&cmd),
            SubscribeCommand::OptionChain(_) => Ok(()), // Handled internally by engine
        };

        if let Err(e) = &result {
            let subscription = pending
                .iter()
                .filter(|(_, member)| !self.has_active_subscription(member))
                .map(|(key, _)| key.log_context())
                .collect::<Vec<_>>()
                .join(", ");

            log_command_error(
                &format_args!(
                    "Subscribe {} unaccepted [{subscription}] command {command_id}",
                    self.client_id()
                ),
                e,
            );
        }

        self.retain_subscriptions(pending, &result, retain_on_failure);
    }

    fn retain_subscriptions(
        &mut self,
        pending: Vec<(SubscriptionKey, SubscribeCommand)>,
        result: &anyhow::Result<()>,
        retain_on_failure: bool,
    ) {
        for (key, member) in pending {
            let accepted = result.is_ok() || self.has_active_subscription(&member);
            // Engine-owned intent survives failure until its matching release
            if !accepted && !retain_on_failure {
                continue;
            }

            if accepted && let Some(subscription) = self.subscriptions_active.get_mut(&key) {
                subscription.command = member.clone();
            }

            self.subscriptions_active
                .retain(key, member.command_id(), member);
        }
    }

    #[must_use]
    #[rustfmt::skip]
    pub(crate) fn has_active_subscription(&self, cmd: &SubscribeCommand) -> bool {
        match cmd {
            SubscribeCommand::Data(cmd) => self.subscriptions_custom.contains(&cmd.data_type),
            SubscribeCommand::Instrument(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_instrument.contains(id)),
            SubscribeCommand::Instruments(cmd) => self.subscriptions_instrument_venue.contains(&cmd.venue),
            SubscribeCommand::BookDeltas(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_book_deltas.contains(id)),
            SubscribeCommand::BookDepth(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_book_depth.contains(id)),
            SubscribeCommand::Quotes(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_quotes.contains(id)),
            SubscribeCommand::Trades(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_trades.contains(id)),
            SubscribeCommand::Bars(cmd) => !cmd.bar_types.is_empty() && cmd.bar_types.iter().all(|bar_type| self.subscriptions_bars.contains(bar_type)),
            SubscribeCommand::MarkPrices(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_mark_prices.contains(id)),
            SubscribeCommand::IndexPrices(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_index_prices.contains(id)),
            SubscribeCommand::FundingRates(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_funding_rates.contains(id)),
            SubscribeCommand::InstrumentStatus(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_instrument_status.contains(id)),
            SubscribeCommand::InstrumentClose(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_instrument_close.contains(id)),
            SubscribeCommand::OptionGreeks(cmd) => !cmd.instrument_ids.is_empty() && cmd.instrument_ids.iter().all(|id| self.subscriptions_option_greeks.contains(id)),
            SubscribeCommand::BookSnapshots(_) | SubscribeCommand::OptionChain(_) => self.subscriptions_active.contains(&SubscriptionKey::from_subscribe(cmd)),
        }
    }

    pub(crate) fn clear_subscription_state(&mut self) {
        self.subscriptions_custom.clear();
        self.subscriptions_book_deltas.clear();
        self.subscriptions_book_depth.clear();
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
        if let Some(bar_types) = cmd.bar_types()
            && let Err(e) = ensure_routable_bar_types(bar_types, cmd.client_id().is_some())
        {
            log_command_error(
                &format_args!(
                    "Unsubscribe {} command {}",
                    self.client_id(),
                    cmd.command_id()
                ),
                &e,
            );
            return;
        }

        if let Some(ids) = cmd.instrument_ids()
            && let Err(e) = ensure_routable_instrument_ids(ids, cmd.client_id().is_some())
        {
            log_command_error(
                &format_args!(
                    "Unsubscribe {} command {}",
                    self.client_id(),
                    cmd.command_id()
                ),
                &e,
            );
            return;
        }

        let mut releases = IndexMap::new();

        for member in split_unsubscribe(cmd) {
            let key = SubscriptionKey::from_unsubscribe(&member);

            let (acquisition_id, command) = match self.subscriptions_active.release(&key) {
                SubscriptionRelease::Retained => continue,
                SubscriptionRelease::Final(subscribe) => (
                    subscribe.command_id(),
                    subscribe.into_unsubscribe(
                        cmd.command_id(),
                        cmd.ts_init(),
                        cmd.correlation_id(),
                    ),
                ),
                SubscriptionRelease::Untracked => (cmd.command_id(), member),
            };

            releases
                .entry(acquisition_id)
                .or_insert_with(Vec::new)
                .push(command);
        }

        for command in releases.into_values().flat_map(UnsubscribeCommand::group) {
            self.release_subscription(&command);
        }
    }

    fn release_subscription(&mut self, command: &UnsubscribeCommand) {
        let result = match command {
            UnsubscribeCommand::Data(cmd) => self.unsubscribe(cmd),
            UnsubscribeCommand::Instrument(cmd) => self.unsubscribe_instrument(cmd),
            UnsubscribeCommand::Instruments(cmd) => self.unsubscribe_instruments(cmd),
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd),
            UnsubscribeCommand::BookDepth(cmd) => self.unsubscribe_book_depth(cmd),
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
        };

        let members = split_unsubscribe(command);
        for member in &members {
            let key = SubscriptionKey::from_unsubscribe(member);
            let subscribe = self
                .subscriptions_active
                .get_mut(&key)
                .map(|entry| entry.command.clone());

            if subscribe.is_none_or(|subscribe| !self.has_active_subscription(&subscribe)) {
                self.subscriptions_active.remove(&key);
            }
        }

        if let Err(e) = result {
            let subscription = members
                .iter()
                .map(SubscriptionKey::from_unsubscribe)
                .filter(|key| self.subscriptions_active.contains(key))
                .map(|key| key.log_context())
                .collect::<Vec<_>>()
                .join(", ");

            log_command_error(
                &format_args!(
                    "Unsubscribe {} unreleased [{subscription}] command {}",
                    self.client_id(),
                    command.command_id()
                ),
                &e,
            );
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
    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_instrument,
            &cmd.instrument_ids,
            "instrument",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_instrument(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from instrument definition updates for a single instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_instrument,
            &cmd.instrument_ids,
            "instrument",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_instrument(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to book deltas updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_book_deltas,
            &cmd.instrument_ids,
            "order book deltas",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_book_deltas(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from book deltas for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_book_deltas,
            &cmd.instrument_ids,
            "order book deltas",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_book_deltas(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_book_depth(&mut self, cmd: &SubscribeBookDepth) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_book_depth,
            &cmd.instrument_ids,
            "order book depth",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_book_depth(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from book depth updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_book_depth(&mut self, cmd: &UnsubscribeBookDepth) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_book_depth,
            &cmd.instrument_ids,
            "order book depth",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_book_depth(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to quotes for instruments, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_quotes,
            &cmd.instrument_ids,
            "quotes",
            |partial| &partial.instrument_ids_accepted,
            |ids| self.client.subscribe_quotes(cmd.with_instrument_ids(ids)),
        )
    }

    /// Unsubscribes from quotes for instruments, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_quotes,
            &cmd.instrument_ids,
            "quotes",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_quotes(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to trades for instruments, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_trades,
            &cmd.instrument_ids,
            "trades",
            |partial| &partial.instrument_ids_accepted,
            |ids| self.client.subscribe_trades(cmd.with_instrument_ids(ids)),
        )
    }

    /// Unsubscribes from trades for instruments, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_trades,
            &cmd.instrument_ids,
            "trades",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_trades(&cmd.with_instrument_ids(ids))
            },
        )
    }

    fn subscribe_batch<T: Copy + Eq + Hash + Display>(
        subscriptions: &mut AHashSet<T>,
        members: &[T],
        label: &str,
        accepted: fn(&SubscriptionError) -> &[T],
        dispatch: impl FnOnce(&[T]) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let ids: Vec<_> = members
            .iter()
            .copied()
            .filter(|id| !subscriptions.contains(id))
            .collect();

        if ids.is_empty() {
            return Ok(());
        }

        let result = dispatch(&ids);

        let reported: AHashSet<_> = match &result {
            Ok(()) => AHashSet::new(),
            Err(e) => e
                .downcast_ref::<SubscriptionError>()
                .map_or(&[][..], accepted)
                .iter()
                .copied()
                .collect(),
        };

        for id in ids
            .iter()
            .filter(|id| result.is_ok() || reported.contains(*id))
        {
            subscriptions.insert(*id);
            log_info!("Subscribed {id} {label}", color = LogColor::Blue);
        }

        result
    }

    fn unsubscribe_batch<T: Copy + Eq + Hash + Display>(
        subscriptions: &mut AHashSet<T>,
        members: &[T],
        label: &str,
        accepted: fn(&SubscriptionError) -> &[T],
        dispatch: impl FnOnce(&[T]) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let ids: Vec<_> = members
            .iter()
            .copied()
            .filter(|id| subscriptions.contains(id))
            .collect();

        if ids.is_empty() {
            return Ok(());
        }

        let result = dispatch(&ids);

        let reported: AHashSet<_> = match &result {
            Ok(()) => AHashSet::new(),
            Err(e) => e
                .downcast_ref::<SubscriptionError>()
                .map_or(&[][..], accepted)
                .iter()
                .copied()
                .collect(),
        };

        for id in ids
            .iter()
            .filter(|id| result.is_ok() || reported.contains(*id))
        {
            subscriptions.remove(id);
            log_info!("Unsubscribed {id} {label}", color = LogColor::Blue);
        }

        result
    }

    /// Subscribes to bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_bars,
            &cmd.bar_types,
            "bars",
            |partial| &partial.bar_types_accepted,
            |ids| self.client.subscribe_bars(cmd.with_bar_types(ids)),
        )
    }

    /// Unsubscribes from bars for a bar type, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_bars,
            &cmd.bar_types,
            "bars",
            |partial| &partial.bar_types_accepted,
            |ids| self.client.unsubscribe_bars(&cmd.with_bar_types(ids)),
        )
    }

    /// Subscribes to mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_mark_prices,
            &cmd.instrument_ids,
            "mark prices",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_mark_prices(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from mark price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_mark_prices,
            &cmd.instrument_ids,
            "mark prices",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_mark_prices(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_index_prices,
            &cmd.instrument_ids,
            "index prices",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_index_prices(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from index price updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_index_prices,
            &cmd.instrument_ids,
            "index prices",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_index_prices(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_funding_rates,
            &cmd.instrument_ids,
            "funding rates",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_funding_rates(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from funding rate updates for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_funding_rates,
            &cmd.instrument_ids,
            "funding rates",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_funding_rates(&cmd.with_instrument_ids(ids))
            },
        )
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
        Self::subscribe_batch(
            &mut self.subscriptions_instrument_status,
            &cmd.instrument_ids,
            "instrument status",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_instrument_status(cmd.with_instrument_ids(ids))
            },
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
        Self::unsubscribe_batch(
            &mut self.subscriptions_instrument_status,
            &cmd.instrument_ids,
            "instrument status",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_instrument_status(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_instrument_close,
            &cmd.instrument_ids,
            "instrument close",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_instrument_close(cmd.with_instrument_ids(ids))
            },
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
        Self::unsubscribe_batch(
            &mut self.subscriptions_instrument_close,
            &cmd.instrument_ids,
            "instrument close",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_instrument_close(&cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Subscribes to option greeks for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_option_greeks(&mut self, cmd: &SubscribeOptionGreeks) -> anyhow::Result<()> {
        Self::subscribe_batch(
            &mut self.subscriptions_option_greeks,
            &cmd.instrument_ids,
            "option greeks",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .subscribe_option_greeks(cmd.with_instrument_ids(ids))
            },
        )
    }

    /// Unsubscribes from option greeks for an instrument, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        Self::unsubscribe_batch(
            &mut self.subscriptions_option_greeks,
            &cmd.instrument_ids,
            "option greeks",
            |partial| &partial.instrument_ids_accepted,
            |ids| {
                self.client
                    .unsubscribe_option_greeks(&cmd.with_instrument_ids(ids))
            },
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

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

//! Basic Databento subscriber actor implementation for quotes and trades.

use std::ops::{Deref, DerefMut};

use nautilus_common::actor::{DataActor, DataActorCore, data_actor::DataActorConfig};
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    identifiers::{ActorId, ClientId, InstrumentId},
};

/// Configuration for the Databento subscriber actor.
#[derive(Debug, Clone)]
pub struct DatabentoSubscriberActorConfig {
    /// Base data actor configuration.
    pub base: DataActorConfig,
    /// Client ID to use for subscriptions.
    pub client_id: ClientId,
    /// Instrument IDs to subscribe to.
    pub instrument_ids: Vec<InstrumentId>,
}

impl DatabentoSubscriberActorConfig {
    /// Creates a new [`DatabentoSubscriberActorConfig`] instance.
    #[must_use]
    pub fn new(client_id: ClientId, instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            base: DataActorConfig::default(),
            client_id,
            instrument_ids,
        }
    }
}

/// A basic Databento subscriber actor that subscribes to quotes and trades.
///
/// This actor demonstrates how to use the `DataActor` trait to subscribe to market data
/// from Databento for specified instruments. It logs received quotes and trades to
/// demonstrate the data flow.
#[derive(Debug)]
pub struct DatabentoSubscriberActor {
    core: DataActorCore,
    config: DatabentoSubscriberActorConfig,
    pub received_quotes: Vec<QuoteTick>,
    pub received_trades: Vec<TradeTick>,
}

impl Deref for DatabentoSubscriberActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for DatabentoSubscriberActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for DatabentoSubscriberActor {
    fn actor_id(&self) -> ActorId {
        self.core.actor_id()
    }

    fn core(&self) -> &DataActorCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut DataActorCore {
        &mut self.core
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Databento subscriber actor for {} instruments",
            self.config.instrument_ids.len()
        );

        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        // Subscribe to quotes and trades for each instrument
        for instrument_id in instrument_ids {
            log::info!("Subscribing to quotes for {instrument_id}");
            self.subscribe_quotes(instrument_id, Some(client_id), None);

            log::info!("Subscribing to trades for {instrument_id}");
            self.subscribe_trades(instrument_id, Some(client_id), None);
        }

        log::info!("Databento subscriber actor started successfully");
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Stopping Databento subscriber actor for {} instruments",
            self.config.instrument_ids.len()
        );

        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        // Unsubscribe to quotes and trades for each instrument
        for instrument_id in instrument_ids {
            log::info!("Unsubscribing from quotes for {instrument_id}");
            self.unsubscribe_quotes(instrument_id, Some(client_id), None);

            log::info!("Unsubscribing from trades for {instrument_id}");
            self.unsubscribe_trades(instrument_id, Some(client_id), None);
        }

        log::info!("Databento subscriber actor stopped successfully");
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        log::info!("Received quote: {quote:?}");
        self.received_quotes.push(*quote);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        log::info!("Received trade: {trade:?}");
        self.received_trades.push(*trade);
        Ok(())
    }
}

impl DatabentoSubscriberActor {
    /// Creates a new [`DatabentoSubscriberActor`] instance.
    #[must_use]
    pub fn new(config: DatabentoSubscriberActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone()),
            config,
            received_quotes: Vec::new(),
            received_trades: Vec::new(),
        }
    }

    /// Returns the number of quotes received by this actor.
    #[must_use]
    pub const fn quote_count(&self) -> usize {
        self.received_quotes.len()
    }

    /// Returns the number of trades received by this actor.
    #[must_use]
    pub const fn trade_count(&self) -> usize {
        self.received_trades.len()
    }
}

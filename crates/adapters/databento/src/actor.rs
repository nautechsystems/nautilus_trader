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

use std::{
    any::Any,
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use nautilus_common::{
    actor::{Actor, DataActor, DataActorCore, data_actor::DataActorConfig},
    cache::Cache,
    clock::Clock,
    component::Component,
    enums::{ComponentState, ComponentTrigger},
    timer::TimeEvent,
};
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    identifiers::{ClientId, ComponentId, InstrumentId},
};
use ustr::Ustr;

/// Configuration for the Databento subscriber actor.
#[derive(Debug, Clone)]
pub struct DatabentoSubscriberActorConfig {
    /// Base data actor configuration.
    pub base: DataActorConfig,
    /// Instrument IDs to subscribe to.
    pub instrument_ids: Vec<InstrumentId>,
    /// Client ID to use for subscriptions.
    pub client_id: ClientId,
}

impl DatabentoSubscriberActorConfig {
    /// Creates a new [`DatabentoSubscriberActorConfig`] instance.
    #[must_use]
    pub fn new(instrument_ids: Vec<InstrumentId>, client_id: ClientId) -> Self {
        Self {
            base: DataActorConfig::default(),
            instrument_ids,
            client_id,
        }
    }
}

/// A basic Databento subscriber actor that subscribes to quotes and trades.
///
/// This actor demonstrates how to use the DataActor trait to subscribe to market data
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

impl Actor for DatabentoSubscriberActor {
    fn id(&self) -> Ustr {
        self.core.actor_id.inner()
    }

    fn handle(&mut self, msg: &dyn Any) {
        // Let the core handle message routing
        self.core.handle(msg);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Component for DatabentoSubscriberActor {
    fn id(&self) -> ComponentId {
        ComponentId::from(self.core.actor_id.inner().as_str())
    }

    fn state(&self) -> ComponentState {
        self.core.state()
    }

    fn trigger(&self) -> ComponentTrigger {
        ComponentTrigger::Initialize
    }

    fn is_running(&self) -> bool {
        matches!(self.core.state(), ComponentState::Running)
    }

    fn is_stopped(&self) -> bool {
        matches!(self.core.state(), ComponentState::Stopped)
    }

    fn is_disposed(&self) -> bool {
        matches!(self.core.state(), ComponentState::Disposed)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.core.start()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.stop()
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.core.reset()
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.core.dispose()
    }

    fn handle_event(&mut self, _event: TimeEvent) {
        // No-op for now
    }
}

impl DataActor for DatabentoSubscriberActor {
    fn state(&self) -> ComponentState {
        self.core.state()
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Databento subscriber actor for {} instruments",
            self.config.instrument_ids.len()
        );

        // Clone config values to avoid borrowing issues
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        // Subscribe to quotes and trades for each instrument
        for instrument_id in instrument_ids {
            log::info!("Subscribing to quotes for {instrument_id}");
            self.subscribe_quotes::<DatabentoSubscriberActor>(instrument_id, Some(client_id), None);

            log::info!("Subscribing to trades for {instrument_id}");
            self.subscribe_trades::<DatabentoSubscriberActor>(instrument_id, Some(client_id), None);
        }

        log::info!("Databento subscriber actor started successfully");
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
    pub fn new(
        config: DatabentoSubscriberActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone(), cache, clock),
            config,
            received_quotes: Vec::new(),
            received_trades: Vec::new(),
        }
    }

    /// Returns the number of quotes received by this actor.
    #[must_use]
    pub fn quote_count(&self) -> usize {
        self.received_quotes.len()
    }

    /// Returns the number of trades received by this actor.
    #[must_use]
    pub fn trade_count(&self) -> usize {
        self.received_trades.len()
    }
}

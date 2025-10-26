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

use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    time::Duration,
};

use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    enums::{Environment, LogColor},
    log_info,
    timer::TimeEvent,
};
use nautilus_core::env::get_env_var;
use nautilus_databento::factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    identifiers::{ClientId, InstrumentId, TraderId},
};

// Run with `cargo run --bin databento-node-test --features high-precision`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "DATABENTO-TESTER-001".to_string();

    // Get Databento API key from environment
    let api_key = get_env_var("DATABENTO_API_KEY").unwrap_or_else(|_| {
        println!("WARNING: DATABENTO_API_KEY not found, using placeholder");
        "db-placeholder-key".to_string()
    });

    // Determine publishers file path
    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
    if !publishers_filepath.exists() {
        println!(
            "WARNING: Publishers file not found at: {}",
            publishers_filepath.display()
        );
    }

    // Configure Databento client
    let databento_config = DatabentoLiveClientConfig::new(
        api_key,
        publishers_filepath,
        true, // use_exchange_as_venue
        true, // bars_timestamp_on_close
    );

    let client_factory = DatabentoDataClientFactory::new();

    // Create and register a Databento subscriber actor
    let client_id = ClientId::new("DATABENTO");
    let instrument_ids = vec![
        InstrumentId::from("ESZ5.XCME"),
        // Add more instruments as needed
    ];

    // Build the live node with Databento data client
    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(None, Box::new(client_factory), Box::new(databento_config))?
        .build()?;

    let actor_config = DatabentoSubscriberActorConfig::new(client_id, instrument_ids);
    let actor = DatabentoSubscriberActor::new(actor_config);

    node.add_actor(actor)?;

    node.run().await?;

    Ok(())
}

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
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        for instrument_id in instrument_ids {
            self.subscribe_quotes(instrument_id, Some(client_id), None);
            self.subscribe_trades(instrument_id, Some(client_id), None);
        }

        self.clock().set_timer(
            "TEST-TIMER-1-SECOND",
            Duration::from_secs(1),
            None,
            None,
            None,
            Some(true),
            Some(false),
        )?;

        self.clock().set_timer(
            "TEST-TIMER-2-SECOND",
            Duration::from_secs(2),
            None,
            None,
            None,
            Some(true),
            Some(false),
        )?;

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        for instrument_id in instrument_ids {
            self.unsubscribe_quotes(instrument_id, Some(client_id), None);
            self.unsubscribe_trades(instrument_id, Some(client_id), None);
        }

        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        log_info!("Received {event:?}", color = LogColor::Blue);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        log_info!("Received {quote:?}", color = LogColor::Cyan);
        self.received_quotes.push(*quote);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        log_info!("Received {trade:?}", color = LogColor::Cyan);
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

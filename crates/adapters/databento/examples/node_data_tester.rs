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

//! Example demonstrating live data testing with the Databento adapter.
//!
//! Edit the constants below to change the target instrument and price precision.
//!
//! Run with: `cargo run --example databento-data-tester --package nautilus-databento`
//!
//! Required credential environment variables:
//! - `DATABENTO_API_KEY`.

use std::path::PathBuf;

use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    enums::{Environment, LogColor},
    log_info, nautilus_actor,
    timer::TimeEvent,
};
use nautilus_core::{Params, env::get_env_var};
use nautilus_databento::{
    common::DATABENTO_CLIENT_ID,
    factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    identifiers::{ClientId, InstrumentId, TraderId},
};
use serde_json::json;

const PRICE_PRECISION_PARAM: &str = "price_precision";

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "DATABENTO-TESTER-001";
const INSTRUMENT_ID: &str = "ESZ6.XCME";
const PRICE_PRECISION: Option<u8> = None;

// Alternative instrument with a price-precision override:
// const INSTRUMENT_ID: &str = "6EM6.XCME";
// const PRICE_PRECISION: Option<u8> = Some(5);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();

    let api_key = get_env_var("DATABENTO_API_KEY")?;

    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
    if !publishers_filepath.exists() {
        println!(
            "WARNING: Publishers file not found at: {}",
            publishers_filepath.display()
        );
    }

    let databento_config = DatabentoLiveClientConfig::new(
        api_key,
        publishers_filepath,
        true, // use_exchange_as_venue
        true, // bars_timestamp_on_close
    );

    let client_factory = DatabentoDataClientFactory::new();

    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let price_precision = PRICE_PRECISION;

    let client_id = *DATABENTO_CLIENT_ID;
    let instrument_ids = vec![instrument_id];

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(databento_config))?
        .build()?;

    let actor_config =
        DatabentoSubscriberActorConfig::new(client_id, instrument_ids, price_precision);
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
    /// Price precision override for subscribed instruments.
    pub price_precision: Option<u8>,
}

impl DatabentoSubscriberActorConfig {
    /// Creates a new [`DatabentoSubscriberActorConfig`] instance.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        instrument_ids: Vec<InstrumentId>,
        price_precision: Option<u8>,
    ) -> Self {
        Self {
            base: DataActorConfig::default(),
            client_id,
            instrument_ids,
            price_precision,
        }
    }

    fn subscription_params(&self) -> Option<Params> {
        self.price_precision.map(|price_precision| {
            let mut params = Params::new();
            params.insert(PRICE_PRECISION_PARAM.to_string(), json!(price_precision));
            params
        })
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

nautilus_actor!(DatabentoSubscriberActor);

impl DataActor for DatabentoSubscriberActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;
        let params = self.config.subscription_params();

        for instrument_id in instrument_ids {
            self.subscribe_quotes(instrument_id, Some(client_id), params.clone());
            self.subscribe_trades(instrument_id, Some(client_id), params.clone());
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        // Databento does not support granular unsubscribing
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        log_info!("{event:?}", color = LogColor::Blue);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        log_info!("{quote:?}", color = LogColor::Cyan);
        self.received_quotes.push(*quote);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        log_info!("{trade:?}", color = LogColor::Cyan);
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

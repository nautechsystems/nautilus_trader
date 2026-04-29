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

//! Example demonstrating dynamic instrument discovery and trade subscription.
//!
//! Uses an [`EventSlugFilter`] on the config so the provider only loads
//! "Presidential Election Winner 2028" instruments from the Gamma API (instead
//! of all 72K+ markets). A custom [`DataActor`] then reads the discovered
//! instruments from the cache and subscribes to live trades for each.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example polymarket-election-subscriber --package nautilus-polymarket --features examples
//! ```

use std::{collections::HashMap, sync::Arc};

use log::LevelFilter;
use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::Environment,
    logging::logger::LoggerConfig,
    nautilus_actor,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::TradeTick,
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::TransportBackend;
use nautilus_polymarket::{
    common::models::PolymarketLabel, config::PolymarketDataClientConfig,
    factories::PolymarketDataClientFactory, filters::EventSlugFilter,
};

// ---------------------------------------------------------------------------
// Custom DataActor: discovers instruments from cache and subscribes to trades
// ---------------------------------------------------------------------------

/// Configuration for the election trade subscriber actor.
#[derive(Debug, Clone)]
struct ElectionSubscriberConfig {
    base: DataActorConfig,
    client_id: ClientId,
}

/// A custom [`DataActor`] that reads all instruments loaded by the provider
/// (filtered via [`EventSlugFilter`] on the config) and subscribes to live
/// trade ticks for each.
#[derive(Debug)]
struct ElectionTradeSubscriber {
    core: DataActorCore,
    config: ElectionSubscriberConfig,
    subscribed: Vec<InstrumentId>,
    labels: HashMap<InstrumentId, PolymarketLabel>,
}

impl ElectionTradeSubscriber {
    fn new(config: ElectionSubscriberConfig) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone()),
            config,
            subscribed: Vec::new(),
            labels: HashMap::new(),
        }
    }
}

nautilus_actor!(ElectionTradeSubscriber);

impl DataActor for ElectionTradeSubscriber {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venue = Venue::from("POLYMARKET");
        let client_id = Some(self.config.client_id);

        // Read instruments from cache and build human-readable label index
        let cache = self.cache();
        let instruments: Vec<(InstrumentId, PolymarketLabel)> = cache
            .instruments(&venue, None)
            .iter()
            .map(|i| (i.id(), PolymarketLabel::from_instrument(i)))
            .collect();
        drop(cache); // Release borrow before calling subscribe methods

        log::info!(
            "Found {} instruments from filtered provider, subscribing to trades",
            instruments.len()
        );

        for (instrument_id, label) in &instruments {
            log::info!("  Subscribing: {label}");
            self.subscribe_trades(*instrument_id, client_id, None);
        }

        self.subscribed = instruments.iter().map(|(id, _)| *id).collect();
        self.labels = instruments.into_iter().collect();

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let client_id = Some(self.config.client_id);
        for instrument_id in self.subscribed.clone() {
            self.unsubscribe_trades(instrument_id, client_id, None);
        }
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let label = self
            .labels
            .entry(instrument.id())
            .or_insert_with(|| PolymarketLabel::from_instrument(instrument));
        log::info!("Instrument update: {label}");
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        let label = self
            .labels
            .get(&trade.instrument_id)
            .map_or_else(|| trade.instrument_id.to_string(), |l| l.to_string());
        log::info!(
            "{label} | {side:?} {size} @ {price}",
            side = trade.aggressor_side,
            size = trade.size,
            price = trade.price,
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Main: wire up LiveNode with filtered factory + custom actor
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let client_id = ClientId::new("POLYMARKET");

    let event_filter =
        EventSlugFilter::from_slugs(vec!["presidential-election-winner-2028".to_string()]);

    let client_factory = PolymarketDataClientFactory;

    let polymarket_config = PolymarketDataClientConfig {
        filters: vec![Arc::new(event_filter)],
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("POLYMARKET-ELECTION-SUB-001".to_string())
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(polymarket_config))?
        .build()?;

    let actor_config = ElectionSubscriberConfig {
        base: DataActorConfig::default(),
        client_id,
    };
    let actor = ElectionTradeSubscriber::new(actor_config);

    node.add_actor(actor)?;
    node.run().await?;

    Ok(())
}

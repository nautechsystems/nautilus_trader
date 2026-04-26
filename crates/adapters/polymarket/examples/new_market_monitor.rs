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

//! Example demonstrating new market monitoring with instrument subscriptions.
//!
//! Configures the Polymarket data client with `subscribe_new_markets: true` so
//! the WebSocket connection receives `new_market` events. A [`SearchFilter`]
//! pre-populates BTC markets from the Gamma search API at startup — these
//! serve as the initial instrument set. Any new market created on Polymarket
//! is then fetched from the Gamma API and emitted alongside the initial BTC
//! instruments. A custom [`DataActor`] subscribes to all instruments from the
//! POLYMARKET venue and logs every instrument that arrives — including newly
//! created markets pushed in real time.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example polymarket-new-market-monitor --package nautilus-polymarket --features examples
//! ```

use std::sync::Arc;

use log::LevelFilter;
use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::Environment,
    logging::logger::LoggerConfig,
    nautilus_actor,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, TraderId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::TransportBackend;
use nautilus_polymarket::{
    common::{enums::SignatureType, models::PolymarketLabel},
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
    filters::SearchFilter,
};

#[derive(Debug, Clone)]
struct NewMarketMonitorConfig {
    base: DataActorConfig,
    client_id: ClientId,
}

#[derive(Debug)]
struct NewMarketMonitor {
    core: DataActorCore,
    config: NewMarketMonitorConfig,
    instrument_count: usize,
}

impl NewMarketMonitor {
    fn new(config: NewMarketMonitorConfig) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone()),
            config,
            instrument_count: 0,
        }
    }
}

nautilus_actor!(NewMarketMonitor);

impl DataActor for NewMarketMonitor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venue = Venue::from("POLYMARKET");
        let client_id = Some(self.config.client_id);

        // Log instruments already in cache from the initial provider load
        let cache = self.cache();
        let cached_instruments: Vec<_> = cache
            .instruments(&venue, None)
            .iter()
            .map(|i| (i.id(), PolymarketLabel::from_instrument(i)))
            .collect();
        drop(cache);

        log::info!(
            "Initial provider load: {} instruments in cache",
            cached_instruments.len()
        );

        self.instrument_count = cached_instruments.len();

        // Subscribe to all instruments from the venue — this will deliver
        // both existing and any new instruments pushed by the data client
        // when subscribe_new_markets is enabled.
        self.subscribe_instruments(venue, client_id, None);

        log::info!("Subscribed to POLYMARKET instruments, waiting for new markets...");

        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        self.instrument_count += 1;
        let label = PolymarketLabel::from_instrument(instrument);
        log::info!(
            "Instrument received (total={}): {} — {label} | tick_size={} price_prec={} size_prec={}",
            self.instrument_count,
            instrument.id(),
            instrument.price_increment(),
            instrument.price_precision(),
            instrument.size_precision(),
        );
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = ClientId::new("POLYMARKET");

    // SearchFilter pre-populates BTC markets as the initial instrument set
    let search_filter = SearchFilter::from_query("BTC");

    let data_config = PolymarketDataClientConfig {
        subscribe_new_markets: true,
        filters: vec![Arc::new(search_filter)],
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = PolymarketExecClientConfig {
        trader_id,
        account_id,
        signature_type: SignatureType::PolyGnosisSafe,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("POLYMARKET-NEW-MARKET-MONITOR-001".to_string())
        .with_logging(log_config)
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(120)
        .with_timeout_reconciliation(60)
        .with_delay_post_stop_secs(2)
        .add_data_client(
            None,
            Box::new(PolymarketDataClientFactory),
            Box::new(data_config),
        )?
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(exec_config),
        )?
        .build()?;

    let actor_config = NewMarketMonitorConfig {
        base: DataActorConfig::default(),
        client_id,
    };
    let actor = NewMarketMonitor::new(actor_config);

    node.add_actor(actor)?;
    node.run().await?;

    Ok(())
}

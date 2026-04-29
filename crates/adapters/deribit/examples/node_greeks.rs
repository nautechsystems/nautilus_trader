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

//! Example demonstrating live option greeks subscription with the Deribit adapter.
//!
//! On start, this actor:
//! 1. Queries the cache for all BTC option instruments
//! 2. Finds the nearest expiry
//! 3. Filters for CALL options at that expiry
//! 4. Subscribes to OptionGreeks for each one
//! 5. Logs received greeks in the `on_option_greeks` handler
//!
//! Run with: `cargo run --example deribit-greeks-tester --package nautilus-deribit --features examples`

use std::fmt::Debug;

use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::Environment,
    nautilus_actor,
    timer::TimeEvent,
};
use nautilus_deribit::{
    common::enums::DeribitEnvironment, config::DeribitDataClientConfig,
    factories::DeribitDataClientFactory, http::models::DeribitProductType,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::option_chain::OptionGreeks,
    enums::OptionKind,
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
    instruments::Instrument,
    stubs::TestDefault,
};
use nautilus_network::websocket::TransportBackend;
use ustr::Ustr;

// ---------------------------------------------------------------------------
// GreeksTester actor
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct GreeksTester {
    core: DataActorCore,
    client_id: ClientId,
    subscribed_instruments: Vec<InstrumentId>,
}

nautilus_actor!(GreeksTester);

impl GreeksTester {
    fn new(client_id: ClientId) -> Self {
        Self {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some("GREEKS_TESTER-001".into()),
                ..Default::default()
            }),
            client_id,
            subscribed_instruments: Vec::new(),
        }
    }
}

impl DataActor for GreeksTester {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venue = Venue::new("DERIBIT");
        let underlying_filter = Ustr::from("BTC");

        // Collect option instrument data from cache (owned copies to release borrow)
        // Each entry: (instrument_id, strike_f64, expiry_ns)
        let mut options: Vec<(InstrumentId, f64, u64)> = {
            let cache = self.cache();
            let instruments = cache.instruments(&venue, Some(&underlying_filter));

            instruments
                .iter()
                .filter_map(|inst| {
                    if inst.option_kind() == Some(OptionKind::Call) {
                        let expiry = inst.expiration_ns()?.as_u64();
                        let strike = inst.strike_price()?.as_f64();
                        Some((inst.id(), strike, expiry))
                    } else {
                        None
                    }
                })
                .collect()
        }; // cache borrow dropped here

        // Discard already-expired options
        let now_ns = self.timestamp_ns().as_u64();
        options.retain(|(_, _, exp)| *exp > now_ns);

        if options.is_empty() {
            log::warn!("No BTC CALL options found in cache (all expired)");
            return Ok(());
        }

        // Find the nearest (soonest) non-expired expiry
        let nearest_expiry = options.iter().map(|(_, _, exp)| *exp).min().unwrap();

        // Filter to only instruments at that expiry, sort by strike
        options.retain(|(_, _, exp)| *exp == nearest_expiry);
        options.sort_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap());

        log::info!(
            "Found {} BTC CALL options at nearest expiry (ts={})",
            options.len(),
            nearest_expiry,
        );

        for (id, strike, expiry) in &options {
            log::info!("  {id} strike={strike} expiry={expiry}");
        }

        // Subscribe to option greeks for each instrument
        let client_id = self.client_id;
        for (instrument_id, _, _) in &options {
            self.subscribe_option_greeks(*instrument_id, Some(client_id), None);
            self.subscribed_instruments.push(*instrument_id);
        }

        log::info!(
            "Subscribed to option greeks for {} instruments",
            self.subscribed_instruments.len(),
        );

        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        log::info!(
            "GREEKS | {} | delta={:.4} gamma={:.6} vega={:.4} theta={:.4} rho={:.6} | \
             mark_iv={} bid_iv={} ask_iv={} | \
             underlying={} oi={}",
            greeks.instrument_id,
            greeks.delta,
            greeks.gamma,
            greeks.vega,
            greeks.theta,
            greeks.rho,
            greeks
                .mark_iv
                .map_or("-".to_string(), |v| format!("{v:.2}")),
            greeks.bid_iv.map_or("-".to_string(), |v| format!("{v:.2}")),
            greeks.ask_iv.map_or("-".to_string(), |v| format!("{v:.2}")),
            greeks
                .underlying_price
                .map_or("-".to_string(), |v| format!("{v:.2}")),
            greeks
                .open_interest
                .map_or("-".to_string(), |v| format!("{v:.1}")),
        );
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let ids: Vec<InstrumentId> = self.subscribed_instruments.drain(..).collect();
        let client_id = self.client_id;
        for instrument_id in ids {
            self.unsubscribe_option_greeks(instrument_id, Some(client_id), None);
        }
        log::info!("Unsubscribed from all option greeks");
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let client_id = ClientId::new("DERIBIT");

    let deribit_config = DeribitDataClientConfig {
        api_key: None,    // Will use 'DERIBIT_API_KEY' env var
        api_secret: None, // Will use 'DERIBIT_API_SECRET' env var
        product_types: vec![DeribitProductType::Option],
        environment: DeribitEnvironment::Mainnet,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let client_factory = DeribitDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("DERIBIT-GREEKS-TESTER-001".to_string())
        .add_data_client(None, Box::new(client_factory), Box::new(deribit_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester = GreeksTester::new(client_id);
    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

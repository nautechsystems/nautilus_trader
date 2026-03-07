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

//! Example demonstrating live option chain subscription with the Deribit adapter.
//!
//! On start, this actor:
//! 1. Queries the cache for all BTC option instruments
//! 2. Finds the nearest expiry
//! 3. Builds an `OptionSeriesId` for that expiry
//! 4. Subscribes to an option chain with 5 strikes above and 5 below ATM
//! 5. Uses the BTC index price as the ATM source
//! 6. Logs received `OptionChainSlice` snapshots in the `on_option_chain` handler
//!
//! Run with: `cargo run --example deribit-option-chain-tester --package nautilus-deribit`

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::Environment,
    timer::TimeEvent,
};
use nautilus_deribit::{
    config::DeribitDataClientConfig, factories::DeribitDataClientFactory,
    http::models::DeribitProductType,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::option_chain::{OptionChainSlice, StrikeRange},
    identifiers::{ClientId, InstrumentId, OptionSeriesId, TraderId, Venue},
    instruments::{Instrument, any::InstrumentAny},
    stubs::TestDefault,
};
use ustr::Ustr;

// ---------------------------------------------------------------------------
// OptionChainTester actor
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct OptionChainTester {
    core: DataActorCore,
    client_id: ClientId,
    series_id: Option<OptionSeriesId>,
}

impl Deref for OptionChainTester {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for OptionChainTester {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl OptionChainTester {
    fn new(client_id: ClientId) -> Self {
        Self {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some("OPTION_CHAIN_TESTER-001".into()),
                ..Default::default()
            }),
            client_id,
            series_id: None,
        }
    }
}

impl DataActor for OptionChainTester {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venue = Venue::new("DERIBIT");
        let underlying_filter = Ustr::from("BTC");

        // Collect option instrument data from cache (owned copies to release borrow)
        // Each entry: (instrument_id, underlying, settlement_currency, expiry_ns)
        // Filter out already-expired options.
        let now_ns = self.clock().timestamp_ns().as_u64();
        let options: Vec<(InstrumentId, Ustr, Ustr, u64)> = {
            let cache = self.cache();
            let instruments = cache.instruments(&venue, Some(&underlying_filter));

            instruments
                .iter()
                .filter_map(|inst| {
                    // Only consider CryptoOption instruments
                    if let InstrumentAny::CryptoOption(opt) = inst {
                        let expiry = inst.expiration_ns()?.as_u64();
                        if expiry <= now_ns {
                            return None;
                        }
                        Some((
                            inst.id(),
                            underlying_filter,
                            opt.settlement_currency.code,
                            expiry,
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        }; // cache borrow dropped here

        if options.is_empty() {
            log::warn!("No BTC options found in cache");
            return Ok(());
        }

        // Find the nearest (soonest) future expiry
        let nearest_expiry = options.iter().map(|(_, _, _, exp)| *exp).min().unwrap();

        // Find settlement currency for nearest expiry (use BTC-settled by default)
        let settlement_currency = options
            .iter()
            .find(|(_, _, settlement, exp)| *exp == nearest_expiry && settlement.as_str() == "BTC")
            .map_or_else(
                || {
                    options
                        .iter()
                        .find(|(_, _, _, exp)| *exp == nearest_expiry)
                        .unwrap()
                        .2
                },
                |(_, _, s, _)| *s,
            );

        // Count how many options at nearest expiry with matching settlement
        let count = options
            .iter()
            .filter(|(_, _, s, exp)| *exp == nearest_expiry && *s == settlement_currency)
            .count();

        log::info!(
            "Found {count} BTC options at nearest expiry (ts={nearest_expiry}, settlement={settlement_currency})"
        );

        // Build OptionSeriesId for the nearest expiry
        let series_id = OptionSeriesId::new(
            venue,
            underlying_filter,
            settlement_currency,
            nautilus_core::UnixNanos::from(nearest_expiry),
        );

        log::info!("Subscribing to option chain: {series_id}");
        let strike_range = StrikeRange::AtmRelative {
            strikes_above: 3,
            strikes_below: 3,
        };

        // Snapshot every 2 seconds (use None for raw stream mode)
        let snapshot_interval_ms = Some(2_000u64);

        let client_id = self.client_id;
        self.subscribe_option_chain(
            series_id,
            strike_range,
            snapshot_interval_ms,
            Some(client_id),
        );

        self.series_id = Some(series_id);

        Ok(())
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        log::info!(
            "OPTION_CHAIN | {} | atm={} | calls={} puts={} | strikes={}",
            slice.series_id,
            slice.atm_strike.map_or("-".to_string(), |p| format!("{p}")),
            slice.call_count(),
            slice.put_count(),
            slice.strike_count(),
        );

        // Log each strike with call/put quotes and greeks
        for strike in slice.strikes() {
            let call_info = slice.get_call(&strike).map(|d| {
                let greeks_str = d.greeks.as_ref().map_or("-".to_string(), |g| {
                    format!(
                        "d={:.3} g={:.5} v={:.2} iv={:.1}%",
                        g.delta,
                        g.gamma,
                        g.vega,
                        g.mark_iv.unwrap_or(0.0) * 100.0
                    )
                });
                format!(
                    "bid={} ask={} [{}]",
                    d.quote.bid_price, d.quote.ask_price, greeks_str
                )
            });

            let put_info = slice.get_put(&strike).map(|d| {
                let greeks_str = d.greeks.as_ref().map_or("-".to_string(), |g| {
                    format!(
                        "d={:.3} g={:.5} v={:.2} iv={:.1}%",
                        g.delta,
                        g.gamma,
                        g.vega,
                        g.mark_iv.unwrap_or(0.0) * 100.0
                    )
                });
                format!(
                    "bid={} ask={} [{}]",
                    d.quote.bid_price, d.quote.ask_price, greeks_str
                )
            });

            log::info!(
                "  K={} | CALL: {} | PUT: {}",
                strike,
                call_info.unwrap_or_else(|| "-".to_string()),
                put_info.unwrap_or_else(|| "-".to_string()),
            );
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if let Some(series_id) = self.series_id.take() {
            let client_id = self.client_id;
            self.unsubscribe_option_chain(series_id, Some(client_id));
            log::info!("Unsubscribed from option chain {series_id}");
        }
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
        product_types: vec![DeribitProductType::Option, DeribitProductType::Future],
        use_testnet: false,
        ..Default::default()
    };

    let client_factory = DeribitDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("DERIBIT-OPTION-CHAIN-TESTER-001".to_string())
        .add_data_client(None, Box::new(client_factory), Box::new(deribit_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester = OptionChainTester::new(client_id);
    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

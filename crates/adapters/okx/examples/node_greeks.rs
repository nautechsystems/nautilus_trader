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

//! Example demonstrating live option greeks subscription with the OKX adapter.
//!
//! On start, this actor:
//! 1. Queries the cache for all BTC option instruments
//! 2. Finds the nearest expiry
//! 3. Filters for CALL options at that expiry
//! 4. Subscribes to OptionGreeks for each one, alternating three param shapes:
//!    the first third with no params (defaults to both conventions), the second
//!    third narrowed to Black-Scholes only, and the final third narrowed to
//!    price-adjusted only.
//! 5. Logs received greeks (including the emitted `convention`) in the
//!    `on_option_greeks` handler so the downstream branch on `greeks.convention`
//!    is visible.
//!
//! Run with: `cargo run --example okx-greeks-tester --package nautilus-okx --features examples`

use std::fmt::Debug;

use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::Environment,
    nautilus_actor,
    timer::TimeEvent,
};
use nautilus_core::Params;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::option_chain::OptionGreeks,
    enums::{GreeksConvention, OptionKind},
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
    instruments::Instrument,
    stubs::TestDefault,
};
use nautilus_okx::{
    common::enums::OKXInstrumentType, config::OKXDataClientConfig, factories::OKXDataClientFactory,
};
use serde_json::json;
use ustr::Ustr;

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
        let venue = Venue::new("OKX");
        let underlying_filter = Ustr::from("BTC");

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
        };

        let now_ns = self.timestamp_ns().as_u64();
        options.retain(|(_, _, exp)| *exp > now_ns);

        if options.is_empty() {
            log::warn!("No BTC CALL options found in cache (all expired)");
            return Ok(());
        }

        let nearest_expiry = options.iter().map(|(_, _, exp)| *exp).min().unwrap();

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

        let client_id = self.client_id;
        let third = options.len() / 3;
        let (default_slice, rest) = options.split_at(third);
        let (bs_slice, pa_slice) = rest.split_at(rest.len() / 2);

        for (instrument_id, _, _) in default_slice {
            self.subscribe_option_greeks(*instrument_id, Some(client_id), None);
            self.subscribed_instruments.push(*instrument_id);
        }

        let bs_only = GreeksConvention::BlackScholes.to_string();

        for (instrument_id, _, _) in bs_slice {
            let mut params = Params::new();
            params.insert("greeks_convention".to_string(), json!(bs_only));
            self.subscribe_option_greeks(*instrument_id, Some(client_id), Some(params));
            self.subscribed_instruments.push(*instrument_id);
        }

        let pa_only = GreeksConvention::PriceAdjusted.to_string();

        for (instrument_id, _, _) in pa_slice {
            let mut params = Params::new();
            params.insert("greeks_convention".to_string(), json!(pa_only));
            self.subscribe_option_greeks(*instrument_id, Some(client_id), Some(params));
            self.subscribed_instruments.push(*instrument_id);
        }

        log::info!(
            "Subscribed to option greeks for {} instruments ({} default both, {} BS, {} PA)",
            self.subscribed_instruments.len(),
            default_slice.len(),
            bs_slice.len(),
            pa_slice.len(),
        );

        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        log::info!(
            "GREEKS | {} | convention={} | delta={:.4} gamma={:.6} vega={:.4} theta={:.4} | \
             mark_iv={} bid_iv={} ask_iv={} | \
             underlying={} oi={}",
            greeks.instrument_id,
            greeks.convention,
            greeks.delta,
            greeks.gamma,
            greeks.vega,
            greeks.theta,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let client_id = ClientId::new("OKX");

    let okx_config = OKXDataClientConfig {
        api_key: None,        // Will use 'OKX_API_KEY' env var
        api_secret: None,     // Will use 'OKX_API_SECRET' env var
        api_passphrase: None, // Will use 'OKX_API_PASSPHRASE' env var
        instrument_types: vec![OKXInstrumentType::Option],
        instrument_families: Some(vec!["BTC-USD".to_string()]),
        ..Default::default()
    };

    let client_factory = OKXDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("OKX-GREEKS-TESTER-001".to_string())
        .add_data_client(None, Box::new(client_factory), Box::new(okx_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester = GreeksTester::new(client_id);
    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

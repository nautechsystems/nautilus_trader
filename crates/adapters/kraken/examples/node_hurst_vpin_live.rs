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

//! Example running the Hurst/VPIN directional strategy live against Kraken
//! Futures (`PF_XBTUSD`).
//!
//! Run with: `cargo run -p nautilus-kraken --features examples --example kraken-hurst-vpin-live`
//!
//! Environment variables:
//! - KRAKEN_FUTURES_API_KEY: Your Kraken Futures API key
//! - KRAKEN_FUTURES_API_SECRET: Your Kraken Futures API secret
//!
//! Point at [demo-futures.kraken.com](https://demo-futures.kraken.com) for
//! paper trading by setting `demo=true` on `resolve_futures` or by using a
//! demo key pair.

use nautilus_common::enums::Environment;
use nautilus_kraken::{
    common::{credential::KrakenCredential, enums::KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    factories::{KrakenDataClientFactory, KrakenExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, ClientId, InstrumentId, TraderId},
    types::Quantity,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_trading::examples::strategies::{HurstVpinDirectional, HurstVpinDirectionalConfig};

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let product_type = KrakenProductType::Futures;
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let bar_type = BarType::from("PF_XBTUSD.KRAKEN-2000000-VALUE-LAST-INTERNAL");

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("KRAKEN-001");
    let node_name = "KRAKEN-HURST-VPIN-001".to_string();
    let client_id = ClientId::new("KRAKEN");

    let credential = KrakenCredential::resolve_futures(None, None, false)
        .ok_or("API credentials required (set KRAKEN_FUTURES_API_KEY/KRAKEN_FUTURES_API_SECRET)")?;
    let (api_key, api_secret) = credential.into_parts();

    let data_config = KrakenDataClientConfig {
        api_key: Some(api_key.clone()),
        api_secret: Some(api_secret.clone()),
        product_type,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = KrakenExecClientConfig {
        trader_id,
        account_id,
        api_key,
        api_secret,
        product_type,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let data_factory = KrakenDataClientFactory::new();
    let exec_factory = KrakenExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let config = HurstVpinDirectionalConfig::new(instrument_id, bar_type, Quantity::from("0.0100"))
        .with_max_holding_secs(1800);
    let strategy = HurstVpinDirectional::new(config);

    node.add_strategy(strategy)?;
    let _ = client_id;
    node.run().await?;

    Ok(())
}

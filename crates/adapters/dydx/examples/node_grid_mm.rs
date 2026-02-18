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

//! Example demonstrating the GridMarketMaker strategy on dYdX v4.
//!
//! Prerequisites:
//! - Set `DYDX_PRIVATE_KEY` (or `DYDX_TESTNET_PRIVATE_KEY` for testnet)
//! - Optionally set `DYDX_WALLET_ADDRESS` (derived from private key if not set)
//!
//! Run with: `cargo run --example dydx-grid-mm --package nautilus-dydx`

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_dydx::{
    common::enums::DydxNetwork,
    config::{DYDXExecClientConfig, DydxDataClientConfig},
    factories::{DydxDataClientFactory, DydxExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, TraderId},
    types::Quantity,
};
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};

fn get_env_option(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Configuration
    let is_testnet = false;
    let network = if is_testnet {
        DydxNetwork::Testnet
    } else {
        DydxNetwork::Mainnet
    };

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DYDX-001");
    let node_name = "DYDX-GRID-MM-001".to_string();
    let instrument_id = InstrumentId::from("ETH-USD-PERP.DYDX");

    // Load credentials from environment
    let private_key_env = if is_testnet {
        "DYDX_TESTNET_PRIVATE_KEY"
    } else {
        "DYDX_PRIVATE_KEY"
    };
    let private_key = get_env_option(private_key_env);
    let wallet_address = get_env_option("DYDX_WALLET_ADDRESS");

    if private_key.is_none() && wallet_address.is_none() {
        return Err(
            format!("Set {private_key_env} or DYDX_WALLET_ADDRESS environment variable").into(),
        );
    }

    let data_config = DydxDataClientConfig {
        is_testnet,
        ..Default::default()
    };

    let exec_config = DYDXExecClientConfig {
        trader_id,
        account_id,
        network,
        private_key,
        wallet_address,
        subaccount_number: 0,
        grpc_endpoint: None,
        grpc_urls: vec![],
        ws_endpoint: None,
        http_endpoint: None,
        authenticator_ids: vec![],
        http_timeout_secs: Some(30),
        max_retries: Some(3),
        retry_delay_initial_ms: Some(1000),
        retry_delay_max_ms: Some(10000),
    };

    let data_factory = DydxDataClientFactory::new();
    let exec_factory = DydxExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(false)
        .with_delay_post_stop_secs(5)
        .build()?;

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("0.10"))
        .with_num_levels(3)
        .with_grid_step_bps(100)
        .with_skew_factor(0.5)
        .with_requote_threshold_bps(10)
        .with_expire_time_secs(10)
        .with_on_cancel_resubmit(true);
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

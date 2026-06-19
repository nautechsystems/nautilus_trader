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
//! Edit the constants below to change the network, target instrument, and grid
//! market-making parameters.
//!
//! Run with: `cargo run --example dydx-grid-mm --package nautilus-dydx --features examples`
//!
//! Required credential environment variables:
//! - `DYDX_PRIVATE_KEY` (or `DYDX_TESTNET_PRIVATE_KEY` for testnet).
//! - `DYDX_WALLET_ADDRESS` (or `DYDX_TESTNET_WALLET_ADDRESS` for testnet; optional,
//!   derived from the private key if not set).

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_dydx::{
    common::enums::DydxNetwork,
    config::{DydxDataClientConfig, DydxExecClientConfig},
    factories::{DydxDataClientFactory, DydxExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, TraderId},
    types::Quantity,
};
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};

const DYDX_NETWORK: DydxNetwork = DydxNetwork::Mainnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "DYDX-001";
const NODE_NAME: &str = "DYDX-GRID-MM-001";
const INSTRUMENT_ID: &str = "ETH-USD-PERP.DYDX";

const MAX_POSITION: &str = "0.10";
const NUM_LEVELS: usize = 3;
const GRID_STEP_BPS: u32 = 100;
const SKEW_FACTOR: f64 = 0.5;
const REQUOTE_THRESHOLD_BPS: u32 = 10;
const EXPIRE_TIME_SECS: u64 = 8;
const ON_CANCEL_RESUBMIT: bool = true;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let network = DYDX_NETWORK;

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = DydxDataClientConfig {
        network,
        ..Default::default()
    };

    let exec_config = DydxExecClientConfig {
        trader_id,
        account_id,
        network,
        ..Default::default()
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

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from(MAX_POSITION))
        .with_num_levels(NUM_LEVELS)
        .with_grid_step_bps(GRID_STEP_BPS)
        .with_skew_factor(SKEW_FACTOR)
        .with_requote_threshold_bps(REQUOTE_THRESHOLD_BPS)
        .with_expire_time_secs(EXPIRE_TIME_SECS)
        .with_on_cancel_resubmit(ON_CANCEL_RESUBMIT);
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

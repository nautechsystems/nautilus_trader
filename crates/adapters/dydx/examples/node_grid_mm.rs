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
//! - Optionally set `DYDX_WALLET_ADDRESS` (or `DYDX_TESTNET_WALLET_ADDRESS` for testnet)
//!
//! Run with: `cargo run --example dydx-grid-mm --package nautilus-dydx`

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

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

    let data_config = DydxDataClientConfig {
        is_testnet,
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

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("0.10"))
        .with_num_levels(3)
        .with_grid_step_bps(100)
        .with_skew_factor(0.5)
        .with_requote_threshold_bps(10)
        .with_expire_time_secs(8)
        .with_on_cancel_resubmit(true);
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

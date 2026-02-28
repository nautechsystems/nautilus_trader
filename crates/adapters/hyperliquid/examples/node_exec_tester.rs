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

//! Example demonstrating live execution testing with the Hyperliquid adapter.
//!
//! Prerequisites:
//! - Set `HYPERLIQUID_PK` (or `HYPERLIQUID_TESTNET_PK` for testnet)
//!
//! Run with: `cargo run --example hyperliquid-exec-tester --package nautilus-hyperliquid`

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory, HyperliquidExecClientConfig,
    HyperliquidExecFactoryConfig, HyperliquidExecutionClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let is_testnet = false;

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let node_name = "HYPERLIQUID-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("HYPERLIQUID");
    let instrument_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");

    let data_config = HyperliquidDataClientConfig {
        is_testnet,
        ..Default::default()
    };

    let exec_config = HyperliquidExecFactoryConfig {
        trader_id,
        account_id,
        config: HyperliquidExecClientConfig {
            is_testnet,
            ..Default::default()
        },
    };

    let data_factory = HyperliquidDataClientFactory::new();
    let exec_factory = HyperliquidExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    let order_qty = Quantity::from("0.01"); // Minimum order size for ETH-USD-PERP

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        order_qty,
    )
    .with_log_data(false)
    .with_open_position_on_start(order_qty.as_decimal())
    .with_use_post_only(true)
    .with_cancel_orders_on_stop(true)
    .with_close_positions_on_stop(true);

    tester_config.base.external_order_claims = Some(vec![instrument_id]);

    // Use UUIDs for unique client order IDs across restarts
    tester_config.base.use_uuid_client_order_ids = true;
    // Hyperliquid supports hyphens in client order IDs (they're hashed to cloid)
    tester_config.base.use_hyphens_in_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

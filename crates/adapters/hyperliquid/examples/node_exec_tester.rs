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
//! Run with: `cargo run --example hyperliquid-exec-tester --package nautilus-hyperliquid --features examples`

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory, HyperliquidExecClientConfig,
    HyperliquidExecFactoryConfig, HyperliquidExecutionClientFactory,
    common::enums::HyperliquidEnvironment,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let hl_environment = HyperliquidEnvironment::Mainnet;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let node_name = "HYPERLIQUID-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("HYPERLIQUID");
    let instrument_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");

    let data_config = HyperliquidDataClientConfig {
        environment: hl_environment,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = HyperliquidExecFactoryConfig {
        trader_id,
        account_id,
        config: HyperliquidExecClientConfig {
            environment: hl_environment,
            transport_backend: TransportBackend::Sockudo,
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
        .with_delay_post_stop_secs(10)
        .build()?;

    let order_qty = Quantity::from("0.01"); // Minimum order size for ETH-USD-PERP

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            // Hyperliquid supports hyphens in client order IDs (they're hashed to cloid)
            use_hyphens_in_client_order_ids: true,
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .log_data(false)
        .open_position_on_start_qty(order_qty.as_decimal())
        .use_post_only(true)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

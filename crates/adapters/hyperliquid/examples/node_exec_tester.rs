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
//! Edit the constants below to change the environment, target instrument, and order size.
//!
//! Run with: `cargo run --example hyperliquid-exec-tester --package nautilus-hyperliquid --features examples`
//!
//! Required credential environment variables:
//! - `HYPERLIQUID_PK` (or `HYPERLIQUID_TESTNET_PK` for testnet).

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory, HyperliquidExecClientConfig,
    HyperliquidExecFactoryConfig, HyperliquidExecutionClientFactory,
    common::{consts::HYPERLIQUID_CLIENT_ID, enums::HyperliquidEnvironment},
};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

const HYPERLIQUID_ENVIRONMENT: HyperliquidEnvironment = HyperliquidEnvironment::Mainnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "HYPERLIQUID-001";
const NODE_NAME: &str = "HYPERLIQUID-EXEC-TESTER-001";
const STRATEGY_ID: &str = "EXEC_TESTER-001";
const INSTRUMENT_ID: &str = "ETH-USD-PERP.HYPERLIQUID";
const ORDER_QTY: &str = "0.01"; // Minimum order size for ETH-USD-PERP

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let nt_environment = Environment::Live;
    let hl_environment = HYPERLIQUID_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *HYPERLIQUID_CLIENT_ID;
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = HyperliquidDataClientConfig {
        environment: hl_environment,
        ..Default::default()
    };

    let exec_config = HyperliquidExecFactoryConfig {
        trader_id,
        account_id,
        config: HyperliquidExecClientConfig {
            environment: hl_environment,
            ..Default::default()
        },
    };

    let data_factory = HyperliquidDataClientFactory::new();
    let exec_factory = HyperliquidExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, nt_environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_exec_engine_config(exec_engine_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(10)
        .build()?;

    let order_qty = Quantity::from(ORDER_QTY);

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_claims: Some(vec![instrument_id]),
            // Hyperliquid supports hyphens in client order IDs.
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

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

//! Example demonstrating live execution testing with the Lighter adapter.
//!
//! Edit the constants below to change the environment, target instrument, and order size.
//!
//! Run with: `cargo run --example lighter-exec-tester --package nautilus-lighter --features examples`
//!
//! Required credential environment variables:
//! - `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`, and `LIGHTER_API_SECRET` when
//!   `LIGHTER_ENVIRONMENT` is `LighterEnvironment::Mainnet`.
//! - `LIGHTER_TESTNET_ACCOUNT_INDEX`, `LIGHTER_TESTNET_API_KEY_INDEX`, and
//!   `LIGHTER_TESTNET_API_SECRET` when `LIGHTER_ENVIRONMENT` is `LighterEnvironment::Testnet`.

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_lighter::{
    common::enums::LighterEnvironment,
    config::{LighterDataClientConfig, LighterExecClientConfig},
    factories::{LighterDataClientFactory, LighterExecutionClientFactory},
};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

const LIGHTER_ENVIRONMENT: LighterEnvironment = LighterEnvironment::Mainnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "LIGHTER-001";
const NODE_NAME: &str = "LIGHTER-EXEC-TESTER-001";
const CLIENT_ID: &str = "LIGHTER";
const STRATEGY_ID: &str = "EXEC_TESTER-001";
const INSTRUMENT_ID: &str = "ETH-PERP.LIGHTER";
const ORDER_QTY: &str = "0.01";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let lighter_environment = LIGHTER_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = ClientId::new(CLIENT_ID);
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = LighterDataClientConfig {
        environment: lighter_environment,
        ..Default::default()
    };
    let exec_config = LighterExecClientConfig::builder()
        .trader_id(trader_id)
        .account_id(account_id)
        .environment(lighter_environment)
        .build();

    let data_factory = LighterDataClientFactory::new();
    let exec_factory = LighterExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        // Example client order ID filtering for historical rows
        filtered_client_order_ids: Some(vec![
            "1793664468".to_string(),
            "1062637805503".to_string(),
        ]),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_exec_engine_config(exec_engine_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    let order_qty = Quantity::from(ORDER_QTY);

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .subscribe_quotes(true)
        .subscribe_trades(false)
        .subscribe_book(false)
        .enable_limit_buys(true)
        .enable_limit_sells(true)
        .enable_stop_buys(false)
        .enable_stop_sells(false)
        // .open_position_on_start_qty(order_qty.as_decimal())
        .tob_offset_ticks(100)
        .use_post_only(true)
        .cancel_orders_on_stop(true)
        .close_positions_on_stop(true)
        .log_data(false)
        .build();

    node.add_strategy(ExecTester::new(tester_config))?;
    node.run().await?;

    Ok(())
}

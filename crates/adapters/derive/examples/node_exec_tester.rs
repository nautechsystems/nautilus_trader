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

//! Example demonstrating live execution testing with the Derive adapter.
//!
//! Edit the constants below to change the environment, target instrument, and order size.
//!
//! Run with: `cargo run --example derive-exec-tester --package nautilus-derive --features examples`
//!
//! Required credential environment variables (testnet variants used when
//! `DERIVE_ENVIRONMENT` is `DeriveEnvironment::Testnet`):
//! - `DERIVE_WALLET_ADDRESS` / `DERIVE_TESTNET_WALLET_ADDRESS`.
//! - `DERIVE_SESSION_PRIVATE_KEY` / `DERIVE_TESTNET_SESSION_PRIVATE_KEY`.
//! - `DERIVE_SUBACCOUNT_ID` / `DERIVE_TESTNET_SUBACCOUNT_ID`.

use nautilus_common::enums::Environment;
use nautilus_derive::{
    common::{consts::DERIVE_CLIENT_ID, enums::DeriveEnvironment},
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
    factories::{DeriveDataClientFactory, DeriveExecFactoryConfig, DeriveExecutionClientFactory},
};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    enums::TimeInForce,
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;

const DERIVE_ENVIRONMENT: DeriveEnvironment = DeriveEnvironment::Testnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "DERIVE-001";
const NODE_NAME: &str = "DERIVE-EXEC-TESTER-001";
const STRATEGY_ID: &str = "EXEC_TESTER-001";
const INSTRUMENT_ID: &str = "ETH-PERP.DERIVE";
const ORDER_QTY: &str = "0.1";

const MAX_FEE_PER_CONTRACT: &str = "1000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let derive_environment = DERIVE_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *DERIVE_CLIENT_ID;
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec!["ETH".to_string()],
        ..Default::default()
    };

    let exec_config = DeriveExecClientConfig {
        environment: derive_environment,
        max_fee_per_contract: Some(Decimal::from_str_exact(MAX_FEE_PER_CONTRACT)?),
        ..Default::default()
    };
    let exec_factory_config = DeriveExecFactoryConfig {
        trader_id,
        account_id,
        config: exec_config,
    };

    let data_factory = DeriveDataClientFactory::new();
    let exec_factory = DeriveExecutionClientFactory::new();
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_exec_engine_config(exec_engine_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_factory_config))?
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
        .log_data(false)
        .open_position_on_start_qty(order_qty.as_decimal())
        .open_position_on_first_quote(true)
        .open_position_time_in_force(TimeInForce::Ioc)
        .use_post_only(true)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

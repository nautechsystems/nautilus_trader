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
//! Run with: `cargo run --example derive-exec-tester --package nautilus-derive --features examples`
//!
//! Pinned to Derive testnet for safe iteration. Edit `DeriveEnvironment::Testnet`
//! below to flip to mainnet when running against real funds.

use nautilus_common::enums::Environment;
use nautilus_derive::{
    common::{consts::DERIVE_CLIENT_ID, enums::DeriveEnvironment},
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
    factories::{DeriveDataClientFactory, DeriveExecFactoryConfig, DeriveExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    enums::TimeInForce,
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let derive_environment = DeriveEnvironment::Testnet;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DERIVE-001");
    let node_name = "DERIVE-EXEC-TESTER-001".to_string();
    let client_id = *DERIVE_CLIENT_ID;
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let data_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec!["ETH".to_string()],
        ..Default::default()
    };

    let exec_config = DeriveExecClientConfig {
        environment: derive_environment,
        max_fee_per_contract: Some(Decimal::from_str_exact("1000")?),
        domain_separator: env_override(
            derive_environment,
            "DERIVE_DOMAIN_SEPARATOR",
            "DERIVE_TESTNET_DOMAIN_SEPARATOR",
        ),
        action_typehash: env_override(
            derive_environment,
            "DERIVE_ACTION_TYPEHASH",
            "DERIVE_TESTNET_ACTION_TYPEHASH",
        ),
        trade_module_address: env_override(
            derive_environment,
            "DERIVE_TRADE_MODULE_ADDRESS",
            "DERIVE_TESTNET_TRADE_MODULE_ADDRESS",
        ),
        ..Default::default()
    };
    let exec_factory_config = DeriveExecFactoryConfig {
        trader_id,
        account_id,
        config: exec_config,
    };

    let data_factory = DeriveDataClientFactory::new();
    let exec_factory = DeriveExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_factory_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    let order_qty = Quantity::from("0.1");
    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
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

fn env_override(environment: DeriveEnvironment, mainnet: &str, testnet: &str) -> Option<String> {
    let var_name = if environment.is_testnet() {
        testnet
    } else {
        mainnet
    };

    std::env::var(var_name).ok()
}

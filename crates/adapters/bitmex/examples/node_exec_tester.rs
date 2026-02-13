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

//! Example demonstrating live execution testing with the BitMEX adapter.
//!
//! Run with: `cargo run --example bitmex-exec-tester --package nautilus-bitmex`
//!
//! Environment variables:
//! - `BITMEX_TESTNET`: Set to `true` to use testnet endpoints (default `true`).
//! - `BITMEX_API_KEY` / `BITMEX_API_SECRET` for mainnet credentials.
//! - `BITMEX_TESTNET_API_KEY` / `BITMEX_TESTNET_API_SECRET` for testnet credentials.
//! - `BITMEX_TEST_QTY` optional order quantity override (default `100`).

use nautilus_bitmex::{
    config::{BitmexDataClientConfig, BitmexExecClientConfig},
    factories::{BitmexDataClientFactory, BitmexExecFactoryConfig, BitmexExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

fn get_env_option(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

fn resolve_test_quantity() -> Quantity {
    get_env_option("BITMEX_TEST_QTY")
        .and_then(|value| value.parse::<u64>().ok())
        .map_or_else(|| Quantity::from("100"), Quantity::from)
}

fn resolve_bitmex_credentials(use_testnet: bool) -> anyhow::Result<(String, String)> {
    let (key_var, secret_var) = if use_testnet {
        ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
    } else {
        ("BITMEX_API_KEY", "BITMEX_API_SECRET")
    };

    let api_key = get_env_option(key_var).ok_or_else(|| anyhow::anyhow!("Set {key_var}"))?;
    let api_secret =
        get_env_option(secret_var).ok_or_else(|| anyhow::anyhow!("Set {secret_var}"))?;

    Ok((api_key, api_secret))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let use_testnet = std::env::var("BITMEX_TESTNET")
        .ok()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(true);

    let (api_key, api_secret) = resolve_bitmex_credentials(use_testnet)?;

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BITMEX-001");
    let node_name = "BITMEX-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BITMEX");
    let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
    let test_qty = resolve_test_quantity();

    let data_config = BitmexDataClientConfig {
        api_key: Some(api_key.clone()),
        api_secret: Some(api_secret.clone()),
        use_testnet,
        ..Default::default()
    };

    let exec_config = BitmexExecFactoryConfig {
        trader_id,
        account_id,
        config: BitmexExecClientConfig {
            api_key: Some(api_key),
            api_secret: Some(api_secret),
            use_testnet,
            account_id: Some(account_id),
            ..Default::default()
        },
    };

    let data_factory = BitmexDataClientFactory::new();
    let exec_factory = BitmexExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(2880)
        .with_delay_post_stop_secs(5)
        .build()?;

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC-TESTER-001"),
        instrument_id,
        client_id,
        test_qty,
    )
    .with_subscribe_trades(true)
    .with_subscribe_quotes(true)
    .with_use_post_only(true)
    .with_log_data(false)
    .with_cancel_orders_on_stop(true)
    .with_close_positions_on_stop(true);

    tester_config.base.external_order_claims = Some(vec![instrument_id]);
    tester_config.base.use_uuid_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

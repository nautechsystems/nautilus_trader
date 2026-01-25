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

//! Example demonstrating live execution testing with the Binance Futures USD-M adapter.
//!
//! Run with: `cargo run --example binance-futures-exec-tester --package nautilus-binance`
//!
//! Uses testnet by default for safety.
//!
//! Requires environment variables:
//! - BINANCE_FUTURES_TESTNET_API_KEY: Your Binance Futures testnet API key
//! - BINANCE_FUTURES_TESTNET_API_SECRET: Your Binance Futures testnet API secret

use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use rust_decimal::{Decimal, prelude::FromStr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-FUTURES-001");
    let node_name = "BINANCE-FUTURES-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BINANCE");
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let data_config = BinanceDataClientConfig {
        product_types: vec![BinanceProductType::UsdM],
        environment: BinanceEnvironment::Testnet,
        api_key: None,
        api_secret: None,
        ed25519_api_key: None,
        ed25519_api_secret: None,
        ..Default::default()
    };

    let exec_config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_types: vec![BinanceProductType::UsdM],
        environment: BinanceEnvironment::Testnet,
        api_key: None,    // Will use 'BINANCE_FUTURES_TESTNET_API_KEY' env var
        api_secret: None, // Will use 'BINANCE_FUTURES_TESTNET_API_SECRET' env var
        base_url_http: None,
        base_url_ws: None,
    };

    let data_factory = BinanceDataClientFactory::new();
    let exec_factory = BinanceExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_timeout_connection(10)
        .with_delay_post_stop_secs(5)
        .build()?;

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("0.01"), // Small quantity for testing
    )
    .with_log_data(false)
    .with_open_position_on_start(Some(Decimal::from_str("0.01").unwrap()))
    .with_cancel_orders_on_stop(true)
    .with_close_positions_on_stop(true);

    // Use UUIDs for unique client order IDs across restarts
    tester_config.base.use_uuid_client_order_ids = true;

    tester_config.base.external_order_claims = Some(vec![instrument_id]);
    tester_config.use_post_only = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

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

//! Example demonstrating live execution testing with the Binance Spot adapter.
//!
//! Run with: `cargo run --example binance-spot-exec-tester --package nautilus-binance --features examples`
//!
//! Requires environment variables based on the configured environment
//! (Ed25519 keys are auto-detected):
//! - Mainnet: `BINANCE_API_KEY` / `BINANCE_API_SECRET`
//! - Testnet: `BINANCE_TESTNET_API_KEY` / `BINANCE_TESTNET_API_SECRET`
//! - Demo: `BINANCE_DEMO_API_KEY` / `BINANCE_DEMO_API_SECRET`

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
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-001");
    let node_name = "BINANCE-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BINANCE");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let data_config = BinanceDataClientConfig {
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        api_key: None,
        api_secret: None,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
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

    let order_qty = Quantity::from("0.0001"); // Small quantity for testing
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
        .enable_limit_sells(false)
        .open_position_on_start_qty(order_qty.as_decimal())
        .use_post_only(true)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

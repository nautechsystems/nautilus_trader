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
//! Credentials are resolved from environment variables automatically when not passed
//! explicitly in the config (`api_key` / `api_secret` fields):
//! - Testnet: `BITMEX_TESTNET_API_KEY` / `BITMEX_TESTNET_API_SECRET`
//! - Mainnet: `BITMEX_API_KEY` / `BITMEX_API_SECRET`
//!
//! Run with: `cargo run --example bitmex-exec-tester --package nautilus-bitmex --features examples`

use nautilus_bitmex::{
    common::enums::BitmexEnvironment,
    config::{BitmexDataClientConfig, BitmexExecClientConfig},
    factories::{BitmexDataClientFactory, BitmexExecFactoryConfig, BitmexExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, StrategyId, TraderId},
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
    let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

    let data_config = BitmexDataClientConfig {
        environment: BitmexEnvironment::Testnet,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = BitmexExecFactoryConfig::new(
        trader_id,
        BitmexExecClientConfig {
            environment: BitmexEnvironment::Testnet,
            transport_backend: TransportBackend::Sockudo,
            ..Default::default()
        },
    );

    let data_factory = BitmexDataClientFactory::new();
    let exec_factory = BitmexExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(2880)
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC-TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(ClientId::new("BITMEX"))
        .order_qty(Quantity::from("100"))
        .use_post_only(true)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

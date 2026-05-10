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

//! Example demonstrating live execution testing with the AX Exchange adapter.
//!
//! Run with: `cargo run --example ax-exec-tester --package nautilus-architect-ax --features examples`
//!
//! Environment variables:
//! - `AX_API_KEY`: Your API key
//! - `AX_API_SECRET`: Your API secret
//! - `AX_SYMBOL`: Instrument symbol (default: XAU-PERP)
//!
//! Example instruments across asset classes:
//! - `XAU-PERP` (metals, qty=1, ~$4600)
//! - `NVDA-PERP` (equities, qty=1, ~$167)
//! - `XAG-PERP` (metals, qty=1, ~$73)
//! - `UNG-PERP` (energy ETFs, qty=1, ~$12)
//! - `OCPI-H100-PERP` (compute, qty=100, ~$1.60)
//! - `EURUSD-PERP` (fx, qty=100, ~$1.15)

use nautilus_architect_ax::{
    common::enums::AxEnvironment,
    config::{AxDataClientConfig, AxExecClientConfig},
    factories::{AxDataClientFactory, AxExecutionClientFactory},
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
    let account_id = AccountId::from("AX-001");
    let node_name = "AX-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("AX");
    let symbol = std::env::var("AX_SYMBOL").unwrap_or_else(|_| "XAU-PERP".to_string());
    let instrument_id = InstrumentId::from(format!("{symbol}.AX"));

    let ax_environment = if std::env::var("AX_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true)
    {
        AxEnvironment::Sandbox
    } else {
        AxEnvironment::Production
    };

    let data_config = AxDataClientConfig {
        environment: ax_environment,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = AxExecClientConfig {
        trader_id,
        account_id,
        environment: ax_environment,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let data_factory = AxDataClientFactory::new();
    let exec_factory = AxExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    // Use minimum order size per instrument category
    let order_qty = match symbol.as_str() {
        s if s.starts_with("OCPI") => Quantity::from(100),
        "EURUSD-PERP" | "GBPUSD-PERP" | "BRLUSD-PERP" => Quantity::from(100),
        "MXNUSD-PERP" => Quantity::from(1000),
        "JPYUSD-PERP" => Quantity::from(10000),
        _ => Quantity::from(1),
    };

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .open_position_on_start_qty(order_qty.as_decimal())
        .use_post_only(true)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

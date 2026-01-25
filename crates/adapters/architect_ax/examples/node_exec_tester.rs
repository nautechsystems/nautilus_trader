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
//! Run with: `cargo run --example ax-exec-tester --package nautilus-architect-ax`
//!
//! Environment variables:
//! - `AX_API_KEY`: Your API key
//! - `AX_API_SECRET`: Your API secret
//! - `AX_TOTP_SECRET`: Base32 TOTP secret (if 2FA enabled)

use nautilus_architect_ax::{
    config::{AxDataClientConfig, AxExecClientConfig},
    factories::{AxDataClientFactory, AxExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let node_name = "AX-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("AX");
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");

    let data_config = AxDataClientConfig {
        api_key: None,    // Will use 'AX_API_KEY' env var
        api_secret: None, // Will use 'AX_API_SECRET' env var
        is_sandbox: true, // Use sandbox environment for testing
        ..Default::default()
    };

    let exec_config = AxExecClientConfig {
        trader_id,
        account_id,
        api_key: None,     // Will use 'AX_API_KEY' env var
        api_secret: None,  // Will use 'AX_API_SECRET' env var
        totp_secret: None, // Will use 'AX_TOTP_SECRET' env var
        is_sandbox: true,  // Use sandbox environment for testing
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

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("1000"), // Minor units for AX
    )
    .with_log_data(false)
    .with_use_post_only(true)
    .with_cancel_orders_on_stop(true)
    .with_close_positions_on_stop(true);

    tester_config.base.external_order_claims = Some(vec![instrument_id]);

    // Use UUIDs for unique client order IDs
    tester_config.base.use_uuid_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

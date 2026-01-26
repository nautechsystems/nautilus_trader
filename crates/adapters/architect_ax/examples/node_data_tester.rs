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

//! Example demonstrating live data testing with the AX Exchange adapter.
//!
//! Run with: `cargo run --example ax-data-tester --package nautilus-architect-ax`
//!
//! Environment variables:
//! - `AX_API_KEY`: Your API key
//! - `AX_API_SECRET`: Your API secret
//! - `AX_IS_SANDBOX`: Set to "true" for sandbox (default), "false" for production

use nautilus_architect_ax::{config::AxDataClientConfig, factories::AxDataClientFactory};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::BarType,
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "AX-TESTER-001".to_string();

    let symbol = "JPYUSD";
    let instrument_ids = vec![
        InstrumentId::from(format!("{symbol}-PERP.AX")),
        // InstrumentId::from("EURUSD-PERP.AX"),
        // InstrumentId::from("BTCUSD-PERP.AX"),
    ];

    let is_sandbox = std::env::var("AX_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);

    let ax_config = AxDataClientConfig {
        api_key: std::env::var("AX_API_KEY").ok(),
        api_secret: std::env::var("AX_API_SECRET").ok(),
        is_sandbox,
        ..Default::default()
    };

    let client_factory = AxDataClientFactory::new();
    let client_id = ClientId::new("AX");

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(ax_config))?
        .build()?;

    let bar_types = vec![BarType::from(format!(
        "{symbol}-PERP.AX-1-MINUTE-LAST-EXTERNAL"
    ))];

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        .with_subscribe_quotes(true)
        .with_subscribe_trades(true)
        .with_subscribe_book_deltas(true)
        .with_subscribe_bars(true)
        .with_bar_types(bar_types)
        .with_request_instruments(true);
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

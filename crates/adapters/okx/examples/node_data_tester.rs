// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Example demonstrating live data testing with the OKX adapter.
//!
//! Run with: `cargo run --example node_data_tester`

use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};
use nautilus_okx::{
    common::enums::OKXInstrumentType, config::OKXDataClientConfig, factories::OKXDataClientFactory,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "OKX-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTC-USDT-SWAP.OKX"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
    ];

    let okx_config = OKXDataClientConfig {
        api_key: None,        // Will use 'OKX_API_KEY' env var
        api_secret: None,     // Will use 'OKX_API_SECRET' env var
        api_passphrase: None, // Will use 'OKX_PASSPHRASE' env var
        instrument_types: vec![OKXInstrumentType::Swap],
        is_demo: false,
        ..Default::default()
    };

    let client_factory = OKXDataClientFactory::new();
    let client_id = ClientId::new("OKX");

    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .add_data_client(None, Box::new(client_factory), Box::new(okx_config))?
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids, true, true);
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

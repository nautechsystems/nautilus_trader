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

//! Example demonstrating live data testing with the Massive adapter.
//!
//! Edit the constants below to change the target instrument and bar specification.
//!
//! Run with: `cargo run --example massive-data-tester --package nautilus-massive --features examples`
//!
//! Credentials are read from the environment:
//! - `MASSIVE_API_KEY`: Massive API key (required).

use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_massive::{
    common::consts::MASSIVE_CLIENT_ID, config::MassiveDataClientConfig,
    factories::MassiveDataClientFactory,
};
use nautilus_model::{
    data::bar::BarType,
    identifiers::{InstrumentId, TraderId},
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "MASSIVE-TESTER-001";
const INSTRUMENT_ID: &str = "AAPL.MASSIVE";
const BAR_SPEC: &str = "1-MINUTE-LAST-EXTERNAL";

// *** THIS IS A TEST CONFIGURATION FOR MARKET DATA ONLY. ***
// *** THE MASSIVE ADAPTER DOES NOT SUPPORT ORDER EXECUTION. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *MASSIVE_CLIENT_ID;

    let instrument_ids = vec![InstrumentId::from(INSTRUMENT_ID)];

    let bar_types: Vec<BarType> = instrument_ids
        .iter()
        .map(|id| BarType::from(format!("{id}-{BAR_SPEC}").as_str()))
        .collect();

    let massive_config = MassiveDataClientConfig {
        api_key: None, // Will use 'MASSIVE_API_KEY' env var
        symbols: instrument_ids
            .iter()
            .map(|id| id.symbol.to_string())
            .collect(),
        ..Default::default()
    };

    let client_factory = MassiveDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(massive_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .bar_types(bar_types)
        .subscribe_bars(true)
        .request_bars(true)
        .build()?;

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

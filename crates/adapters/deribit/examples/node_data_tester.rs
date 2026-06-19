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

//! Example demonstrating live data testing with the Deribit adapter.
//!
//! Edit the constants below to change the environment, target instruments, and bar types.
//!
//! Run with: `cargo run --example deribit-data-tester --package nautilus-deribit --features examples`
//!
//! Credentials are read from the environment when set:
//! - `DERIBIT_API_KEY`.
//! - `DERIBIT_API_SECRET`.

use nautilus_common::enums::Environment;
use nautilus_deribit::{
    common::{consts::DERIBIT_CLIENT_ID, enums::DeribitEnvironment},
    config::DeribitDataClientConfig,
    factories::DeribitDataClientFactory,
    http::models::DeribitProductType,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{InstrumentId, TraderId},
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const DERIBIT_ENVIRONMENT: DeribitEnvironment = DeribitEnvironment::Mainnet;
const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "DERIBIT-TESTER-001";
const INSTRUMENT_ID_1: &str = "BTC-PERPETUAL.DERIBIT";
const INSTRUMENT_ID_2: &str = "ETH-PERPETUAL.DERIBIT";
const BAR_TYPE_1: &str = "BTC-PERPETUAL.DERIBIT-1-MINUTE-LAST-EXTERNAL";
const BAR_TYPE_2: &str = "ETH-PERPETUAL.DERIBIT-1-MINUTE-LAST-EXTERNAL";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();
    let instrument_ids = vec![
        InstrumentId::from(INSTRUMENT_ID_1),
        InstrumentId::from(INSTRUMENT_ID_2),
    ];

    let deribit_config = DeribitDataClientConfig {
        api_key: None,    // Will use 'DERIBIT_API_KEY' env var
        api_secret: None, // Will use 'DERIBIT_API_SECRET' env var
        product_types: vec![DeribitProductType::Future],
        environment: DERIBIT_ENVIRONMENT,
        ..Default::default()
    };

    let client_factory = DeribitDataClientFactory::new();
    let client_id = *DERIBIT_CLIENT_ID;

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(client_factory), Box::new(deribit_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let bar_types = vec![BarType::from(BAR_TYPE_1), BarType::from(BAR_TYPE_2)];

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_index_prices(true)
        .subscribe_mark_prices(true)
        .subscribe_instrument_status(true)
        .bar_types(bar_types)
        .subscribe_bars(true)
        .request_trades(true)
        .request_bars(true)
        .manage_book(true)
        .build();

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

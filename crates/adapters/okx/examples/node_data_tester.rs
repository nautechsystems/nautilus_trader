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

//! Example demonstrating live data testing with the OKX adapter.
//!
//! Edit the constants below to change the environment and target instrument.
//!
//! Run with: `cargo run --example okx-data-tester --package nautilus-okx --features examples`
//!
//! Credentials are read from the environment when set:
//! - `OKX_API_KEY`.
//! - `OKX_API_SECRET`.
//! - `OKX_API_PASSPHRASE`.

use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_okx::{
    common::{
        consts::OKX_CLIENT_ID,
        enums::{OKXEnvironment, OKXInstrumentType},
    },
    config::OKXDataClientConfig,
    factories::OKXDataClientFactory,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const OKX_ENVIRONMENT: OKXEnvironment = OKXEnvironment::Live;
const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "OKX-TESTER-001";
const INSTRUMENT_ID: &str = "BTC-USDT-SWAP.OKX";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();
    let instrument_ids = vec![InstrumentId::from(INSTRUMENT_ID)];

    let okx_config = OKXDataClientConfig {
        api_key: None,        // Will use 'OKX_API_KEY' env var
        api_secret: None,     // Will use 'OKX_API_SECRET' env var
        api_passphrase: None, // Will use 'OKX_API_PASSPHRASE' env var
        instrument_types: vec![OKXInstrumentType::Swap],
        environment: OKX_ENVIRONMENT,
        ..Default::default()
    };

    let client_factory = OKXDataClientFactory::new();
    let client_id = *OKX_CLIENT_ID;

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(okx_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_mark_prices(true)
        .subscribe_index_prices(true)
        .subscribe_funding_rates(true)
        .subscribe_instrument_status(true)
        .request_book_snapshot(true)
        .request_funding_rates(true)
        .manage_book(true)
        .build()?;
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

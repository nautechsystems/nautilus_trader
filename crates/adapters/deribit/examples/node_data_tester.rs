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
//! Run with: `cargo run --example deribit-data-tester --package nautilus-deribit --features examples`

use nautilus_common::enums::Environment;
use nautilus_deribit::{
    common::enums::DeribitEnvironment, config::DeribitDataClientConfig,
    factories::DeribitDataClientFactory, http::models::DeribitProductType,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "DERIBIT-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
        InstrumentId::from("ETH-PERPETUAL.DERIBIT"),
    ];

    let deribit_config = DeribitDataClientConfig {
        api_key: None,    // Will use 'DERIBIT_API_KEY' env var
        api_secret: None, // Will use 'DERIBIT_API_SECRET' env var
        product_types: vec![DeribitProductType::Future],
        environment: DeribitEnvironment::Mainnet,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let client_factory = DeribitDataClientFactory::new();
    let client_id = ClientId::new("DERIBIT");

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(client_factory), Box::new(deribit_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    // Define bar types for subscriptions (1-minute bars)
    let bar_types = vec![
        BarType::from("BTC-PERPETUAL.DERIBIT-1-MINUTE-LAST-EXTERNAL"),
        BarType::from("ETH-PERPETUAL.DERIBIT-1-MINUTE-LAST-EXTERNAL"),
    ];

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

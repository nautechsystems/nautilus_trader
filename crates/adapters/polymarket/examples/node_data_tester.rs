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

//! Example demonstrating live data testing with the Polymarket adapter.
//!
//! Connects to Polymarket via WebSocket, loads all instruments from the Gamma
//! API, then subscribes to trade ticks for selected presidential election 2028
//! markets.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example polymarket-data-tester --package nautilus-polymarket --features examples
//! ```

use std::sync::Arc;

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};
use nautilus_network::websocket::TransportBackend;
use nautilus_polymarket::{
    config::PolymarketDataClientConfig, factories::PolymarketDataClientFactory,
    filters::EventSlugFilter,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let node_name = "POLYMARKET-DATA-TESTER-001".to_string();

    // GTA VI Released Before June 2026 (Yes/No)
    // https://polymarket.com/event/gta-vi-released-before-june-2026
    // These instrument IDs are discovered via the Gamma API; in practice you'd
    // use an InstrumentProvider to resolve slugs into IDs dynamically.
    let instrument_ids = vec![
        // Yes
        InstrumentId::from(
            "0xcccb7e7613a087c132b69cbf3a02bece3fdcb824c1da54ae79acc8d4a562d902-8441400852834915183759801017793514978104486628517653995211751018945988243154.POLYMARKET",
        ),
        // No
        InstrumentId::from(
            "0xcccb7e7613a087c132b69cbf3a02bece3fdcb824c1da54ae79acc8d4a562d902-109289569086508934142323222102974769075074494425163878721602922903101062859033.POLYMARKET",
        ),
    ];

    // Use EventSlugFilter so bootstrap loads only this event's instruments
    // instead of walking the full Gamma catalog.
    let event_slugs = vec!["gta-vi-released-before-june-2026".to_string()];
    let data_filter = EventSlugFilter::from_slugs(event_slugs);

    let polymarket_config = PolymarketDataClientConfig {
        filters: vec![Arc::new(data_filter)],
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };
    let client_factory = PolymarketDataClientFactory;
    let client_id = ClientId::new("POLYMARKET");

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(polymarket_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_trades(true)
        .subscribe_quotes(true)
        .manage_book(true)
        .build();
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

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

//! Example demonstrating live execution testing with the Polymarket adapter.
//!
//! Uses `EventSlugFilter` to load only instruments for a specific event (avoiding
//! loading all 71K+ instruments) and `SignatureType::PolyGnosisSafe` for Gnosis
//! Safe proxy wallet authentication.
//!
//! Prerequisites:
//! - Set `POLYMARKET_PK` (EOA signer private key)
//! - Set `POLYMARKET_API_KEY`, `POLYMARKET_API_SECRET`, `POLYMARKET_PASSPHRASE`
//! - Set `POLYMARKET_FUNDER` (Gnosis Safe proxy address)
//!
//! # Usage
//!
//! ```sh
//! cargo run --example polymarket-exec-tester --package nautilus-polymarket --features examples
//! ```

use std::sync::Arc;

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_polymarket::{
    common::enums::SignatureType,
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
    filters::EventSlugFilter,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let node_name = "POLYMARKET-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("POLYMARKET");

    // GTA VI Released Before June 2026 (Yes)
    // https://polymarket.com/event/gta-vi-released-before-june-2026
    let instrument_id = InstrumentId::from(
        "0xcccb7e7613a087c132b69cbf3a02bece3fdcb824c1da54ae79acc8d4a562d902-8441400852834915183759801017793514978104486628517653995211751018945988243154.POLYMARKET",
    );

    // Use EventSlugFilter to load only instruments for this event
    let event_slugs = vec!["gta-vi-released-before-june-2026".to_string()];
    let data_filter = EventSlugFilter::from_slugs(event_slugs);

    let data_config = PolymarketDataClientConfig {
        filters: vec![Arc::new(data_filter)],
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };
    let data_factory = PolymarketDataClientFactory;

    // PolyGnosisSafe: POLYMARKET_PK is the EOA signer, POLYMARKET_FUNDER is the Gnosis Safe proxy
    let exec_config = PolymarketExecClientConfig {
        trader_id,
        account_id,
        signature_type: SignatureType::PolyGnosisSafe,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };
    let exec_factory = PolymarketExecutionClientFactory;

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(120)
        .with_timeout_reconciliation(60)
        .with_delay_post_stop_secs(5)
        .build()?;

    let order_qty = Quantity::from("5");
    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty) // Polymarket min_qty = 5 shares
        // .open_position_on_start_qty(order_qty.as_decimal())
        // .use_quote_quantity(true)
        .use_post_only(true)
        .tob_offset_ticks(5) // 5 ticks = 0.005 offset (price range 0.001-0.999)
        .enable_limit_sells(false) // Can't sell without inventory on Polymarket
        .enable_stop_buys(false) // Polymarket doesn't support stop orders
        .enable_stop_sells(false)
        .reduce_only_on_stop(false) // Polymarket does not support reduce-only orders
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

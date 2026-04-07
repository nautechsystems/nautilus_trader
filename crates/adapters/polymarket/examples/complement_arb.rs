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

//! Example demonstrating complement arbitrage on Polymarket binary options.
//!
//! Uses an [`EventParamsFilter`] to load liquid sports markets from the Gamma
//! events API, plus a [`NewMarketPredicateFilter`] to accept only sport-tagged
//! new markets via WebSocket. The [`ComplementArb`] strategy discovers complement
//! pairs (same condition ID, opposite Yes/No outcomes) and monitors for arbitrage
//! when combined ask < 1.0 (buy arb) or combined bid > 1.0 (sell arb).
//!
//! Prerequisites:
//! - Set `POLYMARKET_PK` (EOA signer private key)
//! - Set `POLYMARKET_API_KEY`, `POLYMARKET_API_SECRET`, `POLYMARKET_PASSPHRASE`
//! - Set `POLYMARKET_FUNDER` (Gnosis Safe proxy address)
//!
//! # Usage
//!
//! ```sh
//! cargo run --example polymarket-complement-arb --package nautilus-polymarket
//! ```

use std::sync::Arc;

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, ClientId, StrategyId, TraderId, Venue};
use nautilus_polymarket::{
    common::enums::SignatureType,
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
    filters::{EventParamsFilter, NewMarketPredicateFilter},
    http::query::GetGammaEventsParams,
};
use nautilus_trading::{
    examples::strategies::{ComplementArb, ComplementArbConfig},
    strategy::StrategyConfig,
};
use rust_decimal_macros::dec;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = ClientId::new("POLYMARKET");
    let venue = Venue::new("POLYMARKET");

    // Filter to liquid sports markets via events endpoint
    let sports_events = GetGammaEventsParams {
        active: Some(true),
        closed: Some(false),
        tag_slug: Some("sports".into()),
        liquidity_min: Some(100000.0),
        max_events: Some(10),
        ..Default::default()
    };
    let data_filter = EventParamsFilter::new(sports_events);

    // Filter new markets (WS) to only accept sport-tagged markets
    let new_market_filter = NewMarketPredicateFilter::new("sports-only", |nm| {
        nm.tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("sports") || t.eq_ignore_ascii_case("sport"))
    });

    let data_config = PolymarketDataClientConfig {
        subscribe_new_markets: true,
        filters: vec![Arc::new(data_filter)],
        new_market_filter: Some(Arc::new(new_market_filter)),
        ..Default::default()
    };

    let exec_config = PolymarketExecClientConfig {
        trader_id,
        account_id,
        signature_type: SignatureType::PolyGnosisSafe,
        ..Default::default()
    };

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("POLYMARKET-COMPLEMENT-ARB-001".to_string())
        .with_logging(log_config)
        .add_data_client(
            None,
            Box::new(PolymarketDataClientFactory),
            Box::new(data_config),
        )?
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(exec_config),
        )?
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(120)
        .with_timeout_reconciliation(60)
        .with_delay_post_stop_secs(5)
        .build()?;

    let strategy_config = ComplementArbConfig::builder()
        .venue(venue)
        .client_id(client_id)
        .min_profit_bps(dec!(50))
        .trade_size(dec!(10))
        .live_trading(false)
        .order_expire_secs(15)
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("COMPLEMENT_ARB-001")),
            order_id_tag: Some("001".to_string()),
            log_events: false,
            log_commands: false,
            ..Default::default()
        })
        .build();

    let strategy = ComplementArb::new(strategy_config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

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

//! Example demonstrating live data testing with the Betfair adapter.
//!
//! Run with: `cargo run -p nautilus-betfair --example betfair-data-tester --features examples`
//!
//! Environment variables:
//! - `BETFAIR_USERNAME`: Your Betfair username
//! - `BETFAIR_PASSWORD`: Your Betfair password
//! - `BETFAIR_APP_KEY`: Your Betfair application key
//! - `BETFAIR_MARKET_ID`: Required active market ID to load and test
//!
//! Market IDs can be found from `https://www.betfair.com.au/exchange/plus/`

use std::sync::Arc;

use nautilus_betfair::{
    config::BetfairDataConfig,
    factories::BetfairDataClientFactory,
    http::client::BetfairHttpClient,
    provider::{BetfairInstrumentProvider, NavigationFilter},
};
use nautilus_common::{enums::Environment, providers::InstrumentProvider};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, TraderId},
    instruments::{Instrument, InstrumentAny},
    types::Currency,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let market_id = std::env::var("BETFAIR_MARKET_ID").expect("BETFAIR_MARKET_ID must be set");
    let (account_currency, instruments) = load_market_context(&market_id).await?;
    let instrument_ids = instrument_ids(&instruments);

    println!("Found instruments for market {market_id}: {instrument_ids:?}");

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let node_name = "BETFAIR-DATA-TESTER-001".to_string();
    let client_id = ClientId::new("BETFAIR");

    let data_config = BetfairDataConfig {
        account_currency,
        market_ids: Some(vec![market_id]),
        stream_conflate_ms: Some(0),
        ..Default::default()
    };

    let client_factory = BetfairDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(client_factory), Box::new(data_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_book_deltas(true)
        .subscribe_trades(true)
        .subscribe_instrument_status(true)
        .can_unsubscribe(false)
        .manage_book(true)
        .build();

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

async fn load_market_context(market_id: &str) -> anyhow::Result<(String, Vec<InstrumentAny>)> {
    let credential = BetfairDataConfig::default().credential()?;
    let http_client = Arc::new(BetfairHttpClient::new(
        credential,
        None,
        None,
        None,
        None,
        Some(5),
        None,
    )?);

    http_client.connect().await?;

    let placeholder_provider = BetfairInstrumentProvider::new(
        Arc::clone(&http_client),
        NavigationFilter::default(),
        Currency::GBP(),
        None,
    );

    let account_currency = placeholder_provider.get_account_currency().await?;
    let mut provider = BetfairInstrumentProvider::new(
        Arc::clone(&http_client),
        NavigationFilter {
            market_ids: Some(vec![market_id.to_string()]),
            ..Default::default()
        },
        account_currency,
        None,
    );

    provider.load_all(None).await?;
    http_client.disconnect().await;

    let instruments: Vec<InstrumentAny> =
        provider.store().list_all().into_iter().cloned().collect();

    if instruments.is_empty() {
        anyhow::bail!(
            "No instruments found for BETFAIR_MARKET_ID={market_id}, find an active market ID \
             from https://www.betfair.com.au/exchange/plus/ and ensure the market is still available"
        );
    }

    Ok((account_currency.code.as_str().to_string(), instruments))
}

fn instrument_ids(instruments: &[InstrumentAny]) -> Vec<InstrumentId> {
    instruments.iter().map(InstrumentAny::id).collect()
}

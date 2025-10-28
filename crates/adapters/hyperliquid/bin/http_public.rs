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

use std::env;

use nautilus_hyperliquid::http::client::HyperliquidHttpClient;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");

    tracing::info!("Starting Hyperliquid HTTP public example");
    tracing::info!("Testnet: {testnet}");

    let client = HyperliquidHttpClient::new(testnet, Some(60));

    // Fetch metadata
    let meta = client.info_meta().await?;
    tracing::info!("Fetched {} markets", meta.universe.len());

    // Fetch BTC order book
    if let Ok(book) = client.info_l2_book("BTC").await {
        let best_bid = book
            .levels
            .first()
            .and_then(|bids| bids.first())
            .map(|l| l.px.clone())
            .unwrap_or_default();
        let best_ask = book
            .levels
            .get(1)
            .and_then(|asks| asks.first())
            .map(|l| l.px.clone())
            .unwrap_or_default();

        tracing::info!("BTC best bid: {}, best ask: {}", best_bid, best_ask);
    }

    Ok(())
}

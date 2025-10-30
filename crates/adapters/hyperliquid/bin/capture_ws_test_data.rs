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

// Capture WebSocket test data samples

use std::{fs, time::Duration};

use nautilus_hyperliquid::{
    common::consts::ws_url,
    websocket::{
        client::HyperliquidWebSocketInnerClient,
        messages::{HyperliquidWsMessage, SubscriptionRequest},
    },
};
use tokio::time::{sleep, timeout};
use ustr::Ustr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Capturing Hyperliquid WebSocket test data...");

    let url = ws_url(false); // mainnet
    let mut client = HyperliquidWebSocketInnerClient::connect(url).await?;
    sleep(Duration::from_millis(500)).await;

    let coin = Ustr::from("BTC");

    // Capture trades
    println!("Subscribing to BTC trades...");
    client
        .ws_subscribe(SubscriptionRequest::Trades { coin })
        .await?;

    let mut trades = Vec::new();
    let _result = timeout(Duration::from_secs(10), async {
        while trades.len() < 5 {
            if let Some(msg) = client.ws_next_event().await
                && let HyperliquidWsMessage::Trades { data } = msg
            {
                trades.push(data);
            }
        }
    })
    .await;

    if !trades.is_empty() {
        fs::write(
            "test_data/ws_trades_sample.json",
            serde_json::to_string_pretty(&trades[0])?,
        )?;
        println!("Saved ws_trades_sample.json");
    }

    // Unsubscribe from trades
    client
        .ws_unsubscribe(SubscriptionRequest::Trades { coin })
        .await?;
    sleep(Duration::from_millis(500)).await;

    // Capture book updates
    println!("Subscribing to BTC order book...");
    client
        .ws_subscribe(SubscriptionRequest::L2Book {
            coin,
            n_sig_figs: None,
            mantissa: None,
        })
        .await?;

    let mut books = Vec::new();
    let _result = timeout(Duration::from_secs(10), async {
        while books.len() < 3 {
            if let Some(msg) = client.ws_next_event().await
                && let HyperliquidWsMessage::L2Book { data } = msg
            {
                books.push(data);
            }
        }
    })
    .await;

    if !books.is_empty() {
        fs::write(
            "test_data/ws_l2_book_sample.json",
            serde_json::to_string_pretty(&books[0])?,
        )?;
        println!("Saved ws_l2_book_sample.json");
    }

    println!("\nWebSocket test data capture complete!");

    Ok(())
}

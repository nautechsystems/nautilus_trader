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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use futures_util::StreamExt;
use nautilus_bitmex::{http::client::BitmexHttpClient, websocket::client::BitmexWebSocketClient};
use tokio::time::Duration;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    tracing::info!("Fetching instruments from HTTP API...");
    let http_client = BitmexHttpClient::new(
        None,     // base_url: defaults to production
        None,     // api_key
        None,     // api_secret
        false,    // testnet
        Some(60), // timeout_secs
        None,     // max_retries
        None,     // retry_delay_ms
        None,     // retry_delay_max_ms
        None,     // recv_window_ms
        None,     // max_requests_per_second
        None,     // max_requests_per_minute
    )
    .expect("Failed to create HTTP client");

    let instruments = http_client
        .request_instruments(true) // active_only
        .await?;

    tracing::info!("Fetched {} instruments", instruments.len());

    let mut ws_client = BitmexWebSocketClient::new(
        None, // url: defaults to wss://ws.bitmex.com/realtime
        None,
        None,
        None,
        Some(5), // 5 second heartbeat
    )
    .unwrap();
    ws_client.initialize_instruments_cache(instruments);
    ws_client.connect().await?;

    // Give the connection a moment to stabilize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Subscribe for all execution related topics
    ws_client
        .subscribe(vec![
            "execution".to_string(),
            "order".to_string(),
            "margin".to_string(),
            "position".to_string(),
            "wallet".to_string(),
        ])
        .await?;

    // Create a future that completes on CTRL+C
    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                tracing::debug!("{event:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                ws_client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}

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
use nautilus_bitmex::{
    http::{client::BitmexHttpClient, parse::parse_instrument_any},
    websocket::client::BitmexWebSocketClient,
};
use nautilus_core::time::get_atomic_clock_realtime;
use tokio::{pin, signal, time::Duration};
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
    );

    let instruments_result = http_client
        .get_instruments(true) // active_only
        .await?;

    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let instruments: Vec<_> = instruments_result
        .iter()
        .filter_map(|inst| parse_instrument_any(inst, ts_init))
        .collect();
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
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    // Use a flag to track if we should close
    let mut should_close = false;

    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                tracing::debug!("{event:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                should_close = true;
                break;
            }
            else => break,
        }
    }

    if should_close {
        ws_client.close().await?;
    }

    Ok(())
}

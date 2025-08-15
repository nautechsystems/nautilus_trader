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
use nautilus_bitmex::websocket::client::BitmexWebSocketClient;
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let mut client = BitmexWebSocketClient::new(
        None, // url: defaults to wss://ws.bitmex.com/realtime
        None,
        None,
        Some(5), // 5 second heartbeat
    )
    .unwrap();

    client.connect().await?;

    // Subscribe for all execution related topics
    client
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

    let stream = client.stream();
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
        client.close().await?;
    }

    Ok(())
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_model::{data::bar::BarType, identifiers::InstrumentId};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let url = None;
    // let url = Some(BITMEX_WS_TESTNET_URL);

    let mut client = BitmexWebSocketClient::new(
        url,     // url: defaults to wss://ws.bitmex.com/realtime
        None,    // No API key for public feeds
        None,    // No API secret
        Some(5), // 5 second heartbeat
    )
    .unwrap();

    // let topics = vec![
    //     // "instrument".to_string(), // TODO: Needs more fields
    //     // "orderBookL2".to_string(),
    //     // "orderBook10:XBTUSD".to_string(),
    //     // "trade".to_string(),
    //     // "quote".to_string(),
    //     // "tradeBin1m:XBTUSD".to_string(),
    //     // "tradeBin5m".to_string(),
    //     // "tradeBin1h".to_string(),
    //     // "tradeBin1d".to_string(),
    //     // "quoteBin1m".to_string(),
    //     // "funding".to_string(),
    //     // "insurance".to_string(),
    //     // "liquidation".to_string(),
    // ];

    client.connect().await?;

    let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

    client.subscribe_order_book(instrument_id).await?;
    client.subscribe_order_book_25(instrument_id).await?;
    client.subscribe_order_book_depth10(instrument_id).await?;
    client.subscribe_quotes(instrument_id).await?;
    client.subscribe_trades(instrument_id).await?;

    let bar_type = BarType::from("XBTUSD.BITMEX-1-MINUTE-LAST-EXTERNAL");
    client.subscribe_bars(bar_type).await?;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    // Use a flag to track if we should close
    let mut should_close = false;

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                tracing::debug!("{msg:?}");
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

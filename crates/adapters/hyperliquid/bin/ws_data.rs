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

use std::{env, time::Duration};

use nautilus_hyperliquid::{
    common::{HyperliquidProductType, consts::ws_url},
    http::HyperliquidHttpClient,
    websocket::client::HyperliquidWebSocketClient,
};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");

    tracing::info!("Starting Hyperliquid WebSocket data example");
    tracing::info!("Testnet: {testnet}");

    // Load instruments first
    let http_client = HyperliquidHttpClient::new(testnet, None, None)?;
    let instruments = http_client.request_instruments().await?;
    tracing::info!("Loaded {} instruments", instruments.len());

    // Find BTC-USD-PERP instrument (raw_symbol is "BTC" for BTC-USD-PERP)
    let btc_inst = instruments
        .iter()
        .find(|i| i.raw_symbol().as_str() == "BTC")
        .ok_or("BTC-USD-PERP instrument not found")?;
    let instrument_id = match btc_inst {
        InstrumentAny::CryptoPerpetual(inst) => inst.id,
        _ => return Err("Expected CryptoPerpetual instrument".into()),
    };
    tracing::info!("Using instrument: {}", instrument_id);

    let ws_url = ws_url(testnet);
    tracing::info!("WebSocket URL: {ws_url}");

    let mut client = HyperliquidWebSocketClient::new(
        Some(ws_url.to_string()),
        testnet,
        HyperliquidProductType::Perp,
        None,
    );

    // Cache instruments before connecting
    client.cache_instruments(instruments);

    client.connect().await?;
    tracing::info!("Connected to Hyperliquid WebSocket");

    // Wait for connection to be fully established
    tokio::time::sleep(Duration::from_millis(500)).await;

    tracing::info!("Subscribing to trades for {}", instrument_id);
    client.subscribe_trades(instrument_id).await?;

    tracing::info!("Subscribing to BBO for {}", instrument_id);
    client.subscribe_quotes(instrument_id).await?;

    // Wait briefly to ensure subscriptions are processed
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let mut message_count = 0;
    loop {
        tokio::select! {
            Some(message) = client.next_event() => {
                message_count += 1;
                tracing::info!("Message #{}: {:?}", message_count, message);
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                client.disconnect().await?;
                break;
            }
            else => break,
        }
    }

    tracing::info!("Received {} total messages", message_count);
    Ok(())
}

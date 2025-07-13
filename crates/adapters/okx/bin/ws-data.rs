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
use nautilus_model::identifiers::InstrumentId;
use nautilus_okx::{
    common::enums::OKXInstrumentType, http::client::OKXHttpClient, websocket::OKXWebSocketClient,
};
use std::time::Duration;
use tokio::{pin, signal, time::interval};
use tracing::level_filters::LevelFilter;

/// Helper function to attempt reconnection and restore subscriptions
async fn attempt_reconnect(
    client: &mut OKXWebSocketClient,
    instruments: Vec<nautilus_model::instruments::InstrumentAny>,
    active_instrument_types: &[OKXInstrumentType],
    active_instrument_ids: &[InstrumentId],
) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_RECONNECT_ATTEMPTS: usize = 5;
    const RECONNECT_DELAY: Duration = Duration::from_secs(2);

    for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
        tracing::info!("Reconnection attempt {attempt}/{MAX_RECONNECT_ATTEMPTS}");

        // Wait before attempting reconnection
        tokio::time::sleep(RECONNECT_DELAY).await;

        // Attempt to connect
        match client.connect(instruments.clone()).await {
            Ok(_) => {
                tracing::info!("Successfully reconnected on attempt {attempt}");

                // Restore all subscriptions
                for instrument_type in active_instrument_types {
                    if let Err(e) = client.subscribe_instruments(*instrument_type).await {
                        tracing::error!("Failed to restore instrument subscription: {e}");
                    }
                }

                for instrument_id in active_instrument_ids {
                    if let Err(e) = client.subscribe_order_book(*instrument_id).await {
                        tracing::error!("Failed to restore order book subscription: {e}");
                    }
                }

                tracing::info!("All subscriptions restored successfully");
                return Ok(());
            }
            Err(e) => {
                tracing::error!("Reconnection attempt {attempt} failed: {e}");
                if attempt == MAX_RECONNECT_ATTEMPTS {
                    return Err(format!(
                        "Failed to reconnect after {MAX_RECONNECT_ATTEMPTS} attempts"
                    )
                    .into());
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    // Configuration - similar to Python implementation
    let instrument_type = OKXInstrumentType::Swap;

    let client = OKXHttpClient::from_env().unwrap();
    let instruments = client.request_instruments(instrument_type).await?;

    // TODO: Filter instruments by contract type - requires access to raw OKXInstrument
    // For now, we'll work with all instruments of the specified type
    tracing::info!("Loaded {} instruments", instruments.len());

    let mut client = OKXWebSocketClient::from_env().unwrap();
    client.connect(instruments.clone()).await?;

    let instrument_id = InstrumentId::from("BTC-USD-SWAP.OKX");

    // let mut client_business = OKXWebSocketClient::new(
    //     Some(OKX_WS_BUSINESS_URL),
    //     None,     // No API key for public feeds
    //     None,     // No API secret
    //     None,     // No API passphrase
    //     Some(10), // 10 second heartbeat
    // )
    // .unwrap();

    // client_business.connect_data(instruments).await?;
    // let bar_type = BarType::new(
    //     instrument_id,
    //     BAR_SPEC_1_MINUTE,
    //     AggregationSource::External,
    // );
    // client_business.subscribe_bars(bar_type).await?;

    client
        .subscribe_instruments(OKXInstrumentType::Swap)
        .await?;
    // client.subscribe_tickers(instrument_id).await?;
    // client.subscribe_trades(instrument_id, true).await?;
    client.subscribe_order_book(instrument_id).await?;
    // client.subscribe_quotes(instrument_id).await?;

    // tokio::time::sleep(Duration::from_secs(1)).await;

    // client.subscribe_order_book(instrument_id).await?;
    // client.subscribe_order_book_25(instrument_id).await?;
    // client.subscribe_order_book_depth10(instrument_id).await?;
    // client.subscribe_quotes(instrument_id).await?;
    // client.subscribe_trades(instrument_id).await?;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    // Add a reconnection check interval (every 30 seconds)
    let mut reconnect_interval = interval(Duration::from_secs(30));

    let stream = client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    // Keep track of active subscriptions for reconnection
    let active_instrument_types = vec![OKXInstrumentType::Swap];
    let active_instrument_ids = vec![instrument_id];

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                tracing::debug!("{msg:?}");
                // Check if connection is still active after receiving message
                if !client.is_active() {
                    tracing::error!("Connection lost, attempting to reconnect...");
                    attempt_reconnect(&mut client, instruments.clone(),
                                     &active_instrument_types, &active_instrument_ids).await?;
                }
            }
            _ = reconnect_interval.tick() => {
                // Periodic connection check
                if !client.is_active() {
                    tracing::info!("Connection check failed, attempting to reconnect...");
                    attempt_reconnect(&mut client, instruments.clone(),
                                     &active_instrument_types, &active_instrument_ids).await?;
                } else {
                    tracing::debug!("Connection check: Connection is active");
                }
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}

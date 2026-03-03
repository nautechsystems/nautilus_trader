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

//! Manual verification script for Ax market data WebSocket.
//!
//! Tests authenticated WebSocket connection, subscription, and message parsing.
//! Defaults to sandbox environment.
//!
//! Requires environment variables:
//! - `AX_API_KEY`: Your API key
//! - `AX_API_SECRET`: Your API secret
//!
//! Usage:
//! ```bash
//! AX_API_KEY=your_key \
//!   AX_API_SECRET=your_secret \
//!   cargo run --bin architect-ws-data -p nautilus-architect
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_architect_ax::{
    common::{credential::Credential, enums::AxEnvironment},
    http::{client::AxRawHttpClient, parse::parse_perp_instrument},
    websocket::{NautilusDataWsMessage, data::AxMdWebSocketClient},
};
use nautilus_core::time::get_atomic_clock_realtime;
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let credential = Credential::resolve(None, None)
        .ok_or("AX_API_KEY and AX_API_SECRET environment variables required")?;

    let environment = if std::env::var("AX_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true)
    {
        AxEnvironment::Sandbox
    } else {
        AxEnvironment::Production
    };

    log::info!("Environment: {environment}");

    // First test basic connectivity with a public endpoint
    log::info!(
        "Testing connectivity to {}/instruments ...",
        environment.http_url()
    );
    let http_client = AxRawHttpClient::new(
        Some(environment.http_url().to_string()),
        Some(environment.orders_url().to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    let instruments_response = match http_client.get_instruments().await {
        Ok(response) => {
            log::info!(
                "Connectivity OK - got {} instruments",
                response.instruments.len()
            );

            if let Some(first) = response.instruments.first() {
                log::debug!("First instrument: {:?}", first.symbol);
            }
            response
        }
        Err(e) => {
            log::error!("Connectivity test failed: {e:?}");
            return Err(format!("Connectivity test failed: {e:?}").into());
        }
    };

    log::info!(
        "Authenticating via HTTP to {}/authenticate ...",
        environment.http_url()
    );

    let auth_response = http_client
        .authenticate(credential.api_key(), credential.api_secret(), 3600)
        .await
        .map_err(|e| format!("Authentication failed: {e:?}"))?;
    log::info!("Authenticated successfully");

    log::info!(
        "Connecting to market data WebSocket: {}",
        environment.ws_md_url()
    );
    let mut client = AxMdWebSocketClient::new(
        environment.ws_md_url().to_string(),
        auth_response.token,
        Some(30),
    );

    let test_symbol = "EURUSD-PERP";
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let maybe_instrument = instruments_response
        .instruments
        .iter()
        .find(|inst| inst.symbol.as_str() == test_symbol)
        .ok_or_else(|| format!("Instrument {test_symbol} not found in /instruments response"))?;

    let instrument = parse_perp_instrument(
        maybe_instrument,
        Decimal::ZERO,
        Decimal::ZERO,
        ts_init,
        ts_init,
    )
    .map_err(|e| format!("Failed to parse instrument {test_symbol}: {e}"))?;
    client.cache_instrument(instrument);
    log::info!("Cached instrument {test_symbol} for WebSocket parsing");

    log::info!("Establishing WebSocket connection...");
    client.connect().await?;
    log::info!("Connected");

    log::info!("Subscribing to {test_symbol} quotes and trades...");
    client.subscribe_quotes(test_symbol).await?;
    client.subscribe_trades(test_symbol).await?;
    log::info!("Subscribed");

    log::info!("Listening for messages (30 seconds)...");
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    let mut message_count = 0;

    {
        let stream = client.stream();
        tokio::pin!(stream);

        while let Some(msg) = stream.next().await {
            message_count += 1;

            match &msg {
                NautilusDataWsMessage::Heartbeat => {
                    log::debug!("Heartbeat");
                }
                NautilusDataWsMessage::Data(data) => {
                    for item in data {
                        log::info!("Data: {item:?}");
                    }
                }
                NautilusDataWsMessage::Deltas(deltas) => {
                    log::info!("Deltas: {}", deltas.instrument_id);
                }
                NautilusDataWsMessage::Bar(bar) => {
                    log::info!("Bar: {}", bar.bar_type);
                }
                NautilusDataWsMessage::Error(err) => {
                    log::error!("Error: {}", err.message);
                }
                NautilusDataWsMessage::Reconnected => {
                    log::warn!("Reconnected");
                }
            }

            if start.elapsed() > timeout {
                log::info!("Timeout reached");
                break;
            }
        }
    }

    log::info!("Disconnecting...");
    client.disconnect().await;

    log::info!("Received {message_count} messages");
    log::info!("Done");

    Ok(())
}

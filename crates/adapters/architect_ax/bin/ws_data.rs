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
//! For 2FA (if enabled on your account):
//! - `AX_TOTP_SECRET`: Base32 TOTP secret for auto-generating codes
//!
//! Usage:
//! ```bash
//! AX_API_KEY=your_key \
//!   AX_API_SECRET=your_secret \
//!   AX_TOTP_SECRET=your_totp_secret \
//!   cargo run --bin architect-ws-data -p nautilus-architect
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_architect_ax::{
    common::enums::{AxEnvironment, AxMarketDataLevel},
    http::{client::AxRawHttpClient, error::AxHttpError},
    websocket::{NautilusDataWsMessage, data::AxMdWebSocketClient},
};
use totp_rs::{Algorithm, Secret, TOTP};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let api_key = std::env::var("AX_API_KEY").expect("AX_API_KEY environment variable required");
    let api_secret =
        std::env::var("AX_API_SECRET").expect("AX_API_SECRET environment variable required");

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

    match http_client.get_instruments().await {
        Ok(response) => {
            log::info!(
                "Connectivity OK - got {} instruments",
                response.instruments.len()
            );
            if let Some(first) = response.instruments.first() {
                log::debug!("First instrument: {:?}", first.symbol);
            }
        }
        Err(e) => {
            log::error!("Connectivity test failed: {e:?}");
            return Err(format!("Connectivity test failed: {e:?}").into());
        }
    }

    log::info!(
        "Authenticating via HTTP to {}/authenticate ...",
        environment.http_url()
    );

    // Generate TOTP code from secret if available
    let totp_code: Option<String> = std::env::var("AX_TOTP_SECRET").ok().map(|secret| {
        let secret_bytes = Secret::Encoded(secret)
            .to_bytes()
            .expect("Invalid base32 TOTP secret");
        let totp =
            TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes).expect("Invalid TOTP configuration");
        let code = totp.generate_current().expect("Failed to generate TOTP");
        log::info!("Generated TOTP code from secret");
        code
    });

    // First try without TOTP (in case 2FA is disabled)
    let auth_response = match http_client.authenticate(&api_key, &api_secret, 3600).await {
        Ok(resp) => resp,
        Err(e) => {
            // Check if 2FA is required
            if matches!(e, AxHttpError::UnexpectedStatus { status: 400, .. }) {
                let code = match totp_code {
                    Some(code) => code,
                    None => {
                        log::error!("2FA required but AX_TOTP_SECRET not set");
                        return Err("2FA required but AX_TOTP_SECRET not provided".into());
                    }
                };

                log::info!("2FA required, using provided code...");
                match http_client
                    .authenticate_with_totp(&api_key, &api_secret, 3600, Some(&code))
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        log::error!("Authentication with 2FA failed: {e:?}");
                        return Err(format!("Authentication failed: {e:?}").into());
                    }
                }
            } else {
                log::error!("Authentication failed: {e:?}");
                return Err(format!("Authentication failed: {e:?}").into());
            }
        }
    };
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

    log::info!("Establishing WebSocket connection...");
    client.connect().await?;
    log::info!("Connected");

    let test_symbol = "EURUSD-PERP";
    log::info!("Subscribing to {test_symbol} L1 data...");
    client
        .subscribe(test_symbol, AxMarketDataLevel::Level1)
        .await?;
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

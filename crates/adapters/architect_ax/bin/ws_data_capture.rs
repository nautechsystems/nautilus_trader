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

//! Captures real market data from Ax WebSocket and saves to test_data files.
//!
//! This script connects to Ax, subscribes to market data at various levels,
//! and saves the raw JSON messages to the test_data directory for use in tests.
//!
//! Requires environment variables:
//! - `AX_API_KEY`: Your API key
//! - `AX_API_SECRET`: Your API secret
//!
//! For 2FA (if enabled on your account):
//! - `AX_TOTP_SECRET`: Base32 TOTP secret for auto-generating codes

use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

use futures_util::StreamExt;
use nautilus_architect_ax::{
    common::enums::{AxEnvironment, AxMarketDataLevel},
    http::{client::AxRawHttpClient, error::AxHttpError},
    websocket::{NautilusDataWsMessage, data::AxMdWebSocketClient},
};
use totp_rs::{Algorithm, Secret, TOTP};

const TEST_SYMBOL: &str = "EURUSD-PERP";

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

    let http_client = AxRawHttpClient::new(
        Some(environment.http_url().to_string()),
        Some(environment.orders_url().to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    // Generate TOTP code from secret if available
    let totp_code: Option<String> = std::env::var("AX_TOTP_SECRET").ok().map(|secret| {
        let secret_bytes = Secret::Encoded(secret)
            .to_bytes()
            .expect("Invalid base32 TOTP secret");
        let totp =
            TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes).expect("Invalid TOTP configuration");
        totp.generate_current().expect("Failed to generate TOTP")
    });

    let auth_response = match http_client.authenticate(&api_key, &api_secret, 3600).await {
        Ok(resp) => resp,
        Err(e) => {
            if matches!(e, AxHttpError::UnexpectedStatus { status: 400, .. }) {
                let code = totp_code.expect("2FA required but no TOTP code available");
                http_client
                    .authenticate_with_totp(&api_key, &api_secret, 3600, Some(&code))
                    .await?
            } else {
                return Err(format!("Authentication failed: {e:?}").into());
            }
        }
    };
    log::info!("Authenticated successfully");

    let mut captured: HashMap<String, String> = HashMap::new();
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");

    log::info!("=== Capturing L1 data for {TEST_SYMBOL} ===");
    capture_level(
        &auth_response.token,
        &environment,
        AxMarketDataLevel::Level1,
        &mut captured,
    )
    .await?;

    log::info!("=== Capturing L2 data for {TEST_SYMBOL} ===");
    capture_level(
        &auth_response.token,
        &environment,
        AxMarketDataLevel::Level2,
        &mut captured,
    )
    .await?;

    log::info!("=== Capturing L3 data for {TEST_SYMBOL} ===");
    capture_level(
        &auth_response.token,
        &environment,
        AxMarketDataLevel::Level3,
        &mut captured,
    )
    .await?;

    log::info!("Saving captured data to {test_data_dir:?}");

    for (msg_type, json) in &captured {
        let filename = match msg_type.as_str() {
            "1" => "ws_md_book_l1_captured.json",
            "2" => "ws_md_book_l2_captured.json",
            "3" => "ws_md_book_l3_captured.json",
            "s" => "ws_md_trade_captured.json",
            "h" => "ws_md_heartbeat_captured.json",
            "t" => "ws_md_ticker_captured.json",
            _ => continue,
        };

        let path = test_data_dir.join(filename);
        fs::write(&path, json)?;
        log::info!("Saved {filename}");
    }

    log::info!("Done! Captured {} message types", captured.len());

    Ok(())
}

async fn capture_level(
    token: &str,
    environment: &AxEnvironment,
    level: AxMarketDataLevel,
    captured: &mut HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = AxMdWebSocketClient::new(
        environment.ws_md_url().to_string(),
        token.to_string(),
        Some(30),
    );

    client.connect().await?;
    client.subscribe(TEST_SYMBOL, level).await?;

    {
        let stream = client.stream();
        tokio::pin!(stream);

        let timeout = Duration::from_secs(15);
        let start = std::time::Instant::now();
        let mut count = 0;

        while let Some(msg) = stream.next().await {
            let (msg_type, json) = match &msg {
                NautilusDataWsMessage::Heartbeat => ("h".to_string(), "{}".to_string()),
                NautilusDataWsMessage::Data(data) => ("data".to_string(), format!("{data:?}")),
                NautilusDataWsMessage::Deltas(deltas) => {
                    ("deltas".to_string(), format!("{deltas:?}"))
                }
                NautilusDataWsMessage::Bar(bar) => ("bar".to_string(), format!("{bar:?}")),
                _ => continue,
            };

            if let std::collections::hash_map::Entry::Vacant(e) = captured.entry(msg_type) {
                log::info!("Captured new message type: {}", e.key());
                e.insert(json);
                count += 1;
            }

            if start.elapsed() > timeout || count >= 2 {
                break;
            }
        }
    }

    client.disconnect().await;

    Ok(())
}

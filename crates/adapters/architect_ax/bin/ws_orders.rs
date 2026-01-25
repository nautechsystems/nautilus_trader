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

//! Manual verification script for Ax orders WebSocket.
//!
//! Tests authenticated WebSocket connection and order event streaming.
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
//!   cargo run --bin architect-ws-orders -p nautilus-architect
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_architect_ax::{
    common::enums::AxEnvironment,
    http::{client::AxRawHttpClient, error::AxHttpError},
    websocket::{AxOrdersWsMessage, NautilusExecWsMessage, orders::AxOrdersWebSocketClient},
};
use nautilus_model::identifiers::{AccountId, TraderId};
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

    let http_client = AxRawHttpClient::new(
        Some(environment.http_url().to_string()),
        Some(environment.orders_url().to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

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

    let account_id = AccountId::new("AX-001");
    let trader_id = TraderId::new("TESTER-001");
    log::info!("Account ID: {account_id}, Trader ID: {trader_id}");

    log::info!(
        "Connecting to orders WebSocket: {}",
        environment.ws_orders_url()
    );
    let mut client = AxOrdersWebSocketClient::new(
        environment.ws_orders_url().to_string(),
        account_id,
        trader_id,
        Some(30),
    );

    client.connect(&auth_response.token).await?;
    log::info!("Connected and authenticated");

    log::info!("Requesting open orders...");
    client.get_open_orders().await?;

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
                AxOrdersWsMessage::Authenticated => {
                    log::info!("WebSocket authenticated");
                }
                AxOrdersWsMessage::Nautilus(event) => match event {
                    NautilusExecWsMessage::OrderAccepted(event) => {
                        log::info!(
                            "Order accepted: {} {}",
                            event.client_order_id,
                            event.venue_order_id
                        );
                    }
                    NautilusExecWsMessage::OrderFilled(event) => {
                        log::info!(
                            "Order filled: {} {} @ {}",
                            event.client_order_id,
                            event.last_qty,
                            event.last_px
                        );
                    }
                    NautilusExecWsMessage::OrderCanceled(event) => {
                        log::info!("Order canceled: {}", event.client_order_id);
                    }
                    NautilusExecWsMessage::OrderExpired(event) => {
                        log::info!("Order expired: {}", event.client_order_id);
                    }
                    NautilusExecWsMessage::OrderRejected(reject) => {
                        log::warn!("Order rejected: {}", reject.client_order_id);
                    }
                    NautilusExecWsMessage::OrderCancelRejected(reject) => {
                        log::warn!("Cancel rejected: {}", reject.client_order_id);
                    }
                    NautilusExecWsMessage::OrderStatusReports(reports) => {
                        log::info!("Order status reports: {} items", reports.len());
                    }
                    NautilusExecWsMessage::FillReports(reports) => {
                        log::info!("Fill reports: {} items", reports.len());
                    }
                },
                AxOrdersWsMessage::PlaceOrderResponse(resp) => {
                    log::info!(
                        "Place order response: rid={} oid={}",
                        resp.rid,
                        resp.res.oid
                    );
                }
                AxOrdersWsMessage::CancelOrderResponse(resp) => {
                    log::info!(
                        "Cancel order response: rid={} accepted={}",
                        resp.rid,
                        resp.res.cxl_rx
                    );
                }
                AxOrdersWsMessage::OpenOrdersResponse(resp) => {
                    log::info!("Open orders: {} orders", resp.res.len());
                    for order in &resp.res {
                        log::info!(
                            "  {} {} {:?} {} @ {} ({:?})",
                            order.oid,
                            order.s,
                            order.d,
                            order.q,
                            order.p,
                            order.o
                        );
                    }
                }
                AxOrdersWsMessage::Error(err) => {
                    log::error!("Error: {}", err.message);
                }
                AxOrdersWsMessage::Reconnected => {
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

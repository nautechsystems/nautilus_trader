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

//! Cancels all open orders and closes all positions on AX Exchange.
//!
//! Usage:
//! ```bash
//! cargo run --bin ax-flatten -p nautilus-architect-ax
//! ```
//!
//! Environment variables:
//! - `AX_API_KEY`: API key (required)
//! - `AX_API_SECRET`: API secret (required)
//! - `AX_IS_SANDBOX`: Use sandbox (default: true)

use nautilus_architect_ax::{
    common::{
        consts::AX_AUTH_TOKEN_TTL_EXEC_SECS,
        enums::{AxEnvironment, AxOrderSide, AxTimeInForce},
    },
    http::{
        client::AxRawHttpClient,
        models::{CancelAllOrdersRequest, PlaceOrderRequest, PreviewAggressiveLimitOrderRequest},
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let environment = if std::env::var("AX_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true)
    {
        AxEnvironment::Sandbox
    } else {
        AxEnvironment::Production
    };

    let api_key = std::env::var("AX_API_KEY")?;
    let api_secret = std::env::var("AX_API_SECRET")?;

    log::info!("Environment: {environment}");

    let client = AxRawHttpClient::with_credentials(
        api_key.clone(),
        api_secret.clone(),
        Some(environment.http_url().to_string()),
        Some(environment.orders_url().to_string()),
        30,
        3,
        1000,
        10_000,
        None,
    )?;

    let token = client
        .authenticate(&api_key, &api_secret, AX_AUTH_TOKEN_TTL_EXEC_SECS)
        .await?;
    client.set_session_token(token.token);
    log::info!("Authenticated");

    // Cancel all open orders
    log::info!("Canceling all open orders...");
    client
        .cancel_all_orders(&CancelAllOrdersRequest::new())
        .await?;
    log::info!("Cancel all orders request sent");

    // Fetch positions and close any with non-zero quantity
    let positions = client.get_positions().await?;
    let open: Vec<_> = positions
        .positions
        .iter()
        .filter(|p| p.signed_quantity != 0)
        .collect();

    if open.is_empty() {
        log::info!("No open positions");
        return Ok(());
    }

    log::info!("Closing {} position(s)", open.len());

    for pos in &open {
        let close_side = if pos.signed_quantity > 0 {
            AxOrderSide::Sell
        } else {
            AxOrderSide::Buy
        };
        let qty = pos.signed_quantity.unsigned_abs();

        let preview = client
            .preview_aggressive_limit_order(&PreviewAggressiveLimitOrderRequest::new(
                pos.symbol, qty, close_side,
            ))
            .await?;

        let limit_price = preview
            .limit_price
            .ok_or_else(|| format!("No liquidity to close {} ({qty} contracts)", pos.symbol))?;

        if preview.remaining_quantity > 0 {
            log::warn!(
                "{} book depth insufficient: can fill {} of {qty} contracts",
                pos.symbol,
                preview.filled_quantity,
            );
        }

        log::info!("{} {close_side} {qty} @ {limit_price}", pos.symbol);

        let request = PlaceOrderRequest {
            d: close_side,
            p: limit_price,
            po: false,
            q: qty,
            s: pos.symbol,
            tif: AxTimeInForce::Ioc,
            tag: None,
            order_type: None,
            trigger_price: None,
        };

        let resp = client.place_order(&request).await?;
        log::info!("Submitted close order {}", resp.oid);
    }

    // Verify all positions are flat
    let remaining = client.get_positions().await?;
    let still_open: Vec<_> = remaining
        .positions
        .iter()
        .filter(|p| p.signed_quantity != 0)
        .collect();

    if still_open.is_empty() {
        log::info!("Flatten complete, all positions closed");
    } else {
        for pos in &still_open {
            log::error!(
                "Residual position: {} {} contracts",
                pos.symbol,
                pos.signed_quantity,
            );
        }
        return Err(format!("{} position(s) still open after flatten", still_open.len()).into());
    }

    Ok(())
}

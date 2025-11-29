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

//! gRPC execution test for dYdX adapter.
//!
//! This binary tests order submission via gRPC to dYdX v4 **mainnet**.
//! It demonstrates:
//! - Wallet initialization from mnemonic
//! - gRPC client setup
//! - Instrument loading from HTTP API
//! - Order submission via gRPC (market and limit orders)
//! - Order cancellation via gRPC
//!
//! Usage:
//! ```bash
//! # Set environment variables
//! export DYDX_MNEMONIC="your mnemonic here"
//! export DYDX_GRPC_URL="https://dydx-grpc.publicnode.com:443"  # Optional
//! export DYDX_HTTP_URL="https://indexer.dydx.trade"  # Optional
//!
//! # Run the test
//! cargo run --bin dydx-grpc-exec -p nautilus-dydx
//! ```
//!
//! **Requirements**:
//! - Valid dYdX mainnet wallet mnemonic (24 words)
//! - Mainnet funds in subaccount 0
//! - Network access to mainnet gRPC and HTTP endpoints
//!
//! **WARNING**: This connects to mainnet and can place real orders with real funds!

use std::{env, str::FromStr};

use nautilus_dydx::{
    common::consts::{DYDX_GRPC_URL, DYDX_HTTP_URL},
    grpc::{
        TxBuilder,
        client::DydxGrpcClient,
        order::{
            OrderBuilder, OrderGoodUntil, OrderMarketParams, SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
        },
        types::ChainId,
        wallet::Wallet,
    },
    http::client::DydxHttpClient,
    proto::{
        ToAny,
        dydxprotocol::clob::{MsgPlaceOrder, order::TimeInForce},
    },
};
use nautilus_model::{enums::OrderSide, identifiers::InstrumentId, types::Quantity};
use rust_decimal::Decimal;
use serde::Deserialize;
use tracing::level_filters::LevelFilter;

const DEFAULT_SUBACCOUNT: u32 = 0;

#[derive(Debug, Deserialize)]
struct Credentials {
    mnemonic: String,
    #[serde(default)]
    subaccount: u32,
}

fn load_credentials() -> Result<Credentials, Box<dyn std::error::Error>> {
    if let Ok(mnemonic) = env::var("DYDX_MNEMONIC") {
        tracing::info!("Loaded credentials from DYDX_MNEMONIC environment variable");
        return Ok(Credentials {
            mnemonic,
            subaccount: DEFAULT_SUBACCOUNT,
        });
    }

    Err(
        "No credentials found. Please set DYDX_MNEMONIC environment variable"
            .to_string()
            .into(),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Initialize rustls crypto provider (required for TLS connections)
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Load credentials
    let creds = load_credentials()?;
    let grpc_url = env::var("DYDX_GRPC_URL").unwrap_or_else(|_| DYDX_GRPC_URL.to_string());
    let http_url = env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_HTTP_URL.to_string());
    let subaccount_number = creds.subaccount;

    // Initialize wallet
    let wallet = Wallet::from_mnemonic(&creds.mnemonic)?;
    let mut account = wallet.account_offline(subaccount_number)?;
    let wallet_address = account.address.clone();
    tracing::info!("Wallet address: {}", wallet_address);

    // Initialize gRPC client
    tracing::info!("Connecting to gRPC: {}", grpc_url);
    let mut grpc_client = DydxGrpcClient::new(grpc_url).await?;

    // Query account info from chain (required for signing)
    tracing::info!("Querying account info from chain...");
    let (account_number, sequence) = grpc_client.query_address(&wallet_address).await?;
    account.set_account_info(account_number, sequence);
    tracing::info!("Account number: {}, sequence: {}", account_number, sequence);

    // Initialize HTTP client for instruments
    tracing::info!("Connecting to HTTP API: {}", http_url);
    let http_client = DydxHttpClient::new(
        Some(http_url),
        Some(30), // timeout_secs
        None,     // proxy_url
        false,    // is_testnet
        None,     // retry_config
    )
    .expect("Failed to create HTTP client");

    // Fetch instruments
    tracing::info!("Fetching instruments...");
    http_client.fetch_and_cache_instruments().await?;
    tracing::info!("Instruments cached");

    // Get current block height
    let height = grpc_client.latest_block_height().await?;
    tracing::info!("Current block height: {}", height.0);

    // Place a small limit BUY order far from market (safe, won't fill)
    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let client_order_id: u32 = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u32)
        % 1_000_000_000; // Keep it within u32 range
    let side = OrderSide::Buy;
    let quantity = Quantity::from("0.001"); // Minimum size
    let price = Decimal::from_str("10000.0")?; // Far below market (~$95k)

    tracing::info!("Placing limit order for {}", instrument_id);
    tracing::info!("Client order ID: {}", client_order_id);
    tracing::info!("Side: {:?}", side);
    tracing::info!("Price: ${}", price);
    tracing::info!("Quantity: {}", quantity);

    // Get market params from cache
    let market_params = http_client
        .get_market_params(&instrument_id)
        .ok_or("Market params not found in cache")?;

    let params = OrderMarketParams {
        atomic_resolution: market_params.atomic_resolution,
        clob_pair_id: market_params.clob_pair_id,
        oracle_price: None,
        quantum_conversion_exponent: market_params.quantum_conversion_exponent,
        step_base_quantums: market_params.step_base_quantums,
        subticks_per_tick: market_params.subticks_per_tick,
    };

    // Build limit order
    let mut builder = OrderBuilder::new(
        params,
        wallet_address.clone(),
        subaccount_number,
        client_order_id,
    );

    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;
    let proto_side = Side::Buy;
    let size_decimal = Decimal::from_str(&quantity.to_string())?;

    builder = builder.limit(proto_side, price, size_decimal);
    builder = builder.short_term();
    builder = builder.time_in_force(TimeInForce::PostOnly);
    builder = builder.until(OrderGoodUntil::Block(
        height.0 + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
    ));

    let order = builder
        .build()
        .map_err(|e| format!("Failed to build order: {e}"))?;

    tracing::info!("Order built successfully");

    // Build and broadcast transaction
    let tx_builder = TxBuilder::new(ChainId::Mainnet1, "adydx".to_string())
        .map_err(|e| format!("TxBuilder init failed: {e}"))?;

    let msg_place_order = MsgPlaceOrder { order: Some(order) };
    let any_msg = msg_place_order.to_any();

    let tx_raw = tx_builder
        .build_transaction(&account, vec![any_msg], None)
        .map_err(|e| format!("Failed to build tx: {e}"))?;

    let tx_bytes = tx_raw
        .to_bytes()
        .map_err(|e| format!("Failed to serialize tx: {e}"))?;

    tracing::info!("Broadcasting transaction...");
    let tx_hash = grpc_client
        .broadcast_tx(tx_bytes)
        .await
        .map_err(|e| format!("Broadcast failed: {e}"))?;

    tracing::info!("Order placed successfully, tx_hash: {}", tx_hash);

    Ok(())
}

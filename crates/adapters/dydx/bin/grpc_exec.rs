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
//! **Requirements**:
//! - Valid dYdX mainnet wallet mnemonic (24 words)
//! - Mainnet funds in subaccount 0
//! - Network access to mainnet gRPC and HTTP endpoints
//!
//! **WARNING**: This connects to mainnet and can place real orders with real funds!

use std::{env, str::FromStr, time::Duration};

use nautilus_dydx::{
    common::{
        consts::{DYDX_GRPC_URLS, DYDX_HTTP_URL, DYDX_TESTNET_GRPC_URLS, DYDX_TESTNET_HTTP_URL},
        enums::DydxOrderStatus,
    },
    grpc::{
        TxBuilder,
        client::DydxGrpcClient,
        order::{
            OrderBuilder, OrderGoodUntil, OrderMarketParams, SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
        },
        types::ChainId,
        wallet::{Account, Wallet},
    },
    http::{
        client::{DydxHttpClient, DydxRawHttpClient},
        models::Order as DydxOrder,
    },
    proto::{
        ToAny,
        dydxprotocol::{
            clob::{
                MsgBatchCancel, MsgCancelOrder, MsgPlaceOrder, OrderBatch, OrderId,
                msg_cancel_order::GoodTilOneof,
                order::{Side as DydxSide, TimeInForce},
            },
            subaccounts::SubaccountId,
        },
    },
};
use nautilus_model::{enums::OrderSide, identifiers::InstrumentId, types::Quantity};
use rust_decimal::Decimal;
use serde::Deserialize;
use tracing::level_filters::LevelFilter;

const DEFAULT_SUBACCOUNT: u32 = 0;
const DEFAULT_INSTRUMENT: &str = "BTC-USD-PERP.DYDX";
const DEFAULT_SIDE: &str = "buy";
const DEFAULT_PRICE: &str = "10000.0";
const DEFAULT_QUANTITY: &str = "0.001";

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

    let args: Vec<String> = env::args().collect();

    let has_network_flag = args.iter().any(|a| a == "--mainnet" || a == "--testnet");
    let has_instrument_flag = args.iter().any(|a| a == "--instrument");

    if has_network_flag && !has_instrument_flag {
        return run_all_edge_case_tests(&args).await;
    }

    let instrument_str = args
        .iter()
        .position(|a| a == "--instrument")
        .and_then(|i| args.get(i + 1))
        .map_or(DEFAULT_INSTRUMENT, |s| s.as_str());

    let side_str = args
        .iter()
        .position(|a| a == "--side")
        .and_then(|i| args.get(i + 1))
        .map_or(DEFAULT_SIDE, |s| s.as_str());

    let price_str = args
        .iter()
        .position(|a| a == "--price")
        .and_then(|i| args.get(i + 1))
        .map_or(DEFAULT_PRICE, |s| s.as_str());

    let quantity_str = args
        .iter()
        .position(|a| a == "--quantity")
        .and_then(|i| args.get(i + 1))
        .map_or(DEFAULT_QUANTITY, |s| s.as_str());

    let subaccount_arg = args
        .iter()
        .position(|a| a == "--subaccount")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok());

    let is_mainnet = args.iter().any(|a| a == "--mainnet");

    // Initialize rustls crypto provider (required for TLS connections)
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Load credentials
    let creds = load_credentials()?;
    let grpc_urls = if is_mainnet {
        DYDX_GRPC_URLS
    } else {
        DYDX_TESTNET_GRPC_URLS
    };
    let http_url = env::var("DYDX_HTTP_URL").unwrap_or_else(|_| {
        if is_mainnet {
            DYDX_HTTP_URL.to_string()
        } else {
            DYDX_TESTNET_HTTP_URL.to_string()
        }
    });
    let subaccount_number = subaccount_arg.unwrap_or(creds.subaccount);

    tracing::info!("dYdX gRPC Order Submission Test");
    tracing::info!("================================");
    tracing::info!(
        "Network:     {}",
        if is_mainnet { "MAINNET" } else { "TESTNET" }
    );
    tracing::info!("Instrument:  {}", instrument_str);
    tracing::info!("Side:        {}", side_str);
    tracing::info!("Price:       {}", price_str);
    tracing::info!("Quantity:    {}", quantity_str);
    tracing::info!("Subaccount:  {}", subaccount_number);
    tracing::info!("");

    // Initialize wallet
    let wallet = Wallet::from_mnemonic(&creds.mnemonic)?;
    let mut account = wallet.account_offline(subaccount_number)?;
    let wallet_address = account.address.clone();
    tracing::info!("Wallet address: {}", wallet_address);

    // Initialize gRPC client with fallback URLs
    tracing::info!("Connecting to gRPC endpoints (with fallback):");
    for url in grpc_urls {
        tracing::info!("  - {}", url);
    }
    let mut grpc_client = DydxGrpcClient::new_with_fallback(grpc_urls).await?;

    // Query account info from chain (required for signing)
    tracing::info!("Querying account info from chain...");
    let (account_number, sequence) = grpc_client.query_address(&wallet_address).await?;
    account.set_account_info(account_number, sequence);
    tracing::info!("Account number: {}, sequence: {}", account_number, sequence);

    // Initialize HTTP client for instruments
    tracing::info!("Connecting to HTTP API: {}", http_url);
    let http_client = DydxHttpClient::new(
        Some(http_url.clone()),
        Some(30),    // timeout_secs
        None,        // proxy_url
        !is_mainnet, // is_testnet
        None,        // retry_config
    )
    .expect("Failed to create HTTP client");

    // Also create raw HTTP client for order queries
    let raw_http_client = DydxRawHttpClient::new(Some(http_url), Some(30), None, !is_mainnet, None)
        .expect("Failed to create raw HTTP client");

    // Fetch instruments
    tracing::info!("Fetching instruments...");
    http_client.fetch_and_cache_instruments().await?;
    tracing::info!("Instruments cached");

    // Get current block height
    let height = grpc_client.latest_block_height().await?;
    tracing::info!("Current block height: {}", height.0);

    let instrument_id = InstrumentId::from(instrument_str);
    let client_order_id: u32 = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u32)
        % 1_000_000_000;
    let side = match side_str.to_lowercase().as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => return Err(format!("Invalid side: {side_str}").into()),
    };
    let quantity = Quantity::from(quantity_str);
    let price = Decimal::from_str(price_str)?;

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

    let proto_side = DydxSide::Buy;
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
    let chain_id = if is_mainnet {
        ChainId::Mainnet1
    } else {
        ChainId::Testnet4
    };
    let tx_builder = TxBuilder::new(chain_id, "adydx".to_string())
        .map_err(|e| format!("TxBuilder init failed: {e}"))?;

    let msg_place_order = MsgPlaceOrder { order: Some(order) };
    let any_msg = msg_place_order.to_any();

    let tx_raw = tx_builder
        .build_transaction(&account, vec![any_msg], None, None)
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

    // Wait a moment for order to be indexed
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Fetch and cancel all open orders
    tracing::info!("Fetching open orders to cancel...");
    let orders_response = raw_http_client
        .get_orders(&wallet_address, subaccount_number, None, None)
        .await?;

    // Filter to only OPEN orders (API returns all statuses by default)
    let open_orders: Vec<_> = orders_response
        .into_iter()
        .filter(|o| matches!(o.status, DydxOrderStatus::Open))
        .collect();

    if open_orders.is_empty() {
        tracing::info!("No open orders to cancel");
    } else {
        tracing::info!(
            "Found {} open order(s), canceling all...",
            open_orders.len()
        );

        for order in open_orders {
            // Parse client_id from string
            let client_id: u32 = order
                .client_id
                .parse()
                .map_err(|e| format!("Failed to parse client_id: {e}"))?;

            let msg_cancel = MsgCancelOrder {
                order_id: Some(OrderId {
                    subaccount_id: Some(SubaccountId {
                        owner: wallet_address.clone(),
                        number: subaccount_number,
                    }),
                    client_id,
                    order_flags: 0, // Short-term orders
                    clob_pair_id: market_params.clob_pair_id,
                }),
                good_til_oneof: Some(GoodTilOneof::GoodTilBlock(
                    height.0 + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
                )),
            };

            let any_cancel = msg_cancel.to_any();
            account.increment_sequence();
            let tx_raw = tx_builder
                .build_transaction(&account, vec![any_cancel], None, None)
                .map_err(|e| format!("Failed to build cancel tx: {e}"))?;
            let tx_bytes = tx_raw
                .to_bytes()
                .map_err(|e| format!("Failed to serialize cancel tx: {e}"))?;

            tracing::info!("Canceling order client_id={}", client_id);
            let cancel_tx_hash = grpc_client
                .broadcast_tx(tx_bytes)
                .await
                .map_err(|e| format!("Cancel broadcast failed: {e}"))?;

            tracing::info!("Order canceled, tx_hash: {}", cancel_tx_hash);
        }
    }

    Ok(())
}

async fn run_all_edge_case_tests(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let is_mainnet = args.iter().any(|a| a == "--mainnet");
    let creds = load_credentials()?;
    let wallet = Wallet::from_mnemonic(&creds.mnemonic)?;
    let mut account = wallet.account_offline(0)?;
    let wallet_address = account.address.clone();

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let grpc_urls = if is_mainnet {
        DYDX_GRPC_URLS
    } else {
        DYDX_TESTNET_GRPC_URLS
    };
    let http_url = if is_mainnet {
        DYDX_HTTP_URL
    } else {
        DYDX_TESTNET_HTTP_URL
    };

    let mut grpc_client = DydxGrpcClient::new_with_fallback(grpc_urls).await?;
    let (account_number, sequence) = grpc_client.query_address(&wallet_address).await?;
    account.set_account_info(account_number, sequence);

    let http_client = DydxHttpClient::new(
        Some(http_url.to_string()),
        Some(30),
        None,
        !is_mainnet,
        None,
    )?;
    let raw_http = DydxRawHttpClient::new(
        Some(http_url.to_string()),
        Some(30),
        None,
        !is_mainnet,
        None,
    )?;

    http_client.fetch_and_cache_instruments().await?;
    tracing::info!("Setup complete - wallet: {}", wallet_address);

    run_all_edge_tests(
        &mut grpc_client,
        &mut account,
        &wallet_address,
        &http_client,
        &raw_http,
        is_mainnet,
    )
    .await
}

async fn run_all_edge_tests(
    grpc: &mut DydxGrpcClient,
    account: &mut Account,
    address: &str,
    http: &DydxHttpClient,
    raw_http: &DydxRawHttpClient,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n=== Running All Edge Case Tests ===\n");

    test_cancel_specific(grpc, account, http, raw_http, address, is_mainnet).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    test_cancel_by_market(grpc, account, http, raw_http, address, is_mainnet).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    test_replace_order(grpc, account, http, raw_http, address, is_mainnet).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    test_duplicate_cancel(grpc, account, http, raw_http, address, is_mainnet).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    test_rapid_sequence(grpc, account, http, raw_http, address, is_mainnet).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    test_batch_cancel(grpc, account, http, raw_http, address, is_mainnet).await?;

    tracing::info!("\n=== All Tests Complete ===");
    Ok(())
}

async fn test_cancel_specific(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Cancel Specific Order ---");

    let client_id = generate_client_id();
    let tx_hash = place_edge_test_order(
        grpc,
        account,
        http,
        client_id,
        "BTC-USD-PERP.DYDX",
        10000.0,
        is_mainnet,
    )
    .await?;
    tracing::info!("Placed order {} (tx: {})", client_id, tx_hash);

    tokio::time::sleep(Duration::from_secs(3)).await;

    let orders = fetch_open_orders(raw_http, address).await?;
    let target = orders.iter().find(|o| o.client_id == client_id.to_string());

    match target {
        Some(order) => {
            tracing::info!(
                "Order found: client_id={}, status={:?}",
                order.client_id,
                order.status
            );

            cancel_order_by_client_id(
                grpc,
                account,
                http,
                address,
                client_id,
                order.ticker.as_deref().unwrap_or("BTC-USD"),
                is_mainnet,
            )
            .await?;
            tracing::info!("Canceled order {}", client_id);
        }
        None => tracing::warn!("Order {} not yet indexed or already filled", client_id),
    }

    Ok(())
}

async fn test_cancel_by_market(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Cancel All Orders for Market ---");

    for _ in 0..3 {
        let client_id = generate_client_id();
        place_edge_test_order(
            grpc,
            account,
            http,
            client_id,
            "BTC-USD-PERP.DYDX",
            10000.0,
            is_mainnet,
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tracing::info!("Placed 3 BTC orders");

    tokio::time::sleep(Duration::from_secs(3)).await;

    let orders = fetch_open_orders(raw_http, address).await?;
    let btc_orders: Vec<_> = orders
        .iter()
        .filter(|o| o.ticker.as_deref() == Some("BTC-USD"))
        .collect();

    tracing::info!("Found {} open BTC orders", btc_orders.len());

    for order in btc_orders {
        let client_id: u32 = order.client_id.parse()?;
        cancel_order_by_client_id(
            grpc,
            account,
            http,
            address,
            client_id,
            order.ticker.as_deref().unwrap_or("BTC-USD"),
            is_mainnet,
        )
        .await?;
    }

    tracing::info!("Canceled all BTC orders");
    Ok(())
}

async fn test_replace_order(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    _raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Replace Order (Cancel + Place) ---");

    let old_client_id = generate_client_id();
    place_edge_test_order(
        grpc,
        account,
        http,
        old_client_id,
        "BTC-USD-PERP.DYDX",
        10000.0,
        is_mainnet,
    )
    .await?;
    tracing::info!("Placed original order {}", old_client_id);

    tokio::time::sleep(Duration::from_secs(3)).await;

    let new_client_id = generate_client_id();

    cancel_order_by_client_id(
        grpc,
        account,
        http,
        address,
        old_client_id,
        "BTC-USD",
        is_mainnet,
    )
    .await
    .ok();
    tracing::info!("Canceled old order {}", old_client_id);

    place_edge_test_order(
        grpc,
        account,
        http,
        new_client_id,
        "BTC-USD-PERP.DYDX",
        11000.0,
        is_mainnet,
    )
    .await?;
    tracing::info!("Placed new order {} at $11,000", new_client_id);

    Ok(())
}

async fn test_duplicate_cancel(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    _raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Duplicate Cancellation ---");

    let client_id = generate_client_id();
    place_edge_test_order(
        grpc,
        account,
        http,
        client_id,
        "BTC-USD-PERP.DYDX",
        10000.0,
        is_mainnet,
    )
    .await?;
    tracing::info!("Placed order {}", client_id);

    tokio::time::sleep(Duration::from_secs(3)).await;

    cancel_order_by_client_id(
        grpc, account, http, address, client_id, "BTC-USD", is_mainnet,
    )
    .await?;
    tracing::info!("First cancel succeeded");

    tokio::time::sleep(Duration::from_secs(1)).await;
    match cancel_order_by_client_id(
        grpc, account, http, address, client_id, "BTC-USD", is_mainnet,
    )
    .await
    {
        Ok(_) => tracing::info!("Second cancel succeeded (order may have been re-indexed)"),
        Err(e) => tracing::info!("Second cancel failed as expected: {}", e),
    }

    Ok(())
}

async fn test_rapid_sequence(
    grpc: &mut DydxGrpcClient,
    account: &mut Account,
    http: &DydxHttpClient,
    _raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Rapid Order Sequence ---");

    let mut client_ids = Vec::new();

    for i in 0..5 {
        let client_id = generate_client_id();
        client_ids.push(client_id);

        match place_edge_test_order(
            grpc,
            account,
            http,
            client_id,
            "BTC-USD-PERP.DYDX",
            10000.0 + (i as f64 * 100.0),
            is_mainnet,
        )
        .await
        {
            Ok(tx) => tracing::info!("Order {} placed (tx: {})", i + 1, tx),
            Err(e) => tracing::warn!("Order {} failed: {}", i + 1, e),
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    for (i, client_id) in client_ids.iter().enumerate() {
        match cancel_order_by_client_id(
            grpc, account, http, address, *client_id, "BTC-USD", is_mainnet,
        )
        .await
        {
            Ok(_) => tracing::info!("Order {} canceled", i + 1),
            Err(e) => tracing::warn!("Cancel {} failed: {}", i + 1, e),
        }
    }

    tracing::info!("Rapid sequence test complete");
    Ok(())
}

async fn test_batch_cancel(
    grpc: &mut DydxGrpcClient,
    account: &mut Account,
    http: &DydxHttpClient,
    raw_http: &DydxRawHttpClient,
    address: &str,
    is_mainnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("\n--- Test: Batch Cancel Orders ---");

    // Place 5 orders on BTC
    let mut client_ids = Vec::new();
    for i in 0..5 {
        let client_id = generate_client_id();
        client_ids.push(client_id);
        place_edge_test_order(
            grpc,
            account,
            http,
            client_id,
            "BTC-USD-PERP.DYDX",
            10000.0 + (i as f64 * 50.0),
            is_mainnet,
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tracing::info!("Placed {} BTC orders", client_ids.len());

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Get market params for clob_pair_id
    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let market_params = http
        .get_market_params(&instrument_id)
        .ok_or("Market params not found")?;

    // Build batch cancel message
    let height = grpc.latest_block_height().await?;
    let order_batch = OrderBatch {
        clob_pair_id: market_params.clob_pair_id,
        client_ids: client_ids.clone(),
    };

    let msg_batch_cancel = MsgBatchCancel {
        subaccount_id: Some(SubaccountId {
            owner: address.to_string(),
            number: 0,
        }),
        short_term_cancels: vec![order_batch],
        good_til_block: height.0 + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
    };

    // Broadcast batch cancel
    let chain_id = if is_mainnet {
        ChainId::Mainnet1
    } else {
        ChainId::Testnet4
    };
    let tx_builder = TxBuilder::new(chain_id, "adydx".to_string())?;
    let tx_raw =
        tx_builder.build_transaction(account, vec![msg_batch_cancel.to_any()], None, None)?;
    let tx_hash = grpc.broadcast_tx(tx_raw.to_bytes()?).await?;

    tracing::info!(
        "Batch canceled {} orders in single transaction: {}",
        client_ids.len(),
        tx_hash
    );

    // Verify cancellations
    tokio::time::sleep(Duration::from_secs(2)).await;
    let orders = fetch_open_orders(raw_http, address).await?;
    let remaining = orders
        .iter()
        .filter(|o| client_ids.contains(&o.client_id.parse::<u32>().unwrap_or(0)))
        .count();
    tracing::info!(
        "Batch cancel complete - {} orders remaining (expected 0)",
        remaining
    );

    Ok(())
}

fn generate_client_id() -> u32 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32)
        % 1_000_000_000
}

async fn place_edge_test_order(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    client_id: u32,
    instrument: &str,
    price: f64,
    is_mainnet: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let instrument_id = InstrumentId::from(instrument);
    let market_params = http
        .get_market_params(&instrument_id)
        .ok_or("Market params not found")?;

    let params = OrderMarketParams {
        atomic_resolution: market_params.atomic_resolution,
        clob_pair_id: market_params.clob_pair_id,
        oracle_price: None,
        quantum_conversion_exponent: market_params.quantum_conversion_exponent,
        step_base_quantums: market_params.step_base_quantums,
        subticks_per_tick: market_params.subticks_per_tick,
    };

    let height = grpc.latest_block_height().await?;

    let mut builder = OrderBuilder::new(params, account.address.clone(), 0, client_id);

    builder = builder.limit(
        DydxSide::Buy,
        Decimal::from_str(&price.to_string())?,
        Decimal::from_str("0.001")?,
    );
    builder = builder.short_term();
    builder = builder.time_in_force(TimeInForce::PostOnly);
    builder = builder.until(OrderGoodUntil::Block(
        height.0 + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
    ));

    let order = builder.build()?;
    let msg = MsgPlaceOrder { order: Some(order) };

    let chain_id = if is_mainnet {
        ChainId::Mainnet1
    } else {
        ChainId::Testnet4
    };
    let tx_builder = TxBuilder::new(chain_id, "adydx".to_string())?;
    let tx_raw = tx_builder.build_transaction(account, vec![msg.to_any()], None, None)?;
    let tx_hash = grpc.broadcast_tx(tx_raw.to_bytes()?).await?;

    Ok(tx_hash)
}

async fn cancel_order_by_client_id(
    grpc: &mut DydxGrpcClient,
    account: &Account,
    http: &DydxHttpClient,
    address: &str,
    client_id: u32,
    ticker: &str,
    is_mainnet: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let instrument_id = InstrumentId::from(format!("{ticker}-PERP.DYDX"));
    let market_params = http
        .get_market_params(&instrument_id)
        .ok_or("Market params not found")?;

    let height = grpc.latest_block_height().await?;

    let msg_cancel = MsgCancelOrder {
        order_id: Some(OrderId {
            subaccount_id: Some(SubaccountId {
                owner: address.to_string(),
                number: 0,
            }),
            client_id,
            order_flags: 0,
            clob_pair_id: market_params.clob_pair_id,
        }),
        good_til_oneof: Some(GoodTilOneof::GoodTilBlock(
            height.0 + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
        )),
    };

    let chain_id = if is_mainnet {
        ChainId::Mainnet1
    } else {
        ChainId::Testnet4
    };
    let tx_builder = TxBuilder::new(chain_id, "adydx".to_string())?;
    let tx_raw = tx_builder.build_transaction(account, vec![msg_cancel.to_any()], None, None)?;
    let tx_hash = grpc.broadcast_tx(tx_raw.to_bytes()?).await?;

    Ok(tx_hash)
}

async fn fetch_open_orders(
    raw_http: &DydxRawHttpClient,
    address: &str,
) -> Result<Vec<DydxOrder>, Box<dyn std::error::Error>> {
    let orders = raw_http.get_orders(address, 0, None, None).await?;
    Ok(orders
        .into_iter()
        .filter(|o| matches!(o.status, DydxOrderStatus::Open))
        .collect())
}

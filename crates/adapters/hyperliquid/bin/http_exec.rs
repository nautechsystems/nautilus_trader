use std::{env, str::FromStr};

use nautilus_hyperliquid::http::{
    client::HyperliquidHttpClient,
    models::{
        HyperliquidExecAction, HyperliquidExecGrouping, HyperliquidExecLimitParams,
        HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
    },
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_target(false).with_env_filter(filter).init();

    let _ = env::var("HYPERLIQUID_TESTNET_PK")
        .expect("HYPERLIQUID_TESTNET_PK environment variable not set");

    tracing::info!("Starting Hyperliquid Testnet Order Placer");

    let client = match HyperliquidHttpClient::from_env() {
        Ok(client) => {
            tracing::info!("Client created (testnet: {})", client.is_testnet());
            client
        }
        Err(e) => {
            tracing::error!("Failed to create client: {}", e);
            return Err(e.into());
        }
    };

    tracing::info!("Fetching market metadata...");
    let meta = client.info_meta().await?;

    // Debug: Print all assets
    tracing::debug!("Available assets:");
    for (idx, asset) in meta.universe.iter().enumerate() {
        tracing::debug!(
            "  [{}] {} (sz_decimals: {})",
            idx,
            asset.name,
            asset.sz_decimals
        );
    }

    let btc_asset_id = meta
        .universe
        .iter()
        .position(|asset| asset.name == "BTC")
        .expect("BTC not found in universe");

    tracing::info!("BTC asset ID: {}", btc_asset_id);
    tracing::info!(
        "BTC sz_decimals: {}",
        meta.universe[btc_asset_id].sz_decimals
    );

    // Get the wallet address to verify authentication
    let wallet_address = client
        .get_user_address()
        .expect("Failed to get wallet address");
    tracing::info!("Wallet address: {}", wallet_address);

    // Check account state before placing order
    tracing::info!("Fetching account state...");
    match client.info_clearinghouse_state(&wallet_address).await {
        Ok(state) => {
            tracing::info!(
                "Account state: {}",
                serde_json::to_string_pretty(&state).unwrap_or_else(|_| "N/A".to_string())
            );
        }
        Err(e) => {
            tracing::warn!("Failed to fetch account state: {}", e);
        }
    }

    tracing::info!("Fetching BTC order book...");
    let book = client.info_l2_book("BTC").await?;

    let best_bid_str = &book.levels[0][0].px;
    let best_bid = Decimal::from_str(best_bid_str)?;

    tracing::info!("Best bid: ${}", best_bid);

    // BTC prices on Hyperliquid must be whole dollars (no decimal places)
    let limit_price = (best_bid * dec!(0.95)).round();
    tracing::info!("Limit order price: ${}", limit_price);

    let order = HyperliquidExecPlaceOrderRequest {
        asset: btc_asset_id as u32,
        is_buy: true,
        price: limit_price,
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Gtc,
            },
        },
        cloid: None,
    };

    tracing::info!("Order details:");
    tracing::info!("  Asset: {} (BTC)", btc_asset_id);
    tracing::info!("  Side: BUY");
    tracing::info!("  Price: ${}", limit_price);
    tracing::info!("  Size: 0.001 BTC");

    tracing::info!("Placing order...");

    // Create the action using the typed HyperliquidExecAction enum
    let action = HyperliquidExecAction::Order {
        orders: vec![order],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    };

    tracing::debug!("ExchangeAction: {:?}", action);

    // Also log the action as JSON
    if let Ok(action_json) = serde_json::to_value(&action) {
        tracing::debug!(
            "Action JSON: {}",
            serde_json::to_string_pretty(&action_json)?
        );
    }

    match client.post_action_exec(&action).await {
        Ok(response) => {
            tracing::info!("Order placed successfully!");
            tracing::info!("Response: {:#?}", response);

            // Also log as JSON for easier reading
            if let Ok(json) = serde_json::to_string_pretty(&response) {
                tracing::info!("Response JSON:\n{}", json);
            }
        }
        Err(e) => {
            tracing::error!("Failed to place order: {}", e);
            tracing::error!("Error details: {:?}", e);
            return Err(e.into());
        }
    }
    tracing::info!("Done!");
    Ok(())
}

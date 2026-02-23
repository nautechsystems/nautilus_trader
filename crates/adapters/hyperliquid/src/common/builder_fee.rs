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

//! Builder fee approval and verification functionality.
//!
//! Note: Hyperliquid uses non-standard EIP-712 type names with colons
//! (e.g., "HyperliquidTransaction:ApproveBuilderFee") which cannot be
//! represented using alloy's `sol!` macro. The struct hash is computed
//! manually while the domain uses alloy's `Eip712Domain`.

use std::{
    collections::HashMap,
    env,
    io::{self, Write},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime},
};

use alloy_primitives::{Address, B256, keccak256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::Eip712Domain;
use nautilus_network::http::{HttpClient, Method};
use serde::{Deserialize, Serialize};
use tabled::{Table, Tabled, settings::Style};

use super::consts::{
    HYPERLIQUID_CHAIN_ID, NAUTILUS_BUILDER_FEE_ADDRESS, NAUTILUS_BUILDER_FEE_MAKER_TENTHS_BP,
    NAUTILUS_BUILDER_FEE_TAKER_TENTHS_BP, exchange_url, info_url,
};
use crate::{
    common::credential::EvmPrivateKey,
    http::{
        error::Result,
        models::{HyperliquidExecBuilderFee, RESPONSE_STATUS_OK},
    },
};

/// Builder fee approval rate (0.01% = 1 basis point).
const BUILDER_CODES_APPROVAL_FEE_RATE: &str = "0.01%";

/// Resolves the builder maker fee tier from the Hyperliquid effective maker rate.
///
/// Maps `userAddRate` (effective maker rate including all discounts) to a
/// builder maker fee tier in tenths of a basis point. See [`FEE_TIERS`] for
/// the full volume-to-fee mapping.
#[must_use]
#[allow(clippy::bool_to_int_with_if)]
pub fn resolve_maker_tenths_bp(user_add_rate: f64) -> u32 {
    if user_add_rate > 0.000_12 {
        4
    } else if user_add_rate > 0.000_08 {
        3
    } else if user_add_rate > 0.000_04 {
        2
    } else if user_add_rate > 0.0 {
        1
    } else {
        0
    }
}

/// Resolves the builder fee for an order based on symbol and post-only flag.
///
/// Returns `None` for spot orders or when the resolved fee is zero.
/// For perps, uses the dynamic `maker_tenths_bp` when `post_only` is true,
/// otherwise the fixed taker rate.
#[must_use]
pub fn resolve_builder_fee(
    symbol: &str,
    post_only: bool,
    maker_tenths_bp: u32,
) -> Option<HyperliquidExecBuilderFee> {
    if symbol.ends_with("-SPOT") {
        return None;
    }

    let fee_tenths_bp = if post_only {
        maker_tenths_bp
    } else {
        NAUTILUS_BUILDER_FEE_TAKER_TENTHS_BP
    };

    if fee_tenths_bp == 0 {
        return None;
    }

    Some(HyperliquidExecBuilderFee {
        address: NAUTILUS_BUILDER_FEE_ADDRESS.to_string(),
        fee_tenths_bp,
    })
}

/// Resolves the builder fee for a batch of orders, using the lowest fee.
///
/// Returns `None` if any order is spot or resolves to zero fee. Uses the
/// dynamic `maker_tenths_bp` for post-only orders, otherwise the taker rate.
///
/// Hyperliquid applies a single builder fee per action (not per order), so
/// mixed post-only/taker batches use the minimum to avoid overcharging.
/// Mixed spot/perp batches cannot occur since `OrderList` enforces a single
/// instrument.
#[must_use]
pub fn resolve_builder_fee_batch(
    orders: &[(&str, bool)],
    maker_tenths_bp: u32,
) -> Option<HyperliquidExecBuilderFee> {
    let mut min: Option<HyperliquidExecBuilderFee> = None;

    for &(symbol, post_only) in orders {
        let fee = resolve_builder_fee(symbol, post_only, maker_tenths_bp)?;
        min = Some(match min {
            Some(current) if current.fee_tenths_bp <= fee.fee_tenths_bp => current,
            _ => fee,
        });
    }

    min
}

/// Information about the Nautilus builder fee configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderFeeInfo {
    /// The builder address that receives fees.
    pub address: String,
    /// Taker fee rate for perpetuals in tenths of a basis point.
    pub perp_taker_tenths_bp: u32,
    /// Maker fee rate for perpetuals in tenths of a basis point.
    pub perp_maker_tenths_bp: u32,
    /// The approval rate required.
    pub approval_rate: String,
}

impl Default for BuilderFeeInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl BuilderFeeInfo {
    /// Creates builder fee info from the hardcoded constants.
    #[must_use]
    pub fn new() -> Self {
        Self {
            address: NAUTILUS_BUILDER_FEE_ADDRESS.to_string(),
            perp_taker_tenths_bp: NAUTILUS_BUILDER_FEE_TAKER_TENTHS_BP,
            perp_maker_tenths_bp: NAUTILUS_BUILDER_FEE_MAKER_TENTHS_BP,
            approval_rate: BUILDER_CODES_APPROVAL_FEE_RATE.to_string(),
        }
    }

    /// Prints the builder fee configuration to stdout.
    pub fn print(&self) {
        let separator = "=".repeat(68);

        println!("{separator}");
        println!("NautilusTrader Hyperliquid Builder Fee Configuration");
        println!("{separator}");
        println!();
        println!("Builder address: {}", self.address);
        println!();
        println!("Fee rates (perpetuals only, no fee on spot):");
        println!(
            "  - Taker: {} bp ({:.4}%) [fixed]",
            self.perp_taker_tenths_bp as f64 / 10.0,
            self.perp_taker_tenths_bp as f64 / 1000.0,
        );
        println!(
            "  - Maker: {} bp ({:.4}%) [base, scales down with volume]",
            self.perp_maker_tenths_bp as f64 / 10.0,
            self.perp_maker_tenths_bp as f64 / 1000.0,
        );
        println!();
        print_fee_tier_table();
        println!();
        println!("Source: crates/adapters/hyperliquid/src/common/consts.rs");
        println!("{separator}");
    }
}

#[derive(Tabled)]
#[tabled(rename_all = "verbatim")]
struct FeeTierRow {
    #[tabled(rename = "14d Volume")]
    volume: &'static str,
    #[tabled(rename = "HL Maker Rate")]
    hl_maker: &'static str,
    #[tabled(rename = "HL Taker Rate")]
    hl_taker: &'static str,
    #[tabled(rename = "Builder Maker Fee")]
    builder_maker: &'static str,
    #[tabled(rename = "Builder Taker Fee")]
    builder_taker: &'static str,
}

#[rustfmt::skip]
const FEE_TIERS: [FeeTierRow; 5] = [
    FeeTierRow { volume: "Base",    hl_maker: "1.5 bp", hl_taker: "3.5 bp", builder_maker: "0.4 bp (4 tenths)", builder_taker: "1.0 bp" },
    FeeTierRow { volume: "> $5M",   hl_maker: "1.2 bp", hl_taker: "3.2 bp", builder_maker: "0.3 bp (3 tenths)", builder_taker: "1.0 bp" },
    FeeTierRow { volume: "> $25M",  hl_maker: "0.8 bp", hl_taker: "2.8 bp", builder_maker: "0.2 bp (2 tenths)", builder_taker: "1.0 bp" },
    FeeTierRow { volume: "> $100M", hl_maker: "0.4 bp", hl_taker: "2.2 bp", builder_maker: "0.1 bp (1 tenth)",  builder_taker: "1.0 bp" },
    FeeTierRow { volume: "> $500M", hl_maker: "0.0 bp", hl_taker: "1.5 bp", builder_maker: "0.0 bp (zero)",     builder_taker: "1.0 bp" },
];

fn print_fee_tier_table() {
    println!("The maker fee scales down with your Hyperliquid volume tier.");
    println!("At the highest tier, the builder maker fee is zero:");
    println!();

    let table = Table::new(&FEE_TIERS).with(Style::rounded()).to_string();
    for line in table.lines() {
        println!("  {line}");
    }

    println!();
    println!("These fees are charged in addition to Hyperliquid's standard fees.");
    println!("Maker fee tier is detected automatically from your HL volume tier.");
    println!();
    println!("Hyperliquid fees: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees");
    println!(
        "Builder codes: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes"
    );
}

/// Result of a builder fee approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderFeeApprovalResult {
    /// Whether the approval was successful.
    pub success: bool,
    /// The status returned by Hyperliquid.
    pub status: String,
    /// Optional response message or error details.
    pub message: Option<String>,
    /// The wallet address that made the approval.
    pub wallet_address: String,
    /// The builder address that was approved.
    pub builder_address: String,
    /// Whether this was on testnet.
    pub is_testnet: bool,
}

/// Approves the Nautilus builder fee for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action and submits it to Hyperliquid.
/// The approval allows NautilusTrader to include builder fees on orders for this wallet.
///
/// # Arguments
///
/// * `private_key` - The EVM private key (hex string with or without 0x prefix)
/// * `is_testnet` - Whether to use testnet or mainnet
///
/// # Returns
///
/// The result of the approval request.
///
/// # Errors
///
/// Returns an error if the private key is invalid, signing fails, or the HTTP request fails.
// Mutex/RwLock poisoning is not documented individually
#[allow(clippy::missing_panics_doc)]
pub async fn approve_builder_fee(
    private_key: &str,
    is_testnet: bool,
) -> Result<BuilderFeeApprovalResult> {
    let pk = EvmPrivateKey::new(private_key.to_string())?;
    let wallet_address = derive_address(&pk)?;

    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| crate::http::error::Error::transport(format!("Time error: {e}")))?
        .as_millis() as u64;

    let signature =
        sign_approve_builder_fee(&pk, is_testnet, nonce, BUILDER_CODES_APPROVAL_FEE_RATE)?;

    let action = serde_json::json!({
        "type": "approveBuilderFee",
        "hyperliquidChain": if is_testnet { "Testnet" } else { "Mainnet" },
        "signatureChainId": "0x66eee",
        "maxFeeRate": BUILDER_CODES_APPROVAL_FEE_RATE,
        "builder": NAUTILUS_BUILDER_FEE_ADDRESS,
        "nonce": nonce,
    });

    let payload = serde_json::json!({
        "action": action,
        "nonce": nonce,
        "signature": signature,
    });

    let url = exchange_url(is_testnet);
    let client =
        HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).map_err(|e| {
            crate::http::error::Error::transport(format!("Failed to create client: {e}"))
        })?;

    let body_bytes = serde_json::to_vec(&payload)
        .map_err(|e| crate::http::error::Error::transport(format!("Failed to serialize: {e}")))?;

    let headers = HashMap::from([("Content-Type".to_string(), "application/json".to_string())]);
    let response = client
        .request(
            Method::POST,
            url.to_string(),
            None,
            Some(headers),
            Some(body_bytes),
            None,
            None,
        )
        .await
        .map_err(|e| crate::http::error::Error::transport(format!("HTTP request failed: {e}")))?;

    if !response.status.is_success() {
        let body_str = String::from_utf8_lossy(&response.body);
        return Err(crate::http::error::Error::transport(format!(
            "HTTP {} from {url}: {}",
            response.status.as_u16(),
            if body_str.is_empty() {
                "(empty response)"
            } else {
                &body_str
            }
        )));
    }

    let response_json: serde_json::Value = serde_json::from_slice(&response.body).map_err(|e| {
        let body_str = String::from_utf8_lossy(&response.body);
        crate::http::error::Error::transport(format!(
            "Failed to parse JSON response from {url}: {e}. Body: {}",
            if body_str.is_empty() {
                "(empty)"
            } else if body_str.len() > 200 {
                &body_str[..200]
            } else {
                &body_str
            }
        ))
    })?;

    let status = response_json
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let success = status == RESPONSE_STATUS_OK;
    let message = response_json.get("response").map(|v: &serde_json::Value| {
        if v.is_string() {
            v.as_str().unwrap().to_string()
        } else {
            v.to_string()
        }
    });

    Ok(BuilderFeeApprovalResult {
        success,
        status,
        message,
        wallet_address,
        builder_address: NAUTILUS_BUILDER_FEE_ADDRESS.to_string(),
        is_testnet,
    })
}

/// Approves the Nautilus builder fee using environment variables.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// Prints progress and results to stdout.
///
/// # Arguments
///
/// * `non_interactive` - If true, skip confirmation prompt
///
/// # Returns
///
/// `true` if approval succeeded, `false` otherwise.
pub async fn approve_from_env(non_interactive: bool) -> bool {
    let is_testnet = env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true");

    let env_var = if is_testnet {
        "HYPERLIQUID_TESTNET_PK"
    } else {
        "HYPERLIQUID_PK"
    };

    let private_key = match env::var(env_var) {
        Ok(pk) => pk,
        Err(_) => {
            println!("Error: {env_var} environment variable not set");
            return false;
        }
    };

    let info = BuilderFeeInfo::new();
    let network = if is_testnet { "testnet" } else { "mainnet" };

    println!("Approving Nautilus builder fee on {network}");
    println!("Builder address: {}", info.address);
    println!(
        "Approval rate: 1.0 bp ({}) ceiling, covers perpetual taker and maker fills",
        info.approval_rate
    );
    println!("  - Taker: 1.0 bp (0.01%) on perpetual fills [fixed]");
    println!("  - Maker: 0.4 bp (0.004%) base, scales down with volume");
    println!("  - Spot: no builder fee");
    println!();
    print_fee_tier_table();
    println!();

    if !non_interactive && !wait_for_confirmation("Press Enter to approve or Ctrl+C to cancel... ")
    {
        return false;
    }

    println!("Approving builder fee...");

    match approve_builder_fee(&private_key, is_testnet).await {
        Ok(result) => {
            println!();
            println!("Wallet address: {}", result.wallet_address);
            println!("Status: {}", result.status);
            if let Some(msg) = &result.message {
                println!("Response: {msg}");
            }
            println!();

            if result.success {
                println!("Builder fee approved successfully.");
                println!();
                println!("To verify approval status at any time, run:");
                println!(
                    "  python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py"
                );
            } else {
                println!("Approval may have failed. Check the response above.");
            }

            result.success
        }
        Err(e) => {
            println!("Error: {e}");
            false
        }
    }
}

/// Revoke fee rate (0% effectively blocks the builder).
const REVOKE_FEE_RATE: &str = "0%";

/// Revokes the Nautilus builder fee approval for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action with a 0% rate and submits
/// it to Hyperliquid, effectively revoking the builder's permission.
///
/// # Arguments
///
/// * `private_key` - The EVM private key (hex string with or without 0x prefix)
/// * `is_testnet` - Whether to use testnet or mainnet
///
/// # Returns
///
/// The result of the revoke request.
///
// Mutex/RwLock poisoning is not documented individually
#[allow(clippy::missing_panics_doc)]
pub async fn revoke_builder_fee(
    private_key: &str,
    is_testnet: bool,
) -> Result<BuilderFeeApprovalResult> {
    let pk = EvmPrivateKey::new(private_key.to_string())?;
    let wallet_address = derive_address(&pk)?;

    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| crate::http::error::Error::transport(format!("Time error: {e}")))?
        .as_millis() as u64;

    let signature = sign_approve_builder_fee(&pk, is_testnet, nonce, REVOKE_FEE_RATE)?;

    let action = serde_json::json!({
        "type": "approveBuilderFee",
        "hyperliquidChain": if is_testnet { "Testnet" } else { "Mainnet" },
        "signatureChainId": "0x66eee",
        "maxFeeRate": REVOKE_FEE_RATE,
        "builder": NAUTILUS_BUILDER_FEE_ADDRESS,
        "nonce": nonce,
    });

    let payload = serde_json::json!({
        "action": action,
        "nonce": nonce,
        "signature": signature,
    });

    let url = exchange_url(is_testnet);
    let client =
        HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).map_err(|e| {
            crate::http::error::Error::transport(format!("Failed to create client: {e}"))
        })?;

    let body_bytes = serde_json::to_vec(&payload)
        .map_err(|e| crate::http::error::Error::transport(format!("Failed to serialize: {e}")))?;

    let headers = HashMap::from([("Content-Type".to_string(), "application/json".to_string())]);
    let response = client
        .request(
            Method::POST,
            url.to_string(),
            None,
            Some(headers),
            Some(body_bytes),
            None,
            None,
        )
        .await
        .map_err(|e| crate::http::error::Error::transport(format!("HTTP request failed: {e}")))?;

    if !response.status.is_success() {
        let body_str = String::from_utf8_lossy(&response.body);
        return Err(crate::http::error::Error::transport(format!(
            "HTTP {} from {url}: {}",
            response.status.as_u16(),
            if body_str.is_empty() {
                "(empty response)"
            } else {
                &body_str
            }
        )));
    }

    let response_json: serde_json::Value = serde_json::from_slice(&response.body).map_err(|e| {
        let body_str = String::from_utf8_lossy(&response.body);
        crate::http::error::Error::transport(format!(
            "Failed to parse JSON response from {url}: {e}. Body: {}",
            if body_str.is_empty() {
                "(empty)"
            } else if body_str.len() > 200 {
                &body_str[..200]
            } else {
                &body_str
            }
        ))
    })?;

    let status = response_json
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let success = status == RESPONSE_STATUS_OK;
    let message = response_json.get("response").map(|v: &serde_json::Value| {
        if v.is_string() {
            v.as_str().unwrap().to_string()
        } else {
            v.to_string()
        }
    });

    Ok(BuilderFeeApprovalResult {
        success,
        status,
        message,
        wallet_address,
        builder_address: NAUTILUS_BUILDER_FEE_ADDRESS.to_string(),
        is_testnet,
    })
}

/// Revokes the Nautilus builder fee using environment variables.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// Prints progress and results to stdout.
///
/// # Arguments
///
/// * `non_interactive` - If true, skip confirmation prompt
///
/// # Returns
///
/// `true` if revocation succeeded, `false` otherwise.
pub async fn revoke_from_env(non_interactive: bool) -> bool {
    let is_testnet = env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true");

    let env_var = if is_testnet {
        "HYPERLIQUID_TESTNET_PK"
    } else {
        "HYPERLIQUID_PK"
    };

    let private_key = match env::var(env_var) {
        Ok(pk) => pk,
        Err(_) => {
            println!("Error: {env_var} environment variable not set");
            return false;
        }
    };

    let network = if is_testnet { "testnet" } else { "mainnet" };

    println!("Revoking Nautilus builder fee on {network}");
    println!("Builder address: {NAUTILUS_BUILDER_FEE_ADDRESS}");
    println!();
    println!("WARNING: After revoking, you will not be able to trade on");
    println!("Hyperliquid via NautilusTrader until you re-approve.");
    println!();

    if !non_interactive && !wait_for_confirmation("Press Enter to revoke or Ctrl+C to cancel... ") {
        return false;
    }

    println!("Revoking builder fee...");

    match revoke_builder_fee(&private_key, is_testnet).await {
        Ok(result) => {
            println!();
            println!("Wallet address: {}", result.wallet_address);
            println!("Status: {}", result.status);
            if let Some(msg) = &result.message {
                println!("Response: {msg}");
            }
            println!();

            if result.success {
                println!("Builder fee revoked successfully.");
                println!("You will need to re-approve to trade via NautilusTrader.");
            } else {
                println!("Revocation may have failed. Check the response above.");
            }

            result.success
        }
        Err(e) => {
            println!("Error: {e}");
            false
        }
    }
}

/// Result of a builder fee verification query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderFeeVerifyResult {
    /// The wallet address that was checked.
    pub wallet_address: String,
    /// The builder address that was checked.
    pub builder_address: String,
    /// The approved fee rate as a string (e.g., "1%"), or None if not approved.
    pub approved_rate: Option<String>,
    /// The required fee rate for NautilusTrader.
    pub required_rate: String,
    /// Whether the approval is sufficient.
    pub is_approved: bool,
    /// Whether this was on testnet.
    pub is_testnet: bool,
}

/// Verifies builder fee approval status for a wallet.
///
/// Queries the Hyperliquid `maxBuilderFee` info endpoint to check if the
/// wallet has approved the Nautilus builder fee at the required rate.
///
/// # Arguments
///
/// * `wallet_address` - The wallet address to check (hex string with 0x prefix)
/// * `is_testnet` - Whether to use testnet or mainnet
///
/// # Returns
///
/// The verification result including approval status.
pub async fn verify_builder_fee(
    wallet_address: &str,
    is_testnet: bool,
) -> Result<BuilderFeeVerifyResult> {
    let url = info_url(is_testnet);
    let client =
        HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).map_err(|e| {
            crate::http::error::Error::transport(format!("Failed to create client: {e}"))
        })?;

    let payload = serde_json::json!({
        "type": "maxBuilderFee",
        "user": wallet_address,
        "builder": NAUTILUS_BUILDER_FEE_ADDRESS,
    });

    let body_bytes = serde_json::to_vec(&payload)
        .map_err(|e| crate::http::error::Error::transport(format!("Failed to serialize: {e}")))?;

    let headers = HashMap::from([("Content-Type".to_string(), "application/json".to_string())]);
    let response = client
        .request(
            Method::POST,
            url.to_string(),
            None,
            Some(headers),
            Some(body_bytes),
            None,
            None,
        )
        .await
        .map_err(|e| crate::http::error::Error::transport(format!("HTTP request failed: {e}")))?;

    if !response.status.is_success() {
        let body_str = String::from_utf8_lossy(&response.body);
        return Err(crate::http::error::Error::transport(format!(
            "HTTP {} from {url}: {}",
            response.status.as_u16(),
            if body_str.is_empty() {
                "(empty response)"
            } else {
                &body_str
            }
        )));
    }

    // API returns fee in tenths of basis points (e.g., 1000 = 1%) or "null"
    let response_text = String::from_utf8_lossy(&response.body).trim().to_string();
    let approved_tenths_bp: Option<u32> = if response_text == "null" {
        None
    } else {
        response_text.parse().ok()
    };

    let approved_rate = approved_tenths_bp.map(|tenths| {
        let bps = tenths as f64 / 10.0;
        let percent = bps / 100.0;
        format!("{percent}%")
    });
    let is_approved = approved_tenths_bp.is_some_and(|tenths| tenths >= 10);

    Ok(BuilderFeeVerifyResult {
        wallet_address: wallet_address.to_string(),
        builder_address: NAUTILUS_BUILDER_FEE_ADDRESS.to_string(),
        approved_rate,
        required_rate: BUILDER_CODES_APPROVAL_FEE_RATE.to_string(),
        is_approved,
        is_testnet,
    })
}

/// Verifies builder fee approval using an optional wallet address or environment variables.
///
/// If `wallet_address` is provided, uses it directly. Otherwise reads private key
/// from environment to derive wallet address:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// Prints verification results to stdout.
///
/// # Returns
///
/// `true` if builder fee is approved at the required rate, `false` otherwise.
pub async fn verify_from_env_or_address(wallet_address: Option<String>) -> bool {
    let is_testnet = env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true");

    let wallet_address = match wallet_address {
        Some(addr) => addr,
        None => {
            // Fall back to deriving from private key
            let env_var = if is_testnet {
                "HYPERLIQUID_TESTNET_PK"
            } else {
                "HYPERLIQUID_PK"
            };

            let private_key = match env::var(env_var) {
                Ok(pk) => pk,
                Err(_) => {
                    println!("Error: No wallet address provided and {env_var} not set");
                    return false;
                }
            };

            let pk = match EvmPrivateKey::new(private_key) {
                Ok(pk) => pk,
                Err(e) => {
                    println!("Error: Invalid private key: {e}");
                    return false;
                }
            };

            match derive_address(&pk) {
                Ok(addr) => addr,
                Err(e) => {
                    println!("Error: Failed to derive address: {e}");
                    return false;
                }
            }
        }
    };

    let network = if is_testnet { "testnet" } else { "mainnet" };
    let separator = "=".repeat(60);

    println!("{separator}");
    println!("Hyperliquid Builder Fee Verification");
    println!("{separator}");
    println!();
    println!("Checking approval status on {network}...");
    println!();

    match verify_builder_fee(&wallet_address, is_testnet).await {
        Ok(result) => {
            println!("Wallet:   {}", result.wallet_address);
            println!("Builder:  {}", result.builder_address);
            println!("Network:  {network}");
            println!(
                "Approved: {}",
                result.approved_rate.as_deref().unwrap_or("(none)")
            );
            println!();

            if result.is_approved {
                println!("Status: APPROVED");
                println!();
                println!("NautilusTrader builder fee rates (perpetuals only, no fee on spot):");
                println!("  - Taker: 1.0 bp (0.01%) on perpetual fills [fixed]");
                println!("  - Maker: 0.4 bp (0.004%) base, scales down with volume");
            } else {
                println!("Status: NOT APPROVED");
                println!();
                println!("Run the approval script:");
                println!(
                    "  python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_approve.py"
                );
                println!();
                println!("See: docs/integrations/hyperliquid.md#approving-builder-fees");
            }

            println!("{separator}");
            result.is_approved
        }
        Err(e) => {
            println!("Error: {e}");
            false
        }
    }
}

fn sign_approve_builder_fee(
    pk: &EvmPrivateKey,
    is_testnet: bool,
    nonce: u64,
    fee_rate: &str,
) -> Result<serde_json::Value> {
    // EIP-712 domain separator hash (using alloy's Eip712Domain)
    let domain_hash = compute_domain_hash();

    // Struct type hash for HyperliquidTransaction:ApproveBuilderFee
    let type_hash = keccak256(
        b"HyperliquidTransaction:ApproveBuilderFee(string hyperliquidChain,string maxFeeRate,address builder,uint64 nonce)",
    );

    // Hash the message fields
    let chain_str = if is_testnet { "Testnet" } else { "Mainnet" };
    let chain_hash = keccak256(chain_str.as_bytes());
    let fee_rate_hash = keccak256(fee_rate.as_bytes());

    // Parse builder address
    let builder_addr = Address::from_str(NAUTILUS_BUILDER_FEE_ADDRESS).map_err(|e| {
        crate::http::error::Error::transport(format!("Invalid builder address: {e}"))
    })?;

    // Encode the struct hash
    let mut struct_data = Vec::with_capacity(32 * 5);
    struct_data.extend_from_slice(type_hash.as_slice());
    struct_data.extend_from_slice(chain_hash.as_slice());
    struct_data.extend_from_slice(fee_rate_hash.as_slice());

    // Address is padded to 32 bytes (left-padded with zeros)
    let mut addr_bytes = [0u8; 32];
    addr_bytes[12..].copy_from_slice(builder_addr.as_slice());
    struct_data.extend_from_slice(&addr_bytes);

    // Nonce is uint64, padded to 32 bytes (left-padded with zeros)
    let mut nonce_bytes = [0u8; 32];
    nonce_bytes[24..].copy_from_slice(&nonce.to_be_bytes());
    struct_data.extend_from_slice(&nonce_bytes);

    let struct_hash = keccak256(&struct_data);

    // Create final EIP-712 hash: \x19\x01 + domain_hash + struct_hash
    let mut final_data = Vec::with_capacity(66);
    final_data.extend_from_slice(b"\x19\x01");
    final_data.extend_from_slice(&domain_hash);
    final_data.extend_from_slice(struct_hash.as_slice());

    let signing_hash = keccak256(&final_data);

    // Sign the hash
    let key_hex = pk.as_hex();
    let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

    let signer = PrivateKeySigner::from_str(key_hex).map_err(|e| {
        crate::http::error::Error::transport(format!("Failed to create signer: {e}"))
    })?;

    let hash_b256 = B256::from(signing_hash);
    let signature = signer
        .sign_hash_sync(&hash_b256)
        .map_err(|e| crate::http::error::Error::transport(format!("Failed to sign: {e}")))?;

    // Format signature as {r, s, v} for Hyperliquid
    let r = format!("0x{:064x}", signature.r());
    let s = format!("0x{:064x}", signature.s());
    let v = if signature.v() { 28u8 } else { 27u8 };

    Ok(serde_json::json!({
        "r": r,
        "s": s,
        "v": v,
    }))
}

fn get_eip712_domain() -> Eip712Domain {
    Eip712Domain {
        name: Some("HyperliquidSignTransaction".into()),
        version: Some("1".into()),
        chain_id: Some(alloy_primitives::U256::from(HYPERLIQUID_CHAIN_ID)),
        verifying_contract: Some(Address::ZERO),
        salt: None,
    }
}

fn compute_domain_hash() -> [u8; 32] {
    *get_eip712_domain().hash_struct()
}

fn derive_address(pk: &EvmPrivateKey) -> Result<String> {
    let key_hex = pk.as_hex();
    let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

    let signer = PrivateKeySigner::from_str(key_hex).map_err(|e| {
        crate::http::error::Error::transport(format!("Failed to create signer: {e}"))
    })?;

    Ok(format!("{:#x}", signer.address()))
}

fn wait_for_confirmation(prompt: &str) -> bool {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    if ctrlc::set_handler(move || {
        cancelled_clone.store(true, Ordering::SeqCst);
    })
    .is_err()
    {
        // Handler already set, continue without it
    }

    print!("{prompt}");
    io::stdout().flush().ok();

    // Spawn thread to read stdin so we can check for ctrlc
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let result = io::stdin().read_line(&mut input);
        let _ = tx.send(result);
    });

    // Wait for either input or ctrlc
    loop {
        if cancelled.load(Ordering::SeqCst) {
            println!();
            println!("Aborted.");
            return false;
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(0) | Err(_)) => {
                println!();
                println!("Aborted.");
                return false;
            }
            Ok(Ok(_)) => {
                println!();
                return true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!();
                println!("Aborted.");
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_builder_fee_info() {
        let info = BuilderFeeInfo::new();
        assert_eq!(info.address, NAUTILUS_BUILDER_FEE_ADDRESS);
        assert_eq!(info.perp_taker_tenths_bp, 10);
        assert_eq!(info.perp_maker_tenths_bp, 4);
        assert_eq!(info.approval_rate, "0.01%");
    }

    #[rstest]
    #[case(0.00020, 4)] // Well above base
    #[case(0.00015, 4)] // At HL base rate
    #[case(0.00013, 4)] // Just above threshold
    #[case(0.00012, 3)] // At threshold (not above)
    #[case(0.00010, 3)] // Between thresholds
    #[case(0.00008, 2)] // At second threshold
    #[case(0.00006, 2)] // Between thresholds
    #[case(0.00004, 1)] // At third threshold
    #[case(0.00002, 1)] // Between thresholds
    #[case(0.00001, 1)] // Just above zero
    #[case(0.0, 0)] // Zero rate (> $500M volume)
    #[case(-0.001, 0)] // Negative rate treated as zero tier
    fn test_resolve_maker_tenths_bp(#[case] rate: f64, #[case] expected: u32) {
        assert_eq!(resolve_maker_tenths_bp(rate), expected);
    }

    #[rstest]
    fn test_resolve_builder_fee_perp_taker() {
        let fee = resolve_builder_fee("BTC-PERP", false, 4).unwrap();
        assert_eq!(fee.fee_tenths_bp, 10);
        assert_eq!(fee.address, NAUTILUS_BUILDER_FEE_ADDRESS);
    }

    #[rstest]
    #[case(4, Some(4))]
    #[case(3, Some(3))]
    #[case(2, Some(2))]
    #[case(1, Some(1))]
    #[case(0, None)]
    fn test_resolve_builder_fee_perp_post_only(
        #[case] maker_tenths: u32,
        #[case] expected_fee: Option<u32>,
    ) {
        let result = resolve_builder_fee("BTC-PERP", true, maker_tenths);
        assert_eq!(result.map(|f| f.fee_tenths_bp), expected_fee);
    }

    #[rstest]
    fn test_resolve_builder_fee_spot_returns_none() {
        assert!(resolve_builder_fee("BTC-SPOT", false, 4).is_none());
        assert!(resolve_builder_fee("BTC-SPOT", true, 4).is_none());
    }

    #[rstest]
    fn test_resolve_builder_fee_batch_all_taker() {
        let orders = vec![("BTC-PERP", false), ("BTC-PERP", false)];
        let fee = resolve_builder_fee_batch(&orders, 4).unwrap();
        assert_eq!(fee.fee_tenths_bp, 10);
    }

    #[rstest]
    fn test_resolve_builder_fee_batch_mixed_uses_minimum() {
        let orders = vec![("BTC-PERP", false), ("BTC-PERP", true)];
        let fee = resolve_builder_fee_batch(&orders, 3).unwrap();
        assert_eq!(fee.fee_tenths_bp, 3);
    }

    #[rstest]
    fn test_resolve_builder_fee_batch_post_only_zero_returns_none() {
        let orders = vec![("BTC-PERP", true), ("BTC-PERP", true)];
        assert!(resolve_builder_fee_batch(&orders, 0).is_none());
    }

    #[rstest]
    fn test_resolve_builder_fee_batch_empty_returns_none() {
        assert!(resolve_builder_fee_batch(&[], 4).is_none());
    }

    #[rstest]
    fn test_resolve_builder_fee_perp_taker_ignores_maker_tier() {
        // Taker fee should be fixed regardless of maker tier
        let fee_at_base = resolve_builder_fee("BTC-PERP", false, 4).unwrap();
        let fee_at_zero = resolve_builder_fee("BTC-PERP", false, 0).unwrap();
        assert_eq!(
            fee_at_base.fee_tenths_bp,
            NAUTILUS_BUILDER_FEE_TAKER_TENTHS_BP
        );
        assert_eq!(
            fee_at_zero.fee_tenths_bp,
            NAUTILUS_BUILDER_FEE_TAKER_TENTHS_BP
        );
    }

    #[rstest]
    fn test_derive_address() {
        let pk = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let addr = derive_address(&pk).unwrap();
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42);
    }

    #[rstest]
    fn test_compute_domain_hash() {
        let hash = compute_domain_hash();
        assert_eq!(hash.len(), 32);
    }

    #[rstest]
    fn test_sign_approve_builder_fee() {
        let pk = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let nonce = 1640995200000u64;

        let signature =
            sign_approve_builder_fee(&pk, false, nonce, BUILDER_CODES_APPROVAL_FEE_RATE).unwrap();

        assert!(signature.get("r").is_some());
        assert!(signature.get("s").is_some());
        assert!(signature.get("v").is_some());

        let r = signature["r"].as_str().unwrap();
        let s = signature["s"].as_str().unwrap();

        assert!(r.starts_with("0x"));
        assert!(s.starts_with("0x"));
        assert_eq!(r.len(), 66); // 0x + 64 hex chars
        assert_eq!(s.len(), 66);
    }
}

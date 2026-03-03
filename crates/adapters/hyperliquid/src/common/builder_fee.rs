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

//! Builder fee revocation for Hyperliquid.
//!
//! Allows users to revoke previously-approved builder fee approvals.

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

use super::consts::{HYPERLIQUID_CHAIN_ID, NAUTILUS_BUILDER_ADDRESS, exchange_url};
use crate::{
    common::credential::EvmPrivateKey,
    http::{error::Result, models::RESPONSE_STATUS_OK},
};

// Revoke fee rate (0% effectively blocks the builder)
const REVOKE_FEE_RATE: &str = "0%";

/// Result of a builder fee approval or revocation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderFeeApprovalResult {
    /// Whether the request was successful.
    pub success: bool,
    /// The status returned by Hyperliquid.
    pub status: String,
    /// Optional response message or error details.
    pub message: Option<String>,
    /// The wallet address that made the request.
    pub wallet_address: String,
    /// The builder address.
    pub builder_address: String,
    /// Whether this was on testnet.
    pub is_testnet: bool,
}

/// Revokes the Nautilus builder fee approval for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action with a 0% rate and submits
/// it to Hyperliquid, effectively revoking the builder's permission.
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
        "builder": NAUTILUS_BUILDER_ADDRESS,
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
        builder_address: NAUTILUS_BUILDER_ADDRESS.to_string(),
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
    println!("Builder address: {NAUTILUS_BUILDER_ADDRESS}");
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

fn sign_approve_builder_fee(
    pk: &EvmPrivateKey,
    is_testnet: bool,
    nonce: u64,
    fee_rate: &str,
) -> Result<serde_json::Value> {
    let domain_hash = compute_domain_hash();

    // Struct type hash for HyperliquidTransaction:ApproveBuilderFee
    let type_hash = keccak256(
        b"HyperliquidTransaction:ApproveBuilderFee(string hyperliquidChain,string maxFeeRate,address builder,uint64 nonce)",
    );

    let chain_str = if is_testnet { "Testnet" } else { "Mainnet" };
    let chain_hash = keccak256(chain_str.as_bytes());
    let fee_rate_hash = keccak256(fee_rate.as_bytes());

    let builder_addr = Address::from_str(NAUTILUS_BUILDER_ADDRESS).map_err(|e| {
        crate::http::error::Error::transport(format!("Invalid builder address: {e}"))
    })?;

    let mut struct_data = Vec::with_capacity(32 * 5);
    struct_data.extend_from_slice(type_hash.as_slice());
    struct_data.extend_from_slice(chain_hash.as_slice());
    struct_data.extend_from_slice(fee_rate_hash.as_slice());

    // Address left-padded to 32 bytes
    let mut addr_bytes = [0u8; 32];
    addr_bytes[12..].copy_from_slice(builder_addr.as_slice());
    struct_data.extend_from_slice(&addr_bytes);

    // Nonce as uint64, left-padded to 32 bytes
    let mut nonce_bytes = [0u8; 32];
    nonce_bytes[24..].copy_from_slice(&nonce.to_be_bytes());
    struct_data.extend_from_slice(&nonce_bytes);

    let struct_hash = keccak256(&struct_data);

    // EIP-712 hash: \x19\x01 + domain_hash + struct_hash
    let mut final_data = Vec::with_capacity(66);
    final_data.extend_from_slice(b"\x19\x01");
    final_data.extend_from_slice(&domain_hash);
    final_data.extend_from_slice(struct_hash.as_slice());

    let signing_hash = keccak256(&final_data);

    let key_hex = pk.as_hex();
    let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

    let signer = PrivateKeySigner::from_str(key_hex).map_err(|e| {
        crate::http::error::Error::transport(format!("Failed to create signer: {e}"))
    })?;

    let hash_b256 = B256::from(signing_hash);
    let signature = signer
        .sign_hash_sync(&hash_b256)
        .map_err(|e| crate::http::error::Error::transport(format!("Failed to sign: {e}")))?;

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

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let result = io::stdin().read_line(&mut input);
        let _ = tx.send(result);
    });

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
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!();
                println!("Aborted.");
                return false;
            }
        }
    }
}

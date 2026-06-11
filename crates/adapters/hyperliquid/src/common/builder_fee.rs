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

//! Builder fee approval and revocation for Hyperliquid.
//!
//! Hyperliquid rejects orders that carry a builder address from a wallet that has
//! never approved a builder fee, even when the order fee is zero. This module signs
//! the one-time EIP-712 `ApproveBuilderFee` action at a 0% max fee rate, enabling
//! the zero-fee Nautilus builder attribution without ever charging a fee.
//!
//! Revocation signs the same action at the same 0% rate: it caps any previously
//! approved builder fee at zero (for example, an approval from a version that
//! charged builder fees).
//!
//! The action must be signed by the master wallet's private key; agent (API)
//! wallets cannot sign `ApproveBuilderFee`.

use std::{
    collections::HashMap,
    env,
    io::{self, Write},
    str::FromStr,
    time::SystemTime,
};

use alloy::{
    signers::{SignerSync, local::PrivateKeySigner},
    sol_types::eip712_domain,
};
use alloy_primitives::{Address, B256, keccak256};
use nautilus_network::http::{HttpClient, Method};
use serde::{Deserialize, Serialize};

use super::{
    consts::{HYPERLIQUID_CHAIN_ID, NAUTILUS_BUILDER_ADDRESS, exchange_url},
    enums::HyperliquidEnvironment,
};
use crate::{
    common::credential::EvmPrivateKey,
    http::{
        error::{Error, Result},
        models::{HyperliquidSignature, RESPONSE_STATUS_OK},
    },
};

// Zero max fee rate: approval enables attribution without ever permitting a
// charge, revocation caps any previously approved rate at zero.
const ZERO_FEE_RATE: &str = "0%";

/// Result of a builder fee approval request.
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

/// Approves the Nautilus builder fee using environment variables.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
pub async fn approve_from_env(non_interactive: bool) -> bool {
    let is_testnet = testnet_from_env();
    let Some(private_key) = private_key_from_env(is_testnet) else {
        return false;
    };
    let network = if is_testnet { "testnet" } else { "mainnet" };

    println!("Approving Nautilus builder attribution on {network}");
    println!("Builder address: {NAUTILUS_BUILDER_ADDRESS}");
    println!("Max fee rate: {ZERO_FEE_RATE} (attribution only, no fees are charged)");
    println!();
    println!("This signs a one-time ApproveBuilderFee action so orders can carry the");
    println!("Nautilus builder address. The action must be signed by the master wallet.");
    println!();

    if !non_interactive
        && !wait_for_confirmation("Press Enter to approve or Ctrl+C to cancel... ").await
    {
        return false;
    }

    println!("Approving builder fee...");

    report_result(
        approve_builder_fee(&private_key, is_testnet).await,
        "Builder fee approved successfully.",
        "Approval may have failed. Check the response above.",
    )
}

/// Revokes the Nautilus builder fee using environment variables.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
pub async fn revoke_from_env(non_interactive: bool) -> bool {
    let is_testnet = testnet_from_env();
    let Some(private_key) = private_key_from_env(is_testnet) else {
        return false;
    };
    let network = if is_testnet { "testnet" } else { "mainnet" };

    println!("Revoking Nautilus builder fee on {network}");
    println!("Builder address: {NAUTILUS_BUILDER_ADDRESS}");
    println!();

    if !non_interactive
        && !wait_for_confirmation("Press Enter to revoke or Ctrl+C to cancel... ").await
    {
        return false;
    }

    println!("Revoking builder fee...");

    report_result(
        revoke_builder_fee(&private_key, is_testnet).await,
        "Builder fee revoked successfully.",
        "Revocation may have failed. Check the response above.",
    )
}

/// Approves the Nautilus builder fee for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action with a 0% max fee rate and
/// submits it to Hyperliquid, permitting the zero-fee builder attribution.
///
/// # Errors
///
/// Returns an error if the private key is invalid, signing fails, or the
/// request cannot be submitted.
pub async fn approve_builder_fee(
    private_key: &str,
    is_testnet: bool,
) -> Result<BuilderFeeApprovalResult> {
    submit_builder_fee_update(private_key, is_testnet).await
}

/// Revokes the Nautilus builder fee approval for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action with a 0% max fee rate and
/// submits it to Hyperliquid, capping any previously approved builder fee at
/// zero so no fee can be charged.
///
/// # Errors
///
/// Returns an error if the private key is invalid, signing fails, or the
/// request cannot be submitted.
pub async fn revoke_builder_fee(
    private_key: &str,
    is_testnet: bool,
) -> Result<BuilderFeeApprovalResult> {
    submit_builder_fee_update(private_key, is_testnet).await
}

fn testnet_from_env() -> bool {
    env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true" || v == "1")
}

fn private_key_from_env(is_testnet: bool) -> Option<String> {
    let env_var = if is_testnet {
        "HYPERLIQUID_TESTNET_PK"
    } else {
        "HYPERLIQUID_PK"
    };

    match env::var(env_var) {
        Ok(pk) => Some(pk),
        Err(_) => {
            println!("Error: {env_var} environment variable not set");
            None
        }
    }
}

fn report_result(
    result: Result<BuilderFeeApprovalResult>,
    success_msg: &str,
    failure_msg: &str,
) -> bool {
    match result {
        Ok(result) => {
            println!();
            println!("Wallet address: {}", result.wallet_address);
            println!("Status: {}", result.status);
            if let Some(msg) = &result.message {
                println!("Response: {msg}");
            }
            println!();

            if result.success {
                println!("{success_msg}");
            } else {
                println!("{failure_msg}");
            }

            result.success
        }
        Err(e) => {
            println!("Error: {e}");
            false
        }
    }
}

async fn submit_builder_fee_update(
    private_key: &str,
    is_testnet: bool,
) -> Result<BuilderFeeApprovalResult> {
    let pk = EvmPrivateKey::new(private_key)?;
    let wallet_address = derive_address(&pk)?;

    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| Error::transport(format!("Time error: {e}")))?
        .as_millis() as u64;

    let signature = sign_approve_builder_fee(&pk, is_testnet, nonce, ZERO_FEE_RATE)?;
    let action = build_approval_action(is_testnet, nonce);

    let payload = serde_json::json!({
        "action": action,
        "nonce": nonce,
        "signature": signature,
    });

    let environment = if is_testnet {
        HyperliquidEnvironment::Testnet
    } else {
        HyperliquidEnvironment::Mainnet
    };
    let url = exchange_url(environment);

    let client = HttpClient::new(HashMap::new(), vec![], vec![], None, Some(60), None)
        .map_err(|e| Error::transport(format!("Failed to create client: {e}")))?;

    let body_bytes = serde_json::to_vec(&payload)
        .map_err(|e| Error::transport(format!("Failed to serialize: {e}")))?;

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
        .map_err(|e| Error::transport(format!("HTTP request failed: {e}")))?;

    if !response.status.is_success() {
        let body_str = String::from_utf8_lossy(&response.body);
        return Err(Error::transport(format!(
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
        let preview: String = body_str.chars().take(200).collect();
        Error::transport(format!(
            "Failed to parse JSON response from {url}: {e}. Body: {}",
            if preview.is_empty() {
                "(empty)"
            } else {
                &preview
            }
        ))
    })?;

    let status = response_json
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let success = status == RESPONSE_STATUS_OK;
    let message = response_json.get("response").map(|v| match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string(),
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

fn build_approval_action(is_testnet: bool, nonce: u64) -> serde_json::Value {
    serde_json::json!({
        "type": "approveBuilderFee",
        "hyperliquidChain": if is_testnet { "Testnet" } else { "Mainnet" },
        "signatureChainId": format!("{HYPERLIQUID_CHAIN_ID:#x}"),
        "maxFeeRate": ZERO_FEE_RATE,
        "builder": NAUTILUS_BUILDER_ADDRESS,
        "nonce": nonce,
    })
}

fn sign_approve_builder_fee(
    pk: &EvmPrivateKey,
    is_testnet: bool,
    nonce: u64,
    fee_rate: &str,
) -> Result<HyperliquidSignature> {
    let signing_hash = approval_signing_hash(is_testnet, nonce, fee_rate)?;

    let key_hex = pk.as_hex();
    let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

    let signer = PrivateKeySigner::from_str(key_hex)
        .map_err(|e| Error::auth(format!("Failed to create signer: {e}")))?;

    let signature = signer
        .sign_hash_sync(&signing_hash)
        .map_err(|e| Error::auth(format!("Failed to sign: {e}")))?;

    let r = format!("0x{:064x}", signature.r());
    let s = format!("0x{:064x}", signature.s());
    let v = if signature.v() { 28u64 } else { 27u64 };

    Ok(HyperliquidSignature::new(r, s, v))
}

fn approval_signing_hash(is_testnet: bool, nonce: u64, fee_rate: &str) -> Result<B256> {
    let domain = eip712_domain! {
        name: "HyperliquidSignTransaction",
        version: "1",
        chain_id: HYPERLIQUID_CHAIN_ID,
        verifying_contract: Address::ZERO,
    };
    let domain_hash = domain.hash_struct();

    // Struct type hash for HyperliquidTransaction:ApproveBuilderFee, the colon in
    // the type name rules out the alloy sol! macro, so the encoding is hand-rolled.
    let type_hash = keccak256(
        b"HyperliquidTransaction:ApproveBuilderFee(string hyperliquidChain,string maxFeeRate,address builder,uint64 nonce)",
    );

    let chain_str = if is_testnet { "Testnet" } else { "Mainnet" };
    let chain_hash = keccak256(chain_str.as_bytes());
    let fee_rate_hash = keccak256(fee_rate.as_bytes());

    let builder_addr = Address::from_str(NAUTILUS_BUILDER_ADDRESS)
        .map_err(|e| Error::transport(format!("Invalid builder address: {e}")))?;

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
    final_data.extend_from_slice(domain_hash.as_slice());
    final_data.extend_from_slice(struct_hash.as_slice());

    Ok(keccak256(&final_data))
}

fn derive_address(pk: &EvmPrivateKey) -> Result<String> {
    let key_hex = pk.as_hex();
    let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

    let signer = PrivateKeySigner::from_str(key_hex)
        .map_err(|e| Error::auth(format!("Failed to create signer: {e}")))?;

    Ok(format!("{:#x}", signer.address()))
}

async fn wait_for_confirmation(prompt: &str) -> bool {
    print!("{prompt}");
    io::stdout().flush().ok();

    let stdin_read = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin().read_line(&mut input)
    });

    tokio::select! {
        result = stdin_read => match result {
            Ok(Ok(0) | Err(_)) | Err(_) => {
                println!();
                println!("Aborted.");
                false
            }
            Ok(Ok(_)) => {
                println!();
                true
            }
        },
        _ = tokio::signal::ctrl_c() => {
            println!();
            println!("Aborted.");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Well-known development key (hardhat/anvil account 0)
    const TEST_PK: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

    #[rstest]
    fn test_derive_address_known_key() {
        let pk = EvmPrivateKey::new(TEST_PK).unwrap();

        let address = derive_address(&pk).unwrap();

        assert_eq!(address, TEST_ADDRESS);
    }

    #[rstest]
    fn test_build_approval_action_payload() {
        let action = build_approval_action(false, 1_700_000_000_000);

        assert_eq!(action["type"], "approveBuilderFee");
        assert_eq!(action["hyperliquidChain"], "Mainnet");
        assert_eq!(action["signatureChainId"], "0x66eee");
        assert_eq!(action["maxFeeRate"], "0%");
        assert_eq!(action["builder"], NAUTILUS_BUILDER_ADDRESS);
        assert_eq!(action["nonce"], 1_700_000_000_000_u64);
    }

    #[rstest]
    fn test_build_approval_action_testnet_chain() {
        let action = build_approval_action(true, 1);

        assert_eq!(action["hyperliquidChain"], "Testnet");
    }

    #[rstest]
    fn test_sign_approve_builder_fee_recovers_signer() {
        let pk = EvmPrivateKey::new(TEST_PK).unwrap();
        let nonce = 1_700_000_000_000;

        let signature = sign_approve_builder_fee(&pk, false, nonce, ZERO_FEE_RATE).unwrap();

        let signing_hash = approval_signing_hash(false, nonce, ZERO_FEE_RATE).unwrap();
        let signer = PrivateKeySigner::from_str(TEST_PK.strip_prefix("0x").unwrap()).unwrap();
        let direct = signer.sign_hash_sync(&signing_hash).unwrap();
        let recovered = direct.recover_address_from_prehash(&signing_hash).unwrap();

        assert_eq!(signature.r, format!("0x{:064x}", direct.r()));
        assert_eq!(signature.s, format!("0x{:064x}", direct.s()));
        assert_eq!(signature.v, if direct.v() { 28 } else { 27 });
        assert_eq!(format!("{recovered:#x}"), TEST_ADDRESS);
    }

    #[rstest]
    fn test_approval_signing_hash_varies_with_inputs() {
        let base = approval_signing_hash(false, 1, ZERO_FEE_RATE).unwrap();

        assert_ne!(base, approval_signing_hash(true, 1, ZERO_FEE_RATE).unwrap());
        assert_ne!(
            base,
            approval_signing_hash(false, 2, ZERO_FEE_RATE).unwrap()
        );
        assert_ne!(base, approval_signing_hash(false, 1, "0.001%").unwrap());
    }
}

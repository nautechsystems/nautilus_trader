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

use std::str::FromStr;

use alloy_primitives::{Address, B256, keccak256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{SolStruct, eip712_domain};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{nonce::TimeNonce, types::HyperliquidActionType};
use crate::{
    common::credential::EvmPrivateKey,
    http::error::{Error, Result},
};

// Define the Agent struct for L1 signing using alloy_sol_types
alloy_sol_types::sol! {
    #[derive(Debug, Serialize, Deserialize)]
    struct Agent {
        string source;
        bytes32 connectionId;
    }
}

/// Request to be signed by the Hyperliquid EIP-712 signer.
#[derive(Debug, Clone)]
pub struct SignRequest {
    pub action: Value,                 // For UserSigned actions
    pub action_bytes: Option<Vec<u8>>, // For L1 actions (pre-serialized MessagePack)
    pub time_nonce: TimeNonce,
    pub action_type: HyperliquidActionType,
    pub is_testnet: bool,
    pub vault_address: Option<String>,
}

/// Bundle containing signature for Hyperliquid requests.
#[derive(Debug, Clone)]
pub struct SignatureBundle {
    pub signature: String,
}

/// EIP-712 signer for Hyperliquid.
#[derive(Debug, Clone)]
pub struct HyperliquidEip712Signer {
    private_key: EvmPrivateKey,
}

impl HyperliquidEip712Signer {
    pub fn new(private_key: EvmPrivateKey) -> Self {
        Self { private_key }
    }

    pub fn sign(&self, request: &SignRequest) -> Result<SignatureBundle> {
        let signature = match request.action_type {
            HyperliquidActionType::L1 => self.sign_l1_action(request)?,
            HyperliquidActionType::UserSigned => {
                self.sign_user_signed_action(&request.action, request.time_nonce)?
            }
        };

        Ok(SignatureBundle { signature })
    }

    pub fn sign_l1_action(&self, request: &SignRequest) -> Result<String> {
        // L1 signing for Hyperliquid follows this pattern:
        // 1. Serialize action with MessagePack (rmp_serde)
        // 2. Append timestamp + vault info
        // 3. Hash with keccak256 to get connection_id
        // 4. Create Agent struct with source + connection_id
        // 5. Sign Agent with EIP-712

        // Step 1-3: Create connection_id
        let connection_id = self.compute_connection_id(request)?;

        // Step 4: Create Agent struct
        let source = if request.is_testnet {
            "b".to_string()
        } else {
            "a".to_string()
        };

        let agent = Agent {
            source,
            connectionId: connection_id,
        };

        // Step 5: Sign Agent with EIP-712
        let domain = eip712_domain! {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: Address::ZERO,
        };

        let signing_hash = agent.eip712_signing_hash(&domain);

        // Sign the hash
        self.sign_hash(&signing_hash.0)
    }

    fn compute_connection_id(&self, request: &SignRequest) -> Result<B256> {
        // Use pre-serialized MessagePack bytes if provided, otherwise serialize the JSON action
        let mut bytes = if let Some(action_bytes) = &request.action_bytes {
            action_bytes.clone()
        } else {
            // Fallback: serialize JSON Value with MessagePack
            rmp_serde::to_vec_named(&request.action)
                .map_err(|e| Error::transport(format!("Failed to serialize action: {}", e)))?
        };

        // Append timestamp as big-endian u64
        let timestamp = request.time_nonce.as_millis() as u64;
        bytes.extend_from_slice(&timestamp.to_be_bytes());

        // Append vault address if present
        if let Some(vault_addr) = &request.vault_address {
            bytes.push(1); // vault flag
            // Parse vault address and append bytes
            let vault_hex = vault_addr.trim_start_matches("0x");
            let vault_bytes = hex::decode(vault_hex)
                .map_err(|e| Error::transport(format!("Invalid vault address: {}", e)))?;
            bytes.extend_from_slice(&vault_bytes);
        } else {
            bytes.push(0); // no vault
        }

        // Hash with keccak256
        Ok(keccak256(&bytes))
    }

    pub fn sign_user_signed_action(&self, action: &Value, _nonce: TimeNonce) -> Result<String> {
        let canonicalized = Self::canonicalize_action(action)?;

        // EIP-712 domain separator for Hyperliquid user-signed actions
        let domain_hash = self.get_domain_hash()?;

        // Create the structured data hash
        let action_hash = self.hash_typed_data(&canonicalized)?;

        // Combine with EIP-712 prefix
        let message_hash = self.create_eip712_hash(&domain_hash, &action_hash)?;

        // Sign with private key
        self.sign_hash(&message_hash)
    }

    fn get_domain_hash(&self) -> Result<[u8; 32]> {
        // Hyperliquid EIP-712 domain separator
        // This needs to match Hyperliquid's exact domain configuration
        let domain_type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );

        let name_hash = keccak256(b"Hyperliquid");
        let version_hash = keccak256(b"1");

        // Mainnet chainId = 1, testnet might differ
        let chain_id: [u8; 32] = {
            let mut bytes = [0u8; 32];
            bytes[31] = 1; // chainId = 1 for mainnet
            bytes
        };

        // Verifying contract address (needs to be the actual Hyperliquid contract)
        // This is a placeholder and needs to be replaced with the actual contract address
        let verifying_contract = hex::decode("0000000000000000000000000000000000000000")
            .map_err(|e| Error::transport(format!("Failed to decode verifying contract: {}", e)))?;
        let mut contract_bytes = [0u8; 32];
        contract_bytes[12..].copy_from_slice(&verifying_contract);

        // Hash all components together
        let mut combined = Vec::with_capacity(160);
        combined.extend_from_slice(domain_type_hash.as_slice());
        combined.extend_from_slice(name_hash.as_slice());
        combined.extend_from_slice(version_hash.as_slice());
        combined.extend_from_slice(&chain_id);
        combined.extend_from_slice(&contract_bytes);

        Ok(*keccak256(&combined))
    }

    fn hash_typed_data(&self, data: &Value) -> Result<[u8; 32]> {
        // Convert JSON to canonical encoding and hash
        // This is a simplified version - full implementation needs proper EIP-712 encoding
        let json_str = serde_json::to_string(data)?;
        Ok(*keccak256(json_str.as_bytes()))
    }

    fn create_eip712_hash(
        &self,
        domain_hash: &[u8; 32],
        message_hash: &[u8; 32],
    ) -> Result<[u8; 32]> {
        // EIP-712 prefix: "\x19\x01" + domain_separator + message_hash
        let mut combined = Vec::with_capacity(66);
        combined.extend_from_slice(b"\x19\x01");
        combined.extend_from_slice(domain_hash);
        combined.extend_from_slice(message_hash);
        Ok(*keccak256(&combined))
    }

    fn sign_hash(&self, hash: &[u8; 32]) -> Result<String> {
        // Parse private key and create signer
        let key_hex = self.private_key.as_hex();
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

        // Create PrivateKeySigner from hex string
        let signer = PrivateKeySigner::from_str(key_hex)
            .map_err(|e| Error::transport(format!("Failed to create signer: {}", e)))?;

        // Convert [u8; 32] to B256 for signing
        let hash_b256 = B256::from(*hash);

        // Sign the hash - alloy-signer handles the signing internally
        let signature = signer
            .sign_hash_sync(&hash_b256)
            .map_err(|e| Error::transport(format!("Failed to sign hash: {}", e)))?;

        // Extract r, s, v components for Ethereum signature format
        // Ethereum signature format: 0x + r (64 hex) + s (64 hex) + v (2 hex) = 132 total
        let r = signature.r();
        let s = signature.s();
        let v = signature.v(); // Get the y_parity as bool (true = 1, false = 0)

        // Convert v from bool to Ethereum recovery ID (27 or 28)
        let v_byte = if v { 28u8 } else { 27u8 };

        // Format as Ethereum signature: 0x + r + s + v (132 hex chars total)
        Ok(format!("0x{:064x}{:064x}{:02x}", r, s, v_byte))
    }

    fn canonicalize_action(action: &Value) -> Result<Value> {
        match action {
            Value::Object(obj) => {
                let mut canonicalized = serde_json::Map::new();
                for (key, value) in obj {
                    let canon_value = match key.as_str() {
                        "destination" | "address" | "user" if value.is_string() => {
                            Value::String(Self::canonicalize_address(value.as_str().unwrap()))
                        }
                        "amount" | "px" | "sz" | "price" | "size" if value.is_string() => {
                            Value::String(Self::canonicalize_decimal(value.as_str().unwrap()))
                        }
                        _ => Self::canonicalize_action(value)?,
                    };
                    canonicalized.insert(key.clone(), canon_value);
                }
                Ok(Value::Object(canonicalized))
            }
            Value::Array(arr) => {
                let canonicalized: Result<Vec<_>> =
                    arr.iter().map(Self::canonicalize_action).collect();
                Ok(Value::Array(canonicalized?))
            }
            _ => Ok(action.clone()),
        }
    }

    fn canonicalize_address(addr: &str) -> String {
        if addr.starts_with("0x") || addr.starts_with("0X") {
            format!("0x{}", &addr[2..].to_lowercase())
        } else {
            format!("0x{}", addr.to_lowercase())
        }
    }

    fn canonicalize_decimal(decimal: &str) -> String {
        if let Ok(num) = decimal.parse::<f64>() {
            if num.fract() == 0.0 {
                format!("{:.0}", num)
            } else {
                let trimmed = format!("{}", num)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                if trimmed.is_empty() || trimmed == "-" {
                    "0".to_string()
                } else {
                    trimmed
                }
            }
        } else {
            decimal.to_string()
        }
    }

    pub fn address(&self) -> Result<String> {
        // Derive Ethereum address from private key using alloy-signer
        let key_hex = self.private_key.as_hex();
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

        // Create PrivateKeySigner from hex string
        let signer = PrivateKeySigner::from_str(key_hex)
            .map_err(|e| Error::transport(format!("Failed to create signer: {}", e)))?;

        // Get address from signer and format it properly (not Debug format)
        let address = format!("{:#x}", signer.address());
        Ok(address)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_address_canonicalization() {
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_address("0xABCDEF123456789"),
            "0xabcdef123456789"
        );
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_address("ABCDEF123456789"),
            "0xabcdef123456789"
        );
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_address("0XABCDEF123456789"),
            "0xabcdef123456789"
        );
    }

    #[rstest]
    fn test_decimal_canonicalization() {
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_decimal("100.000"),
            "100"
        );
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_decimal("100.100"),
            "100.1"
        );
        assert_eq!(HyperliquidEip712Signer::canonicalize_decimal("0.000"), "0");
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_decimal("123.456"),
            "123.456"
        );
        assert_eq!(
            HyperliquidEip712Signer::canonicalize_decimal("123.450"),
            "123.45"
        );
    }

    #[rstest]
    fn test_action_canonicalization() {
        let action = json!({
            "destination": "0xABCDEF123456789",
            "amount": "100.000",
            "other": "unchanged"
        });

        let canonicalized = HyperliquidEip712Signer::canonicalize_action(&action).unwrap();

        assert_eq!(canonicalized["destination"], "0xabcdef123456789");
        assert_eq!(canonicalized["amount"], "100");
        assert_eq!(canonicalized["other"], "unchanged");
    }

    #[rstest]
    fn test_sign_request_l1_action() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(private_key);

        let request = SignRequest {
            action: json!({
                "type": "withdraw",
                "destination": "0xABCDEF123456789",
                "amount": "100.000"
            }),
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: false,
            vault_address: None,
        };

        let result = signer.sign(&request).unwrap();
        // Verify signature format: 0x + 64 hex chars (r) + 64 hex chars (s) + 2 hex chars (v)
        assert!(result.signature.starts_with("0x"));
        assert_eq!(result.signature.len(), 132); // 0x + 130 hex chars
    }

    #[rstest]
    fn test_sign_request_user_action() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(private_key);

        let request = SignRequest {
            action: json!({
                "type": "order",
                "coin": "BTC",
                "px": "50000.00",
                "sz": "0.1"
            }),
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::UserSigned,
            is_testnet: false,
            vault_address: None,
        };

        let result = signer.sign(&request).unwrap();
        // Verify signature format: 0x + 64 hex chars (r) + 64 hex chars (s) + 2 hex chars (v)
        assert!(result.signature.starts_with("0x"));
        assert_eq!(result.signature.len(), 132); // 0x + 130 hex chars
    }
}

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

// Define the Agent struct for L1 signing
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

        self.sign_hash(&signing_hash.0)
    }

    fn compute_connection_id(&self, request: &SignRequest) -> Result<B256> {
        let mut bytes = if let Some(action_bytes) = &request.action_bytes {
            action_bytes.clone()
        } else {
            log::warn!(
                "Falling back to JSON Value msgpack serialization - this may cause hash mismatch!"
            );
            rmp_serde::to_vec_named(&request.action)
                .map_err(|e| Error::transport(format!("Failed to serialize action: {e}")))?
        };

        // Append timestamp as big-endian u64
        let timestamp = request.time_nonce.as_millis() as u64;
        bytes.extend_from_slice(&timestamp.to_be_bytes());

        if let Some(vault_addr) = &request.vault_address {
            bytes.push(1); // vault flag
            let vault_hex = vault_addr.trim_start_matches("0x");
            let vault_bytes = hex::decode(vault_hex)
                .map_err(|e| Error::transport(format!("Invalid vault address: {e}")))?;
            bytes.extend_from_slice(&vault_bytes);
        } else {
            bytes.push(0); // no vault
        }

        Ok(keccak256(&bytes))
    }

    pub fn sign_user_signed_action(&self, action: &Value, _nonce: TimeNonce) -> Result<String> {
        let canonicalized = Self::canonicalize_action(action)?;

        // EIP-712 domain separator for Hyperliquid user-signed actions
        let domain_hash = self.get_domain_hash()?;
        let action_hash = self.hash_typed_data(&canonicalized)?;
        let message_hash = self.create_eip712_hash(&domain_hash, &action_hash)?;

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
            .map_err(|e| Error::transport(format!("Failed to decode verifying contract: {e}")))?;
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
            .map_err(|e| Error::transport(format!("Failed to create signer: {e}")))?;

        // Convert [u8; 32] to B256 for signing
        let hash_b256 = B256::from(*hash);

        // Sign the hash - alloy-signer handles the signing internally
        let signature = signer
            .sign_hash_sync(&hash_b256)
            .map_err(|e| Error::transport(format!("Failed to sign hash: {e}")))?;

        // Extract r, s, v components for Ethereum signature format
        // Ethereum signature format: 0x + r (64 hex) + s (64 hex) + v (2 hex) = 132 total
        let r = signature.r();
        let s = signature.s();
        let v = signature.v(); // Get the y_parity as bool (true = 1, false = 0)

        // Convert v from bool to Ethereum recovery ID (27 or 28)
        let v_byte = if v { 28u8 } else { 27u8 };

        // Format as Ethereum signature: 0x + r + s + v (132 hex chars total)
        Ok(format!("0x{r:064x}{s:064x}{v_byte:02x}"))
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
                format!("{num:.0}")
            } else {
                let trimmed = format!("{num}")
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
            .map_err(|e| Error::transport(format!("Failed to create signer: {e}")))?;

        // Get address from signer and format it properly (not Debug format)
        let address = format!("{:#x}", signer.address());
        Ok(address)
    }
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolStruct;
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

    #[rstest]
    fn test_connection_id_matches_python() {
        use rust_decimal_macros::dec;

        use crate::http::models::{
            HyperliquidExecAction, HyperliquidExecGrouping, HyperliquidExecLimitParams,
            HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
        };

        // Test that our connection_id computation matches Python SDK exactly.
        // Python expected output for this test case:
        // MsgPack bytes: 83a474797065a56f72646572a66f72646572739186a16100a162c3a170a53530303030a173a3302e31a172c2a17481a56c696d697481a3746966a3477463a867726f7570696e67a26e61
        // Connection ID: 207b9fb52defb524f5a7f1c80f069ff8b58556b018532401de0e1342bcb13b40

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(private_key);

        // NOTE: json! macro sorts keys alphabetically, but Python preserves insertion order.
        // Field order: Python uses "type", "orders", "grouping"
        // json! produces: "grouping", "orders", "type" (alphabetical)
        // This causes hash mismatch!
        //
        // When using typed structs (HyperliquidExecAction), serde follows declaration order.
        // Let's test with the typed struct approach.

        let typed_action = HyperliquidExecAction::Order {
            orders: vec![HyperliquidExecPlaceOrderRequest {
                asset: 0,
                is_buy: true,
                price: dec!(50000),
                size: dec!(0.1),
                reduce_only: false,
                kind: HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams {
                        tif: HyperliquidExecTif::Gtc,
                    },
                },
                cloid: None,
            }],
            grouping: HyperliquidExecGrouping::Na,
            builder: None,
        };

        // Serialize the typed struct with msgpack
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();
        println!(
            "Rust typed MsgPack bytes ({}): {}",
            action_bytes.len(),
            hex::encode(&action_bytes)
        );

        // Expected from Python
        let python_msgpack = hex::decode(
            "83a474797065a56f72646572a66f72646572739186a16100a162c3a170a53530303030a173a3302e31a172c2a17481a56c696d697481a3746966a3477463a867726f7570696e67a26e61",
        )
        .unwrap();
        println!(
            "Python MsgPack bytes ({}): {}",
            python_msgpack.len(),
            hex::encode(&python_msgpack)
        );

        // Compare msgpack bytes
        assert_eq!(
            hex::encode(&action_bytes),
            hex::encode(&python_msgpack),
            "MsgPack bytes should match Python"
        );

        // Now test the full connection_id computation
        let action_value = serde_json::to_value(&typed_action).unwrap();
        let request = SignRequest {
            action: action_value,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: true, // source = "b"
            vault_address: None,
        };

        let connection_id = signer.compute_connection_id(&request).unwrap();
        println!(
            "Rust Connection ID: {}",
            hex::encode(connection_id.as_slice())
        );

        // Expected from Python
        let expected_connection_id =
            "207b9fb52defb524f5a7f1c80f069ff8b58556b018532401de0e1342bcb13b40";
        assert_eq!(
            hex::encode(connection_id.as_slice()),
            expected_connection_id,
            "Connection ID should match Python"
        );

        // Now test the full signing hash
        // Python expected values:
        // Domain separator: d79297fcdf2ffcd4ae223d01edaa2ba214ff8f401d7c9300d995d17c82aa4040
        // Struct hash: 99c7d776d74816c42973fbe58bb0f0d03c80324bef180220196d0dccf01672c5
        // Signing hash: 5242f54e0c01d3e7ef449f91b25c1a27802fdd221f7f24bc211da6bf7b847d8d

        // Create Agent and sign - matching our sign_l1_action logic
        let source = "b".to_string(); // is_testnet = true
        let agent = Agent {
            source,
            connectionId: connection_id,
        };

        let domain = eip712_domain! {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: Address::ZERO,
        };

        let signing_hash = agent.eip712_signing_hash(&domain);
        println!(
            "Rust EIP-712 signing hash: {}",
            hex::encode(signing_hash.as_slice())
        );

        // Expected from Python
        let expected_signing_hash =
            "5242f54e0c01d3e7ef449f91b25c1a27802fdd221f7f24bc211da6bf7b847d8d";
        assert_eq!(
            hex::encode(signing_hash.as_slice()),
            expected_signing_hash,
            "EIP-712 signing hash should match Python"
        );
    }

    #[rstest]
    fn test_connection_id_matches_python_with_builder_fee() {
        use rust_decimal_macros::dec;

        use crate::http::models::{
            HyperliquidExecAction, HyperliquidExecBuilderFee, HyperliquidExecGrouping,
            HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
            HyperliquidExecTif,
        };

        // Test with builder fee included (what production uses).
        // Python expected output:
        // MsgPack bytes (132): 84a474797065a56f72646572a66f72646572739186a16100a162c3a170a53530303030a173a3302e31a172c2a17481a56c696d697481a3746966a3477463a867726f7570696e67a26e61a76275696c64657282a162d92a307839623665326665343132346564336537613662346638356537383630653033323232326234333136a16601
        // Connection ID: 235d93388ffa044d5fb14a7fe8103a8a29b73d1e2049cd086e7903671a6cfb49
        // Signing hash: 6f046f4b02e79610b8cf26c73505f8de3ff1d91d6953c5e972fbf198a5311a41

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(private_key);

        let typed_action = HyperliquidExecAction::Order {
            orders: vec![HyperliquidExecPlaceOrderRequest {
                asset: 0,
                is_buy: true,
                price: dec!(50000),
                size: dec!(0.1),
                reduce_only: false,
                kind: HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams {
                        tif: HyperliquidExecTif::Gtc,
                    },
                },
                cloid: None,
            }],
            grouping: HyperliquidExecGrouping::Na,
            builder: Some(HyperliquidExecBuilderFee {
                address: "0x9b6e2fe4124ed3e7a6b4f85e7860e032222b4316".to_string(),
                fee_tenths_bp: 1,
            }),
        };

        // Serialize the typed struct with msgpack
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();
        println!(
            "Rust typed MsgPack bytes with builder ({}): {}",
            action_bytes.len(),
            hex::encode(&action_bytes)
        );

        // Expected from Python
        let python_msgpack = hex::decode(
            "84a474797065a56f72646572a66f72646572739186a16100a162c3a170a53530303030a173a3302e31a172c2a17481a56c696d697481a3746966a3477463a867726f7570696e67a26e61a76275696c64657282a162d92a307839623665326665343132346564336537613662346638356537383630653033323232326234333136a16601",
        )
        .unwrap();
        println!(
            "Python MsgPack bytes with builder ({}): {}",
            python_msgpack.len(),
            hex::encode(&python_msgpack)
        );

        // Compare msgpack bytes
        assert_eq!(
            hex::encode(&action_bytes),
            hex::encode(&python_msgpack),
            "MsgPack bytes with builder should match Python"
        );

        // Test connection_id
        let action_value = serde_json::to_value(&typed_action).unwrap();
        let request = SignRequest {
            action: action_value,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: true,
            vault_address: None,
        };

        let connection_id = signer.compute_connection_id(&request).unwrap();
        println!(
            "Rust Connection ID with builder: {}",
            hex::encode(connection_id.as_slice())
        );

        let expected_connection_id =
            "235d93388ffa044d5fb14a7fe8103a8a29b73d1e2049cd086e7903671a6cfb49";
        assert_eq!(
            hex::encode(connection_id.as_slice()),
            expected_connection_id,
            "Connection ID with builder should match Python"
        );

        // Test signing hash
        let source = "b".to_string();
        let agent = Agent {
            source,
            connectionId: connection_id,
        };

        let domain = eip712_domain! {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: Address::ZERO,
        };

        let signing_hash = agent.eip712_signing_hash(&domain);
        println!(
            "Rust EIP-712 signing hash with builder: {}",
            hex::encode(signing_hash.as_slice())
        );

        let expected_signing_hash =
            "6f046f4b02e79610b8cf26c73505f8de3ff1d91d6953c5e972fbf198a5311a41";
        assert_eq!(
            hex::encode(signing_hash.as_slice()),
            expected_signing_hash,
            "EIP-712 signing hash with builder should match Python"
        );
    }

    #[rstest]
    fn test_connection_id_with_cloid() {
        use rust_decimal_macros::dec;

        use crate::http::models::{
            Cloid, HyperliquidExecAction, HyperliquidExecBuilderFee, HyperliquidExecGrouping,
            HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
            HyperliquidExecTif,
        };

        // Test with CLOID included - this is what production actually sends.
        // The key difference: production always includes a cloid field.

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let _signer = HyperliquidEip712Signer::new(private_key);

        // Create a cloid - this is how Python SDK expects it
        let cloid = Cloid::from_hex("0x1234567890abcdef1234567890abcdef").unwrap();
        println!("Cloid hex: {}", cloid.to_hex());

        let typed_action = HyperliquidExecAction::Order {
            orders: vec![HyperliquidExecPlaceOrderRequest {
                asset: 0,
                is_buy: true,
                price: dec!(50000),
                size: dec!(0.1),
                reduce_only: false,
                kind: HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams {
                        tif: HyperliquidExecTif::Gtc,
                    },
                },
                cloid: Some(cloid),
            }],
            grouping: HyperliquidExecGrouping::Na,
            builder: Some(HyperliquidExecBuilderFee {
                address: "0x9b6e2fe4124ed3e7a6b4f85e7860e032222b4316".to_string(),
                fee_tenths_bp: 1,
            }),
        };

        // Serialize the typed struct with msgpack
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();
        println!(
            "Rust MsgPack bytes with cloid ({}): {}",
            action_bytes.len(),
            hex::encode(&action_bytes)
        );

        // Decode to see the structure
        let decoded: serde_json::Value = rmp_serde::from_slice(&action_bytes).unwrap();
        println!(
            "Decoded structure: {}",
            serde_json::to_string_pretty(&decoded).unwrap()
        );

        // Verify the cloid is in the right place
        let orders = decoded.get("orders").unwrap().as_array().unwrap();
        let first_order = &orders[0];
        let cloid_field = first_order.get("c").unwrap();
        println!("Cloid in msgpack: {cloid_field}");
        assert_eq!(
            cloid_field.as_str().unwrap(),
            "0x1234567890abcdef1234567890abcdef"
        );

        // Verify order field order is correct: a, b, p, s, r, t, c
        let order_json = serde_json::to_string(first_order).unwrap();
        println!("Order JSON: {order_json}");
    }

    #[rstest]
    fn test_cloid_from_client_order_id() {
        use nautilus_model::identifiers::ClientOrderId;

        use crate::http::models::Cloid;

        // Test that Cloid::from_client_order_id produces valid hex format
        // This is how production creates cloids
        let client_order_id = ClientOrderId::from("O-20241210-123456-001-001-1");
        let cloid = Cloid::from_client_order_id(client_order_id);

        println!("ClientOrderId: {client_order_id}");
        println!("Cloid hex: {}", cloid.to_hex());

        // Verify format: 0x + 32 hex chars
        let hex = cloid.to_hex();
        assert!(hex.starts_with("0x"), "Should start with 0x");
        assert_eq!(hex.len(), 34, "Should be 34 chars (0x + 32 hex)");

        // Verify all chars after 0x are valid hex
        for c in hex[2..].chars() {
            assert!(c.is_ascii_hexdigit(), "Should be hex digit: {c}");
        }

        // Verify serialization to JSON
        let json = serde_json::to_string(&cloid).unwrap();
        println!("Cloid JSON: {json}");
        assert!(json.contains(&hex));
    }

    #[rstest]
    fn test_production_like_order_with_hashed_cloid() {
        use nautilus_model::identifiers::ClientOrderId;
        use rust_decimal_macros::dec;

        use crate::http::models::{
            Cloid, HyperliquidExecAction, HyperliquidExecBuilderFee, HyperliquidExecGrouping,
            HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
            HyperliquidExecTif,
        };

        // Full production-like test with cloid from ClientOrderId

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(private_key);

        // Production-like values
        let client_order_id = ClientOrderId::from("O-20241210-123456-001-001-1");
        let cloid = Cloid::from_client_order_id(client_order_id);

        println!("=== Production-like Order ===");
        println!("ClientOrderId: {client_order_id}");
        println!("Cloid: {}", cloid.to_hex());

        let typed_action = HyperliquidExecAction::Order {
            orders: vec![HyperliquidExecPlaceOrderRequest {
                asset: 3, // BTC on testnet
                is_buy: true,
                price: dec!(92572.0),
                size: dec!(0.001),
                reduce_only: false,
                kind: HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams {
                        tif: HyperliquidExecTif::Gtc,
                    },
                },
                cloid: Some(cloid),
            }],
            grouping: HyperliquidExecGrouping::Na,
            builder: Some(HyperliquidExecBuilderFee {
                address: "0x9b6e2fe4124ed3e7a6b4f85e7860e032222b4316".to_string(),
                fee_tenths_bp: 1,
            }),
        };

        // Serialize with msgpack
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();
        println!(
            "MsgPack bytes ({}): {}",
            action_bytes.len(),
            hex::encode(&action_bytes)
        );

        // Decode to verify structure
        let decoded: serde_json::Value = rmp_serde::from_slice(&action_bytes).unwrap();
        println!(
            "Decoded: {}",
            serde_json::to_string_pretty(&decoded).unwrap()
        );

        // Compute connection_id and signing hash
        let action_value = serde_json::to_value(&typed_action).unwrap();
        let request = SignRequest {
            action: action_value,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1733833200000), // Dec 10, 2024
            action_type: HyperliquidActionType::L1,
            is_testnet: true, // source = "b"
            vault_address: None,
        };

        let connection_id = signer.compute_connection_id(&request).unwrap();
        println!("Connection ID: {}", hex::encode(connection_id.as_slice()));

        // Create Agent and get signing hash
        let source = "b".to_string();
        let agent = Agent {
            source,
            connectionId: connection_id,
        };

        let domain = eip712_domain! {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: Address::ZERO,
        };

        let signing_hash = agent.eip712_signing_hash(&domain);
        println!("Signing hash: {}", hex::encode(signing_hash.as_slice()));

        // Sign and verify signature format
        let result = signer.sign(&request).unwrap();
        println!("Signature: {}", result.signature);
        assert!(result.signature.starts_with("0x"));
        assert_eq!(result.signature.len(), 132);
    }

    #[rstest]
    fn test_price_decimal_formatting() {
        use nautilus_model::types::Price;
        use rust_decimal_macros::dec;

        // Compare how Price::as_decimal() formats vs dec!() macro
        // Test various price formats
        let test_cases = [
            (92572.0_f64, 1_u8, "92572"), // BTC price
            (92572.5, 1, "92572.5"),      // BTC price with fractional
            (0.001, 8, "0.001"),          // Small qty
            (50000.0, 1, "50000"),        // Round number
            (0.1, 4, "0.1"),              // Typical qty
        ];

        for (value, precision, expected_normalized) in test_cases {
            let price = Price::new(value, precision);
            let price_decimal = price.as_decimal();
            let normalized = price_decimal.normalize();

            println!(
                "Price({value}, {precision}) -> as_decimal: {price_decimal:?} -> normalized: {normalized}"
            );

            assert_eq!(
                normalized.to_string(),
                expected_normalized,
                "Price({value}, {precision}) should normalize to {expected_normalized}"
            );
        }

        // Verify dec! macro produces same result
        let price_from_type = Price::new(92572.0, 1).as_decimal().normalize();
        let price_from_dec = dec!(92572.0).normalize();
        assert_eq!(
            price_from_type.to_string(),
            price_from_dec.to_string(),
            "Price::as_decimal should match dec! macro"
        );
    }
}

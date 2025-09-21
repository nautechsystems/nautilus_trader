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

use serde_json::Value;

use super::{nonce::TimeNonce, types::HyperliquidActionType};
use crate::{common::credential::EvmPrivateKey, http::error::Result};

/// Request to be signed by the Hyperliquid EIP-712 signer.
#[derive(Debug, Clone)]
pub struct SignRequest {
    pub action: Value,
    pub time_nonce: TimeNonce,
    pub action_type: HyperliquidActionType,
}

/// Bundle containing signature for Hyperliquid requests.
#[derive(Debug, Clone)]
pub struct SignatureBundle {
    pub signature: String,
}

/// EIP-712 signer for Hyperliquid.
#[derive(Debug)]
pub struct HyperliquidEip712Signer {
    private_key: EvmPrivateKey,
}

impl HyperliquidEip712Signer {
    pub fn new(private_key: EvmPrivateKey) -> Self {
        Self { private_key }
    }

    pub fn sign(&self, request: &SignRequest) -> Result<SignatureBundle> {
        let signature = match request.action_type {
            HyperliquidActionType::L1 => {
                self.sign_l1_action(&request.action, request.time_nonce)?
            }
            HyperliquidActionType::UserSigned => {
                self.sign_user_signed_action(&request.action, request.time_nonce)?
            }
        };

        Ok(SignatureBundle { signature })
    }

    pub fn sign_l1_action(&self, action: &Value, nonce: TimeNonce) -> Result<String> {
        let _canonicalized = Self::canonicalize_action(action)?;
        // TODO: Implement actual EIP-712 signing with self.private_key
        let _key_hash = self.private_key.to_string().len(); // Use private_key to avoid dead_code warning
        Ok(format!("0x{:064x}{:064x}", nonce.as_millis(), 1))
    }

    pub fn sign_user_signed_action(&self, action: &Value, nonce: TimeNonce) -> Result<String> {
        let _canonicalized = Self::canonicalize_action(action)?;
        // TODO: Implement actual EIP-712 signing with self.private_key
        let _key_hash = self.private_key.to_string().len(); // Use private_key to avoid dead_code warning
        Ok(format!("0x{:064x}{:064x}", nonce.as_millis(), 2))
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
        // TODO: Derive actual address from self.private_key
        let _key_hash = self.private_key.to_string().len(); // Use private_key to avoid dead_code warning
        Ok("0x0000000000000000000000000000000000000000".to_string())
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
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
        };

        let result = signer.sign(&request).unwrap();
        assert!(result.signature.ends_with("0000000000000001"));
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
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::UserSigned,
        };

        let result = signer.sign(&request).unwrap();
        assert!(result.signature.ends_with("0000000000000002"));
    }
}

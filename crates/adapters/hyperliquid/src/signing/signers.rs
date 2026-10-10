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

use std::{fmt::Debug, str::FromStr};

use alloy::signers::SignerSync;
use alloy_primitives::{Address, B256, Keccak256};
use chrono::DateTime;
use hypersdk::hypercore::{Chain, PrivateKeySigner, api::Action, signing::agent_signing_hash};
use nautilus_core::string::secret::REDACTED;
use serde_json::Value;

use super::{nonce::TimeNonce, types::HyperliquidActionType};
use crate::{
    common::credential::{EvmPrivateKey, VaultAddress},
    http::{
        error::{Error, Result},
        models::HyperliquidSignature,
    },
};

/// Request to be signed by the Hyperliquid EIP-712 signer.
///
/// For L1 actions, populate `action_bytes` with the pre-serialized MessagePack
/// of the typed action; `action` may be `None`. The `action` JSON value is only
/// consumed as a fallback when `action_bytes` is `None` (kept for ad-hoc test
/// payloads built via `json!`).
#[derive(Debug, Clone)]
pub struct SignRequest {
    pub action: Option<Value>,         // Fallback when action_bytes is None
    pub action_bytes: Option<Vec<u8>>, // Pre-serialized MessagePack (preferred)
    pub time_nonce: TimeNonce,
    pub action_type: HyperliquidActionType,
    pub is_testnet: bool,
    pub vault_address: Option<VaultAddress>,
    pub expires_after: Option<u64>,
}

/// Bundle containing signature for Hyperliquid requests.
#[derive(Debug, Clone)]
pub struct SignatureBundle {
    pub signature: HyperliquidSignature,
}

/// EIP-712 signer for Hyperliquid.
#[derive(Clone)]
pub struct HyperliquidEip712Signer {
    signer: PrivateKeySigner,
    address: String,
}

impl Debug for HyperliquidEip712Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HyperliquidEip712Signer))
            .field("signer", &REDACTED)
            .field("address", &self.address)
            .finish()
    }
}

impl HyperliquidEip712Signer {
    /// Creates a new [`HyperliquidEip712Signer`].
    ///
    /// # Errors
    ///
    /// Returns an error if the private key cannot be parsed.
    pub fn new(private_key: &EvmPrivateKey) -> Result<Self> {
        let key_hex = private_key.as_hex();
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);

        let signer = PrivateKeySigner::from_str(key_hex)
            .map_err(|e| Error::auth(format!("Failed to create signer: {e}")))?;

        let address = format!("{:#x}", signer.address());

        Ok(Self { signer, address })
    }

    pub fn sign(&self, request: &SignRequest) -> Result<SignatureBundle> {
        let signature = match request.action_type {
            HyperliquidActionType::L1 => self.sign_l1_action(request)?,
            HyperliquidActionType::UserSigned => {
                return Err(Error::bad_request(
                    "Generic user-signed requests are unsupported; use typed exchange actions",
                ));
            }
        };

        Ok(SignatureBundle { signature })
    }

    pub fn sign_l1_action(&self, request: &SignRequest) -> Result<HyperliquidSignature> {
        let connection_id = self.compute_connection_id(request)?;
        let chain = if request.is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };
        let signing_hash = agent_signing_hash(chain, connection_id);

        self.sign_hash(&signing_hash.0)
    }

    pub(crate) fn sign_exchange_action(
        &self,
        action: &Action,
        request: &SignRequest,
    ) -> Result<HyperliquidSignature> {
        let vault = request
            .vault_address
            .map(|vault| Address::from_slice(vault.as_bytes()));
        let chain = if request.is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };

        if let Action::UsdClassTransfer(transfer) = action {
            if transfer.nonce != request.time_nonce.as_millis() as u64
                || transfer.hyperliquid_chain != chain
            {
                return Err(Error::bad_request(
                    "Transfer network or nonce does not match its request",
                ));
            }

            if request.vault_address.is_some() || request.expires_after.is_some() {
                return Err(Error::bad_request(
                    "USD class transfers do not support vaults or expiry",
                ));
            }
        }
        let expires_after = request
            .expires_after
            .map(|expires_after| {
                let timestamp = i64::try_from(expires_after).map_err(|_| {
                    Error::bad_request("Expiry exceeds the supported timestamp range")
                })?;
                DateTime::from_timestamp_millis(timestamp).ok_or_else(|| {
                    Error::bad_request("Expiry exceeds the supported timestamp range")
                })
            })
            .transpose()?;
        let hash = action
            .prehash(
                request.time_nonce.as_millis() as u64,
                vault,
                expires_after,
                chain,
            )
            .map_err(|e| Error::bad_request(format!("Failed to hash action: {e}")))?;
        self.sign_hash(&hash.0)
    }

    fn compute_connection_id(&self, request: &SignRequest) -> Result<B256> {
        let mut hasher = Keccak256::new();

        if let Some(action_bytes) = &request.action_bytes {
            hasher.update(action_bytes);
        } else {
            log::warn!(
                "Falling back to JSON Value msgpack serialization - this may cause hash mismatch!"
            );
            let action = request.action.as_ref().ok_or_else(|| {
                Error::bad_request("SignRequest has neither action_bytes nor action")
            })?;
            let action_bytes = rmp_serde::to_vec_named(action)
                .map_err(|e| Error::bad_request(format!("Failed to serialize action: {e}")))?;
            hasher.update(&action_bytes);
        }

        let timestamp = request.time_nonce.as_millis() as u64;
        hasher.update(timestamp.to_be_bytes());

        if let Some(vault_addr) = request.vault_address {
            hasher.update([1u8]);
            hasher.update(vault_addr.as_bytes());
        } else {
            hasher.update([0u8]);
        }

        if let Some(expires_after) = request.expires_after {
            hasher.update([0u8]);
            hasher.update(expires_after.to_be_bytes());
        }

        Ok(hasher.finalize())
    }

    fn sign_hash(&self, hash: &[u8; 32]) -> Result<HyperliquidSignature> {
        let hash_b256 = B256::from(*hash);

        let signature = self
            .signer
            .sign_hash_sync(&hash_b256)
            .map_err(|e| Error::auth(format!("Failed to sign hash: {e}")))?;

        let r = signature.r();
        let s = signature.s();
        let v = signature.v();
        let v_byte = if v { 28u8 } else { 27u8 };

        Ok(HyperliquidSignature::new(
            format!("0x{r:064x}"),
            format!("0x{s:064x}"),
            v_byte as u64,
        ))
    }

    /// Returns the signer's Ethereum address.
    pub fn address(&self) -> Result<String> {
        Ok(self.address.clone())
    }
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use alloy::sol_types::{SolStruct, eip712_domain};
    use alloy_primitives::Address;
    use hypersdk::hypercore::{
        BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce as SdkTimeInForce,
        api::{ApproveBuilderFee, UsdClassTransferAction, UserOutcomeAction},
    };
    use nautilus_core::hex;
    use nautilus_model::{identifiers::ClientOrderId, types::Price};
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde::Serialize;
    use serde_json::json;

    use super::*;
    use crate::http::models::Cloid;

    alloy::sol! {
        struct Agent {
            string source;
            bytes32 connectionId;
        }
    }

    #[rstest]
    #[case::order(json!({
        "type": "order",
        "orders": [{
            "a": 0, "b": true, "p": "51000.00", "s": "0.1000", "r": false,
            "t": {"limit": {"tif": "Alo"}},
            "c": "0x11111111111111111111111111111111"
        }],
        "grouping": "na"
    }))]
    #[case::trigger(json!({
        "type": "order",
        "orders": [{
            "a": 10001, "b": false, "p": "49000", "s": "0.1", "r": true,
            "t": {"trigger": {"isMarket": true, "triggerPx": "50000.00", "tpsl": "sl"}}
        }],
        "grouping": "positionTpsl",
        "builder": {"b": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "f": 10}
    }))]
    #[case::cancel(json!({"type": "cancel", "cancels": [{"a": 0, "o": 123}]}))]
    #[case::cancel_without_fast(json!({
        "type": "cancel", "cancels": [{"a": 0, "o": 123}], "f": false
    }))]
    #[case::zero_cloid(json!({
        "type": "order",
        "orders": [{
            "a": 0, "b": true, "p": "0.0000000000000000000000000001",
            "s": "1.0000000000000000000000000000", "r": false,
            "t": {"limit": {"tif": "Gtc"}},
            "c": "0x00000000000000000000000000000000"
        }],
        "grouping": "na"
    }))]
    #[case::fast_cancel(json!({
        "type": "cancel", "cancels": [{"a": 10001, "o": 123}], "f": true
    }))]
    #[case::cancel_by_cloid(json!({
        "type": "cancelByCloid",
        "cancels": [{"asset": 0, "cloid": "0x11111111111111111111111111111111"}]
    }))]
    #[case::modify(json!({
        "type": "modify", "oid": "0x11111111111111111111111111111111",
        "order": {"a": 0, "b": true, "p": "51000", "s": "0.1", "r": false,
                  "t": {"limit": {"tif": "Gtc"}}}
    }))]
    #[case::batch_modify(json!({
        "type": "batchModify",
        "modifies": [{"oid": 123,
            "order": {"a": 0, "b": false, "p": "52000", "s": "0.1", "r": true,
                      "t": {"limit": {"tif": "Ioc"}}}}]
    }))]
    #[case::schedule_cancel(json!({"type": "scheduleCancel", "time": 1700000001000_u64}))]
    #[case::clear_schedule(json!({"type": "scheduleCancel"}))]
    #[case::split_outcome(json!({
        "type": "userOutcome", "splitOutcome": {"outcome": 123, "amount": "2"}
    }))]
    #[case::merge_outcome(json!({
        "type": "userOutcome", "mergeOutcome": {"outcome": 123, "amount": null}
    }))]
    #[case::merge_question(json!({
        "type": "userOutcome", "mergeQuestion": {"question": 123, "amount": "2"}
    }))]
    #[case::merge_question_max(json!({
        "type": "userOutcome", "mergeQuestion": {"question": 123, "amount": null}
    }))]
    #[case::negate_outcome(json!({
        "type": "userOutcome",
        "negateOutcome": {"question": 123, "outcome": 456, "amount": "2"}
    }))]
    #[case::noop(json!({"type": "noop"}))]
    #[case::update_leverage(json!({
        "type": "updateLeverage", "asset": 10001, "isCross": false, "leverage": 3
    }))]
    #[case::cancel_twap(json!({"type": "twapCancel", "a": 10001, "t": 123}))]
    #[case::place_twap(json!({
        "type": "twapOrder",
        "twap": {"a": 10001, "b": false, "s": "0.1000", "r": true, "m": 30, "t": true}
    }))]
    #[case::add_margin(json!({
        "type": "updateIsolatedMargin", "asset": 10001, "isBuy": true, "ntli": 1_000_001
    }))]
    fn test_l1_signature_matches_hypersdk(
        #[case] payload: Value,
        #[values(false, true)] is_testnet: bool,
        #[values(false, true)] with_vault: bool,
        #[values(None, Some(1700000001000_u64))] expires_after: Option<u64>,
    ) {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let action: Action = if payload["type"] == "userOutcome" {
            if let Some(params) = payload.get("mergeOutcome") {
                let amount = params["amount"]
                    .as_str()
                    .map(|value| value.parse().unwrap());
                Action::UserOutcome(UserOutcomeAction::merge(
                    params["outcome"].as_u64().unwrap() as u32,
                    amount,
                ))
            } else if let Some(params) = payload.get("mergeQuestion") {
                let amount = params["amount"]
                    .as_str()
                    .map(|value| value.parse().unwrap());
                Action::UserOutcome(UserOutcomeAction::merge_question(
                    params["question"].as_u64().unwrap() as u32,
                    amount,
                ))
            } else {
                serde_json::from_value(payload).unwrap()
            }
        } else {
            serde_json::from_value(payload).unwrap()
        };
        let action_bytes = rmp_serde::to_vec_named(&action).unwrap();

        let vault_address = with_vault
            .then(|| VaultAddress::parse("0x2222222222222222222222222222222222222222").unwrap());
        let request = SignRequest {
            action: None,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1700000000000),
            action_type: HyperliquidActionType::L1,
            is_testnet,
            vault_address,
            expires_after,
        };
        let connection_id = action
            .hash(
                request.time_nonce.as_millis() as u64,
                vault_address.map(|vault| Address::from_slice(vault.as_bytes())),
                expires_after,
            )
            .unwrap();
        assert_eq!(
            signer.compute_connection_id(&request).unwrap(),
            connection_id
        );

        let chain = if is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };
        let signing_hash = agent_signing_hash(chain, connection_id);
        let sdk_signature = signer.signer.sign_hash_sync(&signing_hash).unwrap();
        let signature = signer.sign(&request).unwrap().signature;
        let signature_bytes = signature.to_hex();
        assert_eq!(signature_bytes.expose_secret(), &format!("{sdk_signature}"));
        let typed_signature = signer.sign_exchange_action(&action, &request).unwrap();
        assert_eq!(
            typed_signature.to_hex().expose_secret(),
            signature_bytes.expose_secret()
        );
    }

    #[rstest]
    fn test_sign_request_l1_action() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let debug = format!("{signer:?}");

        let request = SignRequest {
            action: Some(json!({
                "type": "withdraw",
                "destination": "0xABCDEF123456789",
                "amount": "100.000"
            })),
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: false,
            vault_address: None,
            expires_after: None,
        };

        let result = signer.sign(&request).unwrap();
        let sig_hex = result.signature.to_hex();
        // Verify signature format: 0x + 64 hex chars (r) + 64 hex chars (s) + 2 hex chars (v)
        assert!(sig_hex.expose_secret().starts_with("0x"));
        assert_eq!(sig_hex.expose_secret().len(), 132); // 0x + 130 hex chars
        assert!(debug.contains(REDACTED));
        assert!(!debug.contains(private_key.as_hex()));
    }

    #[rstest]
    fn test_usd_transfer_signature_uses_sdk_user_domain(
        #[values(false, true)] is_testnet: bool,
        #[values(false, true)] to_perp: bool,
    ) {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let chain = if is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };
        let action = Action::UsdClassTransfer(UsdClassTransferAction {
            hyperliquid_chain: chain,
            signature_chain_id: chain.arbitrum_id().to_string(),
            amount: dec!(1.0000000000000000000000000001).to_string(),
            to_perp,
            nonce: 1_700_000_000_000,
        });
        let request = SignRequest {
            action: None,
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1_700_000_000_000),
            action_type: HyperliquidActionType::UserSigned,
            is_testnet,
            vault_address: None,
            expires_after: None,
        };
        let signature = signer.sign_exchange_action(&action, &request).unwrap();
        let sdk_action: Action =
            serde_json::from_value(serde_json::to_value(&action).unwrap()).unwrap();
        let sdk_signature = signature.to_hex().expose_secret().parse().unwrap();
        assert_eq!(
            sdk_action
                .recover(&sdk_signature, 1_700_000_000_000, None, None, chain)
                .unwrap(),
            signer.signer.address()
        );

        let raw_request = SignRequest {
            action_bytes: Some(rmp_serde::to_vec_named(&action).unwrap()),
            ..request.clone()
        };
        assert_ne!(
            signer
                .sign_l1_action(&raw_request)
                .unwrap()
                .to_hex()
                .expose_secret(),
            signature.to_hex().expose_secret()
        );
        let mismatched_request = SignRequest {
            time_nonce: TimeNonce::from_millis(1_700_000_000_001),
            ..request.clone()
        };
        assert!(
            signer
                .sign_exchange_action(&action, &mismatched_request)
                .is_err()
        );
        let mismatched_request = SignRequest {
            is_testnet: !is_testnet,
            ..request
        };
        assert!(
            signer
                .sign_exchange_action(&action, &mismatched_request)
                .is_err()
        );
    }

    #[rstest]
    fn test_builder_approval_signature_uses_sdk_user_domain(
        #[values(false, true)] is_testnet: bool,
    ) {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let chain = if is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };
        let nonce = 1_700_000_000_000;
        let action = Action::ApproveBuilderFee(ApproveBuilderFee {
            signature_chain_id: chain.arbitrum_id().to_string(),
            hyperliquid_chain: chain,
            max_fee_rate: "0.001%".to_string(),
            builder: Address::repeat_byte(0x22),
            nonce,
        });
        let request = SignRequest {
            action: None,
            action_bytes: Some(rmp_serde::to_vec_named(&action).unwrap()),
            time_nonce: TimeNonce::from_millis(nonce.into()),
            action_type: HyperliquidActionType::UserSigned,
            is_testnet,
            vault_address: None,
            expires_after: None,
        };
        let signature = signer.sign_exchange_action(&action, &request).unwrap();
        let sdk_signature = signature.to_hex().expose_secret().parse().unwrap();
        assert_eq!(
            action
                .recover(&sdk_signature, nonce, None, None, chain)
                .unwrap(),
            signer.signer.address()
        );
        assert_ne!(
            signature.to_hex().expose_secret(),
            signer
                .sign_l1_action(&request)
                .unwrap()
                .to_hex()
                .expose_secret()
        );
    }

    #[rstest]
    fn test_sdk_signing_rejects_out_of_range_expiry(
        #[values(i64::MAX as u64, u64::MAX)] expires_after: u64,
    ) {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let request = SignRequest {
            action: None,
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1_700_000_000_000),
            action_type: HyperliquidActionType::L1,
            is_testnet: false,
            vault_address: None,
            expires_after: Some(expires_after),
        };
        assert!(matches!(
            signer.sign_exchange_action(&Action::Noop, &request),
            Err(Error::BadRequest(_))
        ));
    }

    #[rstest]
    fn test_margin_withdrawal_preserves_signed_wire_value(
        #[values(-1_i64, -1_000_001, i64::MIN)] ntli: i64,
        #[values(false, true)] is_testnet: bool,
    ) {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let action = crate::http::query::UpdateIsolatedMarginParams {
            asset: 10001,
            is_buy: true,
            ntli,
        };
        let reference_bytes = rmp_serde::to_vec_named(&action).unwrap();
        let request = SignRequest {
            action: None,
            action_bytes: Some(reference_bytes),
            time_nonce: TimeNonce::from_millis(1_700_000_000_000),
            action_type: HyperliquidActionType::L1,
            is_testnet,
            vault_address: None,
            expires_after: Some(1_700_000_001_000),
        };
        let connection_id = signer.compute_connection_id(&request).unwrap();
        let chain = if is_testnet {
            Chain::Testnet
        } else {
            Chain::Mainnet
        };
        let hash = agent_signing_hash(chain, connection_id);
        let expected = signer.signer.sign_hash_sync(&hash).unwrap();
        assert_eq!(
            signer
                .sign_l1_action(&request)
                .unwrap()
                .to_hex()
                .expose_secret(),
            &format!("{expected}")
        );
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["ntli"].as_i64(), Some(ntli));
    }

    // L1 sign with neither field set must error, not panic on missing input
    #[rstest]
    fn test_sign_l1_rejects_when_action_and_bytes_missing() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        let request = SignRequest {
            action: None,
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: false,
            vault_address: None,
            expires_after: None,
        };

        let err = signer.sign(&request).unwrap_err();
        assert!(
            matches!(err, Error::BadRequest(_)),
            "expected BadRequest, was {err:?}",
        );
    }

    #[rstest]
    fn official_l1_dummy_action_signature_matches_python_sdk_for_both_environments() {
        // Official L1 vector from hyperliquid-python-sdk tests/signing_test.py
        // (revision 2fdb18f9517675ea03695a0962bd19eece9c83f0).
        #[derive(Serialize)]
        struct DummyAction<'a> {
            #[serde(rename = "type")]
            action_type: &'a str,
            num: u64,
        }

        let python_quantity_hex = |value: &str| {
            let digits = value.trim_start_matches("0x").trim_start_matches('0');
            format!("0x{}", if digits.is_empty() { "0" } else { digits })
        };

        let private_key = EvmPrivateKey::new(
            "0x0123456789012345678901234567890123456789012345678901234567890123",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();
        let action_bytes = rmp_serde::to_vec_named(&DummyAction {
            action_type: "dummy",
            num: 100_000_000_000,
        })
        .unwrap();
        let request = |is_testnet| SignRequest {
            action: None,
            action_bytes: Some(action_bytes.clone()),
            time_nonce: TimeNonce::from_millis(0),
            action_type: HyperliquidActionType::L1,
            is_testnet,
            vault_address: None,
            expires_after: None,
        };

        let mainnet = signer.sign_l1_action(&request(false)).unwrap();
        assert_eq!(
            python_quantity_hex(mainnet.r.expose_secret()),
            "0x53749d5b30552aeb2fca34b530185976545bb22d0b3ce6f62e31be961a59298"
        );
        assert_eq!(
            mainnet.s.expose_secret(),
            "0x755c40ba9bf05223521753995abb2f73ab3229be8ec921f350cb447e384d8ed8"
        );
        assert_eq!(mainnet.v, 27);

        let testnet = signer.sign_l1_action(&request(true)).unwrap();
        assert_eq!(
            testnet.r.expose_secret(),
            "0x542af61ef1f429707e3c76c5293c80d01f74ef853e34b76efffcb57e574f9510"
        );
        assert_eq!(
            testnet.s.expose_secret(),
            "0x17b8b32f086e8cdede991f1e2c529f5dd5297cbe8128500e00cbaf766204a613"
        );
        assert_eq!(testnet.v, 28);
    }

    #[rstest]
    fn test_sign_user_signed_returns_error() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        let request = SignRequest {
            action: Some(json!({"type": "order"})),
            action_bytes: None,
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::UserSigned,
            is_testnet: false,
            vault_address: None,
            expires_after: None,
        };

        let err = signer.sign(&request).unwrap_err();
        assert!(
            matches!(err, Error::BadRequest(_)),
            "expected BadRequest, was {err:?}"
        );
    }

    #[rstest]
    fn test_connection_id_matches_python() {
        // Test that our connection_id computation matches Python SDK exactly.
        // Python expected output for this test case:
        // MsgPack bytes: 83a474797065a56f72646572a66f72646572739186a16100a162c3a170a53530303030a173a3302e31a172c2a17481a56c696d697481a3746966a3477463a867726f7570696e67a26e61
        // Connection ID: 207b9fb52defb524f5a7f1c80f069ff8b58556b018532401de0e1342bcb13b40

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        // NOTE: json! macro sorts keys alphabetically, but Python preserves insertion order.
        // Field order: Python uses "type", "orders", "grouping"
        // json! produces: "grouping", "orders", "type" (alphabetical)
        // This causes hash mismatch!
        //
        // When using typed structs (Action), serde follows declaration order.
        // Let's test with the typed struct approach.

        let typed_action = Action::Order(BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: Default::default(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        });

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
        let request = SignRequest {
            action: None,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: true, // source = "b"
            vault_address: None,
            expires_after: None,
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
    fn test_connection_id_includes_expires_after_when_present() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        let typed_action = Action::Order(BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: Default::default(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        });
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();

        let without_expiry = SignRequest {
            action: None,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: true,
            vault_address: None,
            expires_after: None,
        };
        let with_expiry = SignRequest {
            expires_after: Some(1640995260000),
            ..without_expiry.clone()
        };

        let without_expiry_id = signer.compute_connection_id(&without_expiry).unwrap();
        let with_expiry_id = signer.compute_connection_id(&with_expiry).unwrap();

        assert_ne!(
            without_expiry_id, with_expiry_id,
            "expiresAfter must be part of the L1 action hash",
        );
    }

    #[rstest]
    fn test_connection_id_with_vault_matches_reference() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        let typed_action = Action::Order(BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: Default::default(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        });
        let action_bytes = rmp_serde::to_vec_named(&typed_action).unwrap();
        let request = SignRequest {
            action: None,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1640995200000),
            action_type: HyperliquidActionType::L1,
            is_testnet: true,
            vault_address: Some(
                VaultAddress::parse("0xAbCdEf0123456789AbCdEf0123456789AbCdEf01").unwrap(),
            ),
            expires_after: None,
        };

        let connection_id = signer.compute_connection_id(&request).unwrap();

        assert_eq!(
            hex::encode(connection_id.as_slice()),
            "edc33e36cec99166e20ea113da7e7b028cb94efda22813f814752d719a272757",
            "connection ID must match the L1 vault signing reference",
        );
    }

    #[rstest]
    fn test_connection_id_with_cloid() {
        // Test with CLOID included - this is what production actually sends.
        // The key difference: production always includes a cloid field.

        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let _signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        // Create a cloid - this is how Python SDK expects it
        let cloid = Cloid::from_hex("0x1234567890abcdef1234567890abcdef").unwrap();
        println!("Cloid hex: {}", cloid.to_hex());

        let typed_action = Action::Order(BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: cloid.0.into(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        });

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
    fn test_cloid_from_client_order_id_is_deterministic() {
        let client_order_id = ClientOrderId::from("O-20241210-123456-001-001-1");
        let other_client_order_id = ClientOrderId::from("O-20241210-123456-001-001-2");
        let first = Cloid::from_client_order_id(client_order_id);
        let second = Cloid::from_client_order_id(client_order_id);
        let other = Cloid::from_client_order_id(other_client_order_id);

        let first_hex = first.to_hex();
        let second_hex = second.to_hex();
        let other_hex = other.to_hex();

        for hex in [&first_hex, &second_hex, &other_hex] {
            assert!(hex.starts_with("0x"));
            assert_eq!(hex.len(), 34);
            assert!(hex[2..].chars().all(|c| c.is_ascii_hexdigit()));
            assert!(hex[2..].chars().all(|c| !c.is_ascii_uppercase()));
        }

        assert_eq!(first_hex, "0x7824fcada984a4aa731780e8326c1932");
        assert_eq!(other_hex, "0x9012504833e63da1435c32e96ef8b873");
        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[rstest]
    fn test_cloid_from_client_order_id_has_varied_leading_bytes() {
        let cloids: Vec<_> = (0..100)
            .map(|i| {
                let client_order_id = ClientOrderId::from(format!("O-SAMPLE-{i:03}").as_str());
                Cloid::from_client_order_id(client_order_id)
            })
            .collect();

        let leading_bytes = cloids
            .iter()
            .map(|cloid| cloid.0[0])
            .collect::<AHashSet<_>>();

        let uuid_like = cloids.iter().filter(|cloid| cloid.is_uuid_v4()).count();
        assert!(uuid_like < cloids.len());
        assert!(leading_bytes.len() > 1);

        let unique = cloids.iter().collect::<AHashSet<_>>();
        assert_eq!(unique.len(), cloids.len());
    }

    #[rstest]
    fn test_production_like_order_with_deterministic_cloid() {
        let private_key = EvmPrivateKey::new(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        let signer = HyperliquidEip712Signer::new(&private_key).unwrap();

        // Production-like values
        let client_order_id = ClientOrderId::from("O-20241210-123456-001-001-1");
        let cloid = Cloid::from_client_order_id(client_order_id);

        println!("=== Production-like Order ===");
        println!("ClientOrderId: {client_order_id}");
        println!("Cloid: {}", cloid.to_hex());

        let typed_action = Action::Order(BatchOrder {
            orders: vec![OrderRequest {
                asset: 3, // BTC on testnet
                is_buy: true,
                limit_px: dec!(92572.0),
                sz: dec!(0.001),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: cloid.0.into(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        });

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
        let request = SignRequest {
            action: None,
            action_bytes: Some(action_bytes),
            time_nonce: TimeNonce::from_millis(1733833200000), // Dec 10, 2024
            action_type: HyperliquidActionType::L1,
            is_testnet: true, // source = "b"
            vault_address: None,
            expires_after: None,
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
        let sig_hex = result.signature.to_hex();
        println!("Signature: {}", sig_hex.expose_secret());
        assert!(sig_hex.expose_secret().starts_with("0x"));
        assert_eq!(sig_hex.expose_secret().len(), 132);
    }

    #[rstest]
    fn test_price_decimal_formatting() {
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

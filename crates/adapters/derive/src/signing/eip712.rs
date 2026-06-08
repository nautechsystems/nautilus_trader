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

//! EIP-712 typed-data hashing and signing for Derive self-custodial actions.
//!
//! ```text
//! action_hash = keccak256(abi.encode(
//!     [bytes32, uint256, uint256, address, bytes32, uint256, address, address],
//!     [ACTION_TYPEHASH, subaccount_id, nonce, module_address,
//!      keccak256(module_data_abi_encoded), signature_expiry_sec, owner, signer],
//! ))
//! typed_data_hash = keccak256(0x1901 || DOMAIN_SEPARATOR || action_hash)
//! signature = secp256k1_sign(typed_data_hash, signer_key)
//! ```

use alloy::{
    signers::{SignerSync, local::PrivateKeySigner},
    sol_types::SolValue,
};
use alloy_primitives::{Address, B256, U256, keccak256};
use thiserror::Error;

use crate::{
    common::consts::MIN_SIGNATURE_TTL,
    signing::{encoding::utc_now_ms, modules::ModuleData},
};

/// Errors raised while building or signing an EIP-712 action.
#[derive(Debug, Error)]
pub enum TypedDataError {
    /// `signature_expiry_sec` is at or before `now`, or shorter than the
    /// venue-required minimum TTL ([`MIN_SIGNATURE_TTL`]).
    #[error(
        "signature expiry {expiry} must be at least {min_ttl_secs}s in the future of now {now}"
    )]
    ExpiryTooSoon {
        /// Caller-supplied expiry (UNIX seconds).
        expiry: i64,
        /// Reference `now` (UNIX seconds).
        now: i64,
        /// Configured minimum TTL in seconds.
        min_ttl_secs: i64,
    },
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
    /// secp256k1 signing failed.
    #[error("signing failed: {message}")]
    SigningFailed {
        /// Signer error message.
        message: String,
    },
    /// The module-data ABI encoder rejected the payload (e.g. negative
    /// `max_fee`, decimal scaling overflow).
    #[error("module data encoding failed: {message}")]
    ModuleEncoding {
        /// Underlying module-encoder error message.
        message: String,
    },
}

/// Inputs to the EIP-712 action hash, common across all module variants.
#[derive(Debug, Clone)]
pub struct ActionContext {
    /// Subaccount identifier used in both the signing payload and the request.
    pub subaccount_id: u64,
    /// Per-action nonce (see [`crate::signing::nonce`]).
    pub nonce: u64,
    /// Per-action module contract address.
    pub module_address: Address,
    /// Signature expiry in UNIX seconds.
    pub signature_expiry_sec: i64,
    /// Smart-contract wallet address (`owner` slot in the EIP-712 payload).
    pub owner: Address,
    /// Session-key wallet address (`signer` slot in the EIP-712 payload).
    pub signer: Address,
}

/// Computes the EIP-712 action hash for a Derive self-custodial action.
///
/// `module_data_hash` must be `keccak256(module_data.to_abi_encoded())` from
/// the per-module encoder; see [`crate::signing::modules`].
#[must_use]
pub fn compute_action_hash(
    ctx: &ActionContext,
    module_data_hash: B256,
    action_typehash: B256,
) -> B256 {
    let tuple = (
        action_typehash,
        U256::from(ctx.subaccount_id),
        U256::from(ctx.nonce),
        ctx.module_address,
        module_data_hash,
        U256::from(ctx.signature_expiry_sec),
        ctx.owner,
        ctx.signer,
    );
    keccak256(tuple.abi_encode())
}

/// Composes the final EIP-712 typed-data hash to be signed by the session key.
///
/// `0x19 0x01 || domain_separator || action_hash`, then keccak256.
#[must_use]
pub fn compute_typed_data_hash(domain_separator: B256, action_hash: B256) -> B256 {
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.push(0x19);
    buf.push(0x01);
    buf.extend_from_slice(domain_separator.as_slice());
    buf.extend_from_slice(action_hash.as_slice());
    keccak256(&buf)
}

/// A self-custodial action ready to be sent to the venue once signed.
///
/// The struct binds the EIP-712 action context to the module-specific payload
/// and tracks the resulting 65-byte signature. Compose it via [`SignedAction::new`],
/// then call [`SignedAction::sign`] with the session-key signer.
#[derive(Debug)]
pub struct SignedAction<'a, M: ModuleData> {
    ctx: ActionContext,
    module_data: &'a M,
    domain_separator: B256,
    action_typehash: B256,
    signature: Option<[u8; 65]>,
}

impl<'a, M: ModuleData> SignedAction<'a, M> {
    /// Constructs a new unsigned action.
    #[must_use]
    pub fn new(
        ctx: ActionContext,
        module_data: &'a M,
        domain_separator: B256,
        action_typehash: B256,
    ) -> Self {
        Self {
            ctx,
            module_data,
            domain_separator,
            action_typehash,
            signature: None,
        }
    }

    /// Signs the action using the supplied secp256k1 session-key signer.
    ///
    /// Validates `signature_expiry_sec` against [`MIN_SIGNATURE_TTL`] before
    /// hashing; the venue rejects expiries less than five minutes in the
    /// future.
    ///
    /// # Errors
    ///
    /// Returns [`TypedDataError::ExpiryTooSoon`] when the configured expiry is
    /// closer to `now` than the venue minimum, [`TypedDataError::ClockBeforeEpoch`]
    /// when the system clock is invalid, [`TypedDataError::ModuleEncoding`]
    /// when the per-module ABI encoder rejects the payload, and
    /// [`TypedDataError::SigningFailed`] when the underlying secp256k1 signer
    /// errors.
    pub fn sign(&mut self, signer: &PrivateKeySigner) -> Result<[u8; 65], TypedDataError> {
        self.validate_expiry()?;

        let module_data_bytes =
            self.module_data
                .to_abi_encoded()
                .map_err(|e| TypedDataError::ModuleEncoding {
                    message: e.to_string(),
                })?;
        let module_data_hash = keccak256(module_data_bytes);
        let action_hash = compute_action_hash(&self.ctx, module_data_hash, self.action_typehash);
        let typed_data_hash = compute_typed_data_hash(self.domain_separator, action_hash);

        let signature =
            signer
                .sign_hash_sync(&typed_data_hash)
                .map_err(|e| TypedDataError::SigningFailed {
                    message: e.to_string(),
                })?;
        let bytes = signature.as_bytes();
        self.signature = Some(bytes);
        Ok(bytes)
    }

    /// Returns the signature as a `0x`-prefixed 130-character hex string.
    /// Panics if [`SignedAction::sign`] has not yet been called.
    ///
    /// # Panics
    ///
    /// Panics if [`SignedAction::sign`] has not been called.
    #[must_use]
    pub fn signature_hex(&self) -> String {
        let bytes = self.signature.expect("signature_hex called before sign");
        format!("0x{}", alloy_primitives::hex::encode(bytes))
    }

    /// Returns the signed action's subaccount id.
    #[must_use]
    pub const fn subaccount_id(&self) -> u64 {
        self.ctx.subaccount_id
    }

    /// Returns the signed action's nonce.
    #[must_use]
    pub const fn nonce(&self) -> u64 {
        self.ctx.nonce
    }

    /// Returns the signed action's session-key signer address.
    #[must_use]
    pub const fn signer_address(&self) -> Address {
        self.ctx.signer
    }

    /// Returns the signed action's signature expiry in UNIX seconds.
    #[must_use]
    pub const fn signature_expiry_sec(&self) -> i64 {
        self.ctx.signature_expiry_sec
    }

    fn validate_expiry(&self) -> Result<(), TypedDataError> {
        let now_ms = utc_now_ms().map_err(|_| TypedDataError::ClockBeforeEpoch)?;
        let now_secs = (now_ms / 1000) as i64;
        let min_ttl_secs = MIN_SIGNATURE_TTL.as_secs() as i64;
        if self.ctx.signature_expiry_sec < now_secs.saturating_add(min_ttl_secs) {
            return Err(TypedDataError::ExpiryTooSoon {
                expiry: self.ctx.signature_expiry_sec,
                now: now_secs,
                min_ttl_secs,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use alloy_primitives::{Signature, hex};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::signing::modules::trade::TradeModuleData;

    const SESSION_KEY_HEX: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";

    fn fixed_typehash() -> B256 {
        // Arbitrary but stable test typehash. Real value comes from Protocol
        // Constants at docs.derive.xyz.
        "0x1111111111111111111111111111111111111111111111111111111111111111"
            .parse()
            .unwrap()
    }

    fn fixed_domain() -> B256 {
        "0x2222222222222222222222222222222222222222222222222222222222222222"
            .parse()
            .unwrap()
    }

    fn module_addr() -> Address {
        "0x000000000000000000000000000000000000bbbb"
            .parse()
            .unwrap()
    }

    fn owner() -> Address {
        "0x000000000000000000000000000000000000aaaa"
            .parse()
            .unwrap()
    }

    fn fresh_expiry() -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        now + 3600
    }

    fn sample_trade() -> TradeModuleData {
        TradeModuleData {
            asset_address: "0x000000000000000000000000000000000000abcd"
                .parse()
                .unwrap(),
            sub_id: U256::from(42),
            limit_price: dec!(100),
            amount: dec!(1),
            max_fee: dec!(1000),
            recipient_id: 30769,
            is_bid: true,
        }
    }

    fn sample_ctx(signer: Address, expiry: i64) -> ActionContext {
        ActionContext {
            subaccount_id: 30769,
            nonce: 1_695_836_058_725_001,
            module_address: module_addr(),
            signature_expiry_sec: expiry,
            owner: owner(),
            signer,
        }
    }

    #[rstest]
    fn test_compute_action_hash_changes_with_subaccount() {
        let module_hash = keccak256(sample_trade().to_abi_encoded().unwrap());
        let mut ctx = sample_ctx(owner(), fresh_expiry());
        let h1 = compute_action_hash(&ctx, module_hash, fixed_typehash());
        ctx.subaccount_id += 1;
        let h2 = compute_action_hash(&ctx, module_hash, fixed_typehash());
        assert_ne!(h1, h2, "changing subaccount must change the hash");
    }

    #[rstest]
    fn test_compute_action_hash_pins_byte_layout() {
        // Lock the 8-field ABI tuple shape against drift. The order
        // (typehash, subaccount, nonce, module, module_data_hash, expiry,
        // owner, signer) is the load-bearing protocol contract for
        // byte-equivalence with the upstream derive_action_signing SDK; a
        // swap, drop, or reorder would slip past relative-change tests.
        let module_hash: B256 =
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .parse()
                .unwrap();
        let ctx = ActionContext {
            subaccount_id: 30769,
            nonce: 1_695_836_058_725_001,
            module_address: module_addr(),
            signature_expiry_sec: 1_700_000_000,
            owner: owner(),
            signer: "0x000000000000000000000000000000000000cccc"
                .parse()
                .unwrap(),
        };
        let hash = compute_action_hash(&ctx, module_hash, fixed_typehash());
        let expected = "0x509b526a0413577f827d7ebaf5b3fed1eb24bb480612b4e705e1001126f04a1b";
        assert_eq!(format!("{hash:?}"), expected, "action-hash layout drift");
    }

    #[rstest]
    fn test_compute_typed_data_hash_pins_byte_layout() {
        // Lock the 0x1901 || domain || action_hash composition. Reordering
        // domain and action_hash, or dropping the prefix, would alter the
        // exact byte value below.
        let action_hash: B256 =
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .parse()
                .unwrap();
        let hash = compute_typed_data_hash(fixed_domain(), action_hash);
        let expected = "0x939b63f7cb4f2902be3004edd4f758ce4af26b96d12fd0992957a4cf5d287312";
        assert_eq!(
            format!("{hash:?}"),
            expected,
            "typed-data hash composition drift",
        );
    }

    #[rstest]
    fn test_compute_typed_data_hash_includes_19_01_prefix() {
        // Construct a known input pair and verify the prefix participates by
        // showing the hash differs from the bare keccak of its components.
        let action_hash: B256 =
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .parse()
                .unwrap();
        let with_prefix = compute_typed_data_hash(fixed_domain(), action_hash);
        let mut bare = Vec::with_capacity(64);
        bare.extend_from_slice(fixed_domain().as_slice());
        bare.extend_from_slice(action_hash.as_slice());
        let without_prefix = keccak256(&bare);
        assert_ne!(
            with_prefix, without_prefix,
            "the 0x1901 prefix must change the digest",
        );
    }

    #[rstest]
    fn test_sign_rejects_expiry_that_is_too_soon() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let near_expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60; // only 1 minute, well under 5-minute MIN_SIGNATURE_TTL
        let ctx = sample_ctx(signer.address(), near_expiry);
        let trade = sample_trade();
        let mut action = SignedAction::new(ctx, &trade, fixed_domain(), fixed_typehash());
        let err = action.sign(&signer).expect_err("must reject near expiry");
        assert!(
            matches!(err, TypedDataError::ExpiryTooSoon { .. }),
            "expected ExpiryTooSoon, was {err:?}",
        );
    }

    #[rstest]
    fn test_sign_produces_recoverable_signature() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let ctx = sample_ctx(signer.address(), fresh_expiry());
        let trade = sample_trade();

        let module_data_hash = keccak256(trade.to_abi_encoded().unwrap());
        let action_hash = compute_action_hash(&ctx, module_data_hash, fixed_typehash());
        let typed_data_hash = compute_typed_data_hash(fixed_domain(), action_hash);

        let mut action = SignedAction::new(ctx, &trade, fixed_domain(), fixed_typehash());
        let raw = action.sign(&signer).expect("sign must succeed");
        assert_eq!(raw.len(), 65);

        // Recover the signer from the signature and verify it matches the
        // session key. This is the venue's verification path inverted.
        let signature = Signature::try_from(raw.as_slice()).expect("65-byte sig");
        let recovered = signature
            .recover_address_from_prehash(&typed_data_hash)
            .expect("recover");
        assert_eq!(recovered, signer.address());
    }

    #[rstest]
    fn test_sign_propagates_module_encoding_error() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let ctx = sample_ctx(signer.address(), fresh_expiry());
        let mut bad_trade = sample_trade();
        bad_trade.max_fee = dec!(-1);
        let mut action = SignedAction::new(ctx, &bad_trade, fixed_domain(), fixed_typehash());
        let err = action
            .sign(&signer)
            .expect_err("invalid trade input must surface as a typed error, not a panic");

        match err {
            TypedDataError::ModuleEncoding { message } => {
                assert!(message.contains("max_fee"), "unexpected message: {message}");
            }
            other => panic!("expected ModuleEncoding, was {other:?}"),
        }
    }

    #[rstest]
    fn test_signed_action_accessors_expose_request_envelope_fields() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let ctx = sample_ctx(signer.address(), fresh_expiry());
        let trade = sample_trade();
        let mut action = SignedAction::new(ctx, &trade, fixed_domain(), fixed_typehash());
        action.sign(&signer).unwrap();

        let sig = action.signature_hex();
        assert!(sig.starts_with("0x"));
        assert_eq!(sig.len(), 2 + 130, "0x + 65 bytes hex = 132 chars");
        assert_eq!(action.nonce(), 1_695_836_058_725_001_u64);
        assert_eq!(action.subaccount_id(), 30769);
        assert_eq!(action.signer_address(), signer.address());
        assert!(action.signature_expiry_sec() > 0);
        // Decoding the hex back produces 65 bytes
        let bytes = hex::decode(sig.trim_start_matches("0x")).unwrap();
        assert_eq!(bytes.len(), 65);
    }
}

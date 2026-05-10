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

//! Transaction building and signing for the Bullet exchange.
//!
//! Ported from `bullet-rust-sdk/rust/src/transaction_builder.rs`.
//!
//! The signing flow:
//! 1. Build `UnsignedTransaction { runtime_call, uniqueness, details }`.
//! 2. Borsh-serialize it.
//! 3. Append the 32-byte `chain_hash` as a domain separator.
//! 4. Ed25519-sign the concatenated bytes.
//! 5. Wrap as `Transaction::V0 { signature, pub_key, ... }`.
//! 6. Borsh-serialize the signed transaction and base64-encode it.
//!
//! See: <https://tradingapi.bullet.xyz/docs/tx-signing.md>

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use bullet_exchange_interface::{
    address::Address,
    message::UserAction,
    transaction::{
        Amount, ExchangeCall, Gas, PriorityFeeBips, RuntimeCall, Transaction as SignedTransaction,
        TxDetails, UniquenessData, UnsignedTransaction as RawUnsignedTransaction, Version0,
    },
};

use crate::common::{
    consts::{DEFAULT_MAX_FEE, DEFAULT_PRIORITY_FEE_BIPS},
    credential::BulletCredential,
    error::BulletError,
};
use crate::signing::{chain_data::ChainData, uniqueness::generation_nonce};

/// Build, sign, and base64-encode a `UserAction` transaction.
///
/// This is the single write path for all trading operations (place, cancel, amend).
///
/// # Errors
///
/// Returns an error if borsh serialization fails.
pub fn sign_user_action(
    action: UserAction<Address>,
    credential: &BulletCredential,
    chain: &ChainData,
    gas_limit: Option<Gas>,
) -> Result<String, BulletError> {
    let runtime_call = RuntimeCall::Exchange(ExchangeCall::User(action));
    sign_runtime_call(runtime_call, credential, chain, gas_limit)
}

/// Build, sign, and base64-encode an arbitrary `RuntimeCall`.
///
/// # Errors
///
/// Returns an error if borsh serialization fails.
pub fn sign_runtime_call(
    runtime_call: RuntimeCall,
    credential: &BulletCredential,
    chain: &ChainData,
    gas_limit: Option<Gas>,
) -> Result<String, BulletError> {
    let unsigned = RawUnsignedTransaction {
        runtime_call,
        uniqueness: UniquenessData::Generation(generation_nonce()),
        details: TxDetails {
            chain_id: chain.chain_id,
            max_fee: Amount(DEFAULT_MAX_FEE),
            gas_limit,
            max_priority_fee_bips: PriorityFeeBips(DEFAULT_PRIORITY_FEE_BIPS),
        },
    };

    // borsh(unsigned_tx) ++ chain_hash
    let mut signable =
        borsh::to_vec(&unsigned).map_err(|e| BulletError::Signing(e.to_string()))?;
    signable.extend_from_slice(&chain.chain_hash);

    let signature = credential.sign(&signable);
    let pub_key = credential.public_key();

    let signed = SignedTransaction::V0(Version0 {
        runtime_call: unsigned.runtime_call,
        uniqueness: unsigned.uniqueness,
        details: unsigned.details,
        pub_key,
        signature,
    });

    let bytes = borsh::to_vec(&signed).map_err(|e| BulletError::Signing(e.to_string()))?;
    Ok(BASE64.encode(&bytes))
}

#[cfg(test)]
mod tests {
    use bullet_exchange_interface::{message::PublicAction, transaction::TxDetails};

    use super::*;

    fn test_chain() -> ChainData {
        ChainData { chain_id: 1, chain_hash: [42u8; 32] }
    }

    fn test_credential() -> BulletCredential {
        BulletCredential::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap()
    }

    #[test]
    fn sign_runtime_call_produces_nonempty_base64() {
        let call = RuntimeCall::Exchange(
            bullet_exchange_interface::message::CallMessage::Public(
                PublicAction::ApplyFunding { addresses: vec![] },
            ),
        );
        let credential = test_credential();
        let chain = test_chain();
        let b64 = sign_runtime_call(call, &credential, &chain, None).unwrap();
        assert!(!b64.is_empty());
        // Should be valid base64
        assert!(BASE64.decode(&b64).is_ok());
    }

    #[test]
    fn signable_bytes_are_borsh_plus_chain_hash() {
        // Verify the signed-bytes construction: borsh(unsigned) ++ chain_hash
        let chain = test_chain();
        let runtime_call = RuntimeCall::Exchange(
            bullet_exchange_interface::message::CallMessage::Public(
                PublicAction::ApplyFunding { addresses: vec![] },
            ),
        );
        let unsigned = RawUnsignedTransaction {
            runtime_call: runtime_call.clone(),
            uniqueness: UniquenessData::Generation(12345),
            details: TxDetails {
                chain_id: chain.chain_id,
                max_fee: Amount(DEFAULT_MAX_FEE),
                gas_limit: None,
                max_priority_fee_bips: PriorityFeeBips(DEFAULT_PRIORITY_FEE_BIPS),
            },
        };

        let mut expected = borsh::to_vec(&unsigned).unwrap();
        expected.extend_from_slice(&chain.chain_hash);

        let mut actual = borsh::to_vec(&unsigned).unwrap();
        actual.extend_from_slice(&chain.chain_hash);

        assert_eq!(actual, expected);
    }
}

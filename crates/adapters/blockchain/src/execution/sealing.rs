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

use ahash::AHashMap;
use alloy::{
    hex,
    primitives::{Address, B256, Bytes, U256},
};
use anyhow::Context;
use aws_lc_rs::{
    aead::{AES_256_GCM, Aad, NONCE_LEN, Nonce, RandomizedNonceKey},
    digest::{SHA256, digest},
};
use zeroize::Zeroizing;

use crate::{
    cache::rows::{ExecutionIntentRow, ExecutionTransactionHashRow},
    execution::transaction::{SignedTransactionIntent, validate_signed_transaction},
};

const ENVELOPE_VERSION: u8 = 1;
const KEY_ID_LEN: usize = 32;
const TAG_LEN: usize = 16;
const ENVELOPE_HEADER_LEN: usize = 1 + KEY_ID_LEN + NONCE_LEN;
const AAD_DOMAIN: &[u8] = b"nautilus:blockchain:signed-transaction";

pub(crate) const MAX_SIGNED_TRANSACTION_BYTES: usize = 128 * 1024;
pub(crate) const MAX_SEALED_TRANSACTION_BYTES: usize =
    ENVELOPE_HEADER_LEN + MAX_SIGNED_TRANSACTION_BYTES + TAG_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadContext {
    pub deployment_id: String,
    pub chain_id: u32,
    pub signer: Address,
    pub intent_id: i64,
    pub signer_nonce: u64,
    pub transaction_hash: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadPolicy {
    pub chain_id: u32,
    pub signer: Address,
    pub gas_limit: u64,
    pub max_fee_per_gas: u64,
}

pub(crate) struct PayloadKeySet {
    active_id: [u8; KEY_ID_LEN],
    keys: AHashMap<[u8; KEY_ID_LEN], RandomizedNonceKey>,
    deployment_id: String,
}

impl Debug for PayloadKeySet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PayloadKeySet))
            .field("active_id", &hex::encode(self.active_id))
            .field("key_count", &self.keys.len())
            .field("deployment_id", &self.deployment_id)
            .finish()
    }
}

impl PayloadKeySet {
    pub(crate) fn load(
        active_env: Option<&str>,
        retired_envs: &[String],
        deployment_id: Option<&str>,
    ) -> anyhow::Result<Option<Self>> {
        let Some(active_env) = active_env else {
            anyhow::ensure!(
                retired_envs.is_empty() && deployment_id.is_none(),
                "Payload deployment or retired keys require an active payload sealing key"
            );
            return Ok(None);
        };
        anyhow::ensure!(
            !active_env.trim().is_empty(),
            "Payload key environment name is empty"
        );
        let deployment_id = deployment_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Payload deployment ID is required when payload sealing is configured"
                )
            })?;

        let active = load_key(active_env)?;
        let mut retired = Vec::with_capacity(retired_envs.len());
        for env in retired_envs {
            anyhow::ensure!(
                !env.trim().is_empty(),
                "Retired payload key environment name is empty"
            );
            retired.push(load_key(env)?);
        }

        Self::from_key_bytes(active, retired, deployment_id.to_string()).map(Some)
    }

    fn from_key_bytes(
        active: Zeroizing<[u8; 32]>,
        retired: Vec<Zeroizing<[u8; 32]>>,
        deployment_id: String,
    ) -> anyhow::Result<Self> {
        let active_id = key_id(&active);
        let mut keys = AHashMap::with_capacity(1 + retired.len());
        keys.insert(active_id, payload_key(&active)?);

        for key in retired {
            let id = key_id(&key);
            if let std::collections::hash_map::Entry::Vacant(entry) = keys.entry(id) {
                entry.insert(payload_key(&key)?);
            }
        }

        Ok(Self {
            active_id,
            keys,
            deployment_id,
        })
    }

    #[must_use]
    pub(crate) const fn active_key_id(&self) -> &[u8; KEY_ID_LEN] {
        &self.active_id
    }

    #[must_use]
    pub(crate) fn deployment_id(&self) -> &str {
        &self.deployment_id
    }

    #[must_use]
    pub(crate) fn contains_key(&self, id: &[u8; KEY_ID_LEN]) -> bool {
        self.keys.contains_key(id)
    }

    pub(crate) fn seal(
        &self,
        plaintext: &[u8],
        context: &PayloadContext,
    ) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            plaintext.len() <= MAX_SIGNED_TRANSACTION_BYTES,
            "Signed transaction payload is {} bytes, exceeding the {} byte limit",
            plaintext.len(),
            MAX_SIGNED_TRANSACTION_BYTES
        );
        validate_context(context, &self.deployment_id)?;

        let key = self
            .keys
            .get(&self.active_id)
            .expect("active payload key missing from key set");
        let aad = encode_aad(&self.active_id, context)?;
        let mut ciphertext = plaintext.to_vec();
        let nonce = key
            .seal_in_place_append_tag(Aad::from(aad), &mut ciphertext)
            .map_err(|_| anyhow::anyhow!("Failed to seal signed transaction payload"))?;

        let mut envelope = Vec::with_capacity(ENVELOPE_HEADER_LEN + ciphertext.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&self.active_id);
        envelope.extend_from_slice(nonce.as_ref());
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    pub(crate) fn unseal(
        &self,
        envelope: &[u8],
        context: &PayloadContext,
    ) -> anyhow::Result<Vec<u8>> {
        validate_context(context, &self.deployment_id)?;
        let parsed = parse_envelope(envelope)?;
        let key = self.keys.get(&parsed.key_id).ok_or_else(|| {
            anyhow::anyhow!(
                "Payload sealing key {} is not configured",
                hex::encode(parsed.key_id)
            )
        })?;
        let aad = encode_aad(&parsed.key_id, context)?;
        let nonce = Nonce::try_assume_unique_for_key(parsed.nonce)
            .map_err(|_| anyhow::anyhow!("Signed transaction payload nonce is invalid"))?;
        let mut plaintext = parsed.ciphertext_and_tag.to_vec();
        let plaintext_len = key
            .open_in_place(nonce, Aad::from(aad), &mut plaintext)
            .map_err(|_| anyhow::anyhow!("Signed transaction payload authentication failed"))?
            .len();
        plaintext.truncate(plaintext_len);
        anyhow::ensure!(
            plaintext.len() <= MAX_SIGNED_TRANSACTION_BYTES,
            "Unsealed transaction payload is {} bytes, exceeding the {} byte limit",
            plaintext.len(),
            MAX_SIGNED_TRANSACTION_BYTES
        );
        Ok(plaintext)
    }
}

pub(crate) fn authenticate_payload(
    raw_transaction: &[u8],
    intent: &ExecutionIntentRow,
    hash: &ExecutionTransactionHashRow,
    policy: PayloadPolicy,
    deployment_id: &str,
) -> anyhow::Result<PayloadContext> {
    let context = payload_context(intent, hash, deployment_id)?;
    authenticate_payload_identity(
        raw_transaction,
        intent,
        &hash.transaction_hash,
        hash.chain_id,
        policy,
    )?;
    Ok(context)
}

pub(crate) fn authenticate_payload_identity(
    raw_transaction: &[u8],
    intent: &ExecutionIntentRow,
    transaction_hash: &str,
    row_chain_id: u32,
    policy: PayloadPolicy,
) -> anyhow::Result<()> {
    let durable_signer = persisted_signer(intent)?;
    authenticate_payload_identity_with_signer(
        raw_transaction,
        intent,
        transaction_hash,
        row_chain_id,
        durable_signer,
        policy,
    )
}

pub(crate) fn authenticate_retained_payload(
    raw_transaction: &[u8],
    intent: &ExecutionIntentRow,
    hash: &ExecutionTransactionHashRow,
    deployment_id: &str,
) -> anyhow::Result<PayloadContext> {
    let context = payload_context(intent, hash, deployment_id)?;
    let durable_signer = persisted_signer(intent)?;
    authenticate_payload_identity_with_signer(
        raw_transaction,
        intent,
        &hash.transaction_hash,
        hash.chain_id,
        durable_signer,
        PayloadPolicy {
            chain_id: intent.chain_id,
            signer: durable_signer,
            gas_limit: u64::MAX,
            max_fee_per_gas: u64::MAX,
        },
    )?;
    Ok(context)
}

pub(crate) fn retained_payload_requires_policy(
    intent: &ExecutionIntentRow,
    hash: &ExecutionTransactionHashRow,
    policy: PayloadPolicy,
) -> anyhow::Result<bool> {
    Ok(intent.chain_id == policy.chain_id
        && persisted_signer(intent)? == policy.signer
        && intent.active
        && hash.current
        && !matches!(intent.status.as_str(), "finalized" | "reverted"))
}

fn authenticate_payload_identity_with_signer(
    raw_transaction: &[u8],
    intent: &ExecutionIntentRow,
    transaction_hash: &str,
    row_chain_id: u32,
    durable_signer: Address,
    policy: PayloadPolicy,
) -> anyhow::Result<()> {
    let nonce = intent
        .nonce
        .ok_or_else(|| anyhow::anyhow!("Execution intent {} has no signer nonce", intent.id))?;
    let transaction_hash = B256::from_str(transaction_hash).with_context(|| {
        format!(
            "Execution intent {} has invalid transaction hash {transaction_hash}",
            intent.id
        )
    })?;
    let (to, input, value) = persisted_call_fields(intent)?;
    validate_signed_transaction(
        raw_transaction,
        &SignedTransactionIntent {
            hash: transaction_hash,
            signer: policy.signer,
            durable_signer,
            chain_id: policy.chain_id,
            intent_chain_id: intent.chain_id,
            row_chain_id,
            nonce,
            to,
            value,
            input,
            gas_limit: policy.gas_limit,
            max_fee_per_gas: policy.max_fee_per_gas,
        },
    )
}

fn persisted_signer(intent: &ExecutionIntentRow) -> anyhow::Result<Address> {
    Address::from_str(&intent.wallet_address).context("persisted execution wallet is invalid")
}

pub(crate) fn payload_context(
    intent: &ExecutionIntentRow,
    hash: &ExecutionTransactionHashRow,
    deployment_id: &str,
) -> anyhow::Result<PayloadContext> {
    anyhow::ensure!(
        hash.intent_id == intent.id,
        "Persisted transaction row references intent {}, expected {}",
        hash.intent_id,
        intent.id
    );
    payload_context_identity(intent, &hash.transaction_hash, hash.chain_id, deployment_id)
}

pub(crate) fn payload_context_identity(
    intent: &ExecutionIntentRow,
    transaction_hash: &str,
    row_chain_id: u32,
    deployment_id: &str,
) -> anyhow::Result<PayloadContext> {
    let nonce = intent
        .nonce
        .ok_or_else(|| anyhow::anyhow!("Execution intent {} has no signer nonce", intent.id))?;
    let transaction_hash = B256::from_str(transaction_hash).with_context(|| {
        format!(
            "Execution intent {} has invalid transaction hash {transaction_hash}",
            intent.id
        )
    })?;
    let durable_signer = Address::from_str(&intent.wallet_address)
        .context("persisted execution wallet is invalid")?;
    Ok(PayloadContext {
        deployment_id: deployment_id.to_string(),
        chain_id: row_chain_id,
        signer: durable_signer,
        intent_id: intent.id,
        signer_nonce: nonce,
        transaction_hash,
    })
}

pub(crate) fn persisted_call_fields(
    intent: &ExecutionIntentRow,
) -> anyhow::Result<(Address, Bytes, U256)> {
    let to = Address::from_str(&intent.transaction_to)
        .context("persisted execution destination is invalid")?;
    let input = hex::decode(
        intent
            .transaction_input
            .strip_prefix("0x")
            .unwrap_or(&intent.transaction_input),
    )
    .context("persisted execution calldata is invalid")?;
    let value = U256::from_str(&intent.transaction_value)
        .context("persisted execution value is invalid")?;

    Ok((to, Bytes::from(input), value))
}

pub(crate) fn envelope_key_id(envelope: &[u8]) -> anyhow::Result<[u8; KEY_ID_LEN]> {
    Ok(parse_envelope(envelope)?.key_id)
}

struct ParsedEnvelope<'a> {
    key_id: [u8; KEY_ID_LEN],
    nonce: &'a [u8],
    ciphertext_and_tag: &'a [u8],
}

fn parse_envelope(envelope: &[u8]) -> anyhow::Result<ParsedEnvelope<'_>> {
    anyhow::ensure!(
        envelope.len() <= MAX_SEALED_TRANSACTION_BYTES,
        "Sealed transaction payload is {} bytes, exceeding the {} byte limit",
        envelope.len(),
        MAX_SEALED_TRANSACTION_BYTES
    );
    anyhow::ensure!(
        envelope.len() >= ENVELOPE_HEADER_LEN + TAG_LEN,
        "Sealed transaction payload is truncated"
    );
    anyhow::ensure!(
        envelope[0] == ENVELOPE_VERSION,
        "Unsupported signed transaction payload envelope version {}",
        envelope[0]
    );

    let key_id = envelope[1..1 + KEY_ID_LEN]
        .try_into()
        .expect("fixed key ID slice length");
    let nonce_start = 1 + KEY_ID_LEN;
    let ciphertext_start = nonce_start + NONCE_LEN;
    Ok(ParsedEnvelope {
        key_id,
        nonce: &envelope[nonce_start..ciphertext_start],
        ciphertext_and_tag: &envelope[ciphertext_start..],
    })
}

fn validate_context(context: &PayloadContext, deployment_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        context.deployment_id == deployment_id,
        "Payload deployment ID does not match the configured key set"
    );
    anyhow::ensure!(
        context.intent_id > 0,
        "Payload intent ID {} is not positive",
        context.intent_id
    );
    Ok(())
}

fn encode_aad(key_id: &[u8; KEY_ID_LEN], context: &PayloadContext) -> anyhow::Result<Vec<u8>> {
    let mut aad = Vec::with_capacity(
        AAD_DOMAIN.len()
            + context.deployment_id.len()
            + 20
            + 32
            + KEY_ID_LEN
            + 9 * size_of::<u32>(),
    );
    append_aad_field(&mut aad, AAD_DOMAIN)?;
    append_aad_field(&mut aad, &[ENVELOPE_VERSION])?;
    append_aad_field(&mut aad, key_id)?;
    append_aad_field(&mut aad, context.deployment_id.as_bytes())?;
    append_aad_field(&mut aad, &context.chain_id.to_be_bytes())?;
    append_aad_field(&mut aad, context.signer.as_slice())?;
    append_aad_field(&mut aad, &context.intent_id.to_be_bytes())?;
    append_aad_field(&mut aad, &context.signer_nonce.to_be_bytes())?;
    append_aad_field(&mut aad, context.transaction_hash.as_slice())?;
    Ok(aad)
}

fn append_aad_field(output: &mut Vec<u8>, value: &[u8]) -> anyhow::Result<()> {
    let len = u32::try_from(value.len()).context("payload AAD field exceeds u32 length")?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

fn load_key(env: &str) -> anyhow::Result<Zeroizing<[u8; 32]>> {
    let value =
        Zeroizing::new(std::env::var(env).with_context(|| {
            format!("Payload sealing key environment variable '{env}' is not set")
        })?);
    decode_key(value.trim())
        .with_context(|| format!("Payload sealing key in '{env}' is not a valid 32-byte hex key"))
}

fn decode_key(value: &str) -> anyhow::Result<Zeroizing<[u8; 32]>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    hex::decode_to_array::<_, 32>(value)
        .map(Zeroizing::new)
        .map_err(anyhow::Error::from)
}

fn key_id(key: &[u8; 32]) -> [u8; KEY_ID_LEN] {
    digest(&SHA256, key)
        .as_ref()
        .try_into()
        .expect("SHA-256 output is 32 bytes")
}

fn payload_key(key: &[u8; 32]) -> anyhow::Result<RandomizedNonceKey> {
    RandomizedNonceKey::new(&AES_256_GCM, key)
        .map_err(|_| anyhow::anyhow!("Failed to initialize AES-256-GCM payload key"))
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};
    use rstest::rstest;

    use super::*;

    const ACTIVE_KEY: [u8; 32] = [0x11; 32];
    const RETIRED_KEY: [u8; 32] = [0x22; 32];
    const OTHER_KEY: [u8; 32] = [0x33; 32];

    fn key_set(active: [u8; 32], retired: Vec<[u8; 32]>) -> PayloadKeySet {
        PayloadKeySet::from_key_bytes(
            Zeroizing::new(active),
            retired.into_iter().map(Zeroizing::new).collect(),
            "deployment-a".to_string(),
        )
        .unwrap()
    }

    fn context() -> PayloadContext {
        PayloadContext {
            deployment_id: "deployment-a".to_string(),
            chain_id: 42161,
            signer: address!("49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
            intent_id: 17,
            signer_nonce: 29,
            transaction_hash: b256!(
                "c1b73f32adf343e8408cc5a566af9a3fb975135459a10e4919b2a4c909486582"
            ),
        }
    }

    #[rstest]
    fn exact_bytes_round_trip() {
        let keys = key_set(ACTIVE_KEY, vec![]);
        let plaintext = vec![0x02, 0xf8, 0x72, 0x01, 0xde, 0xad, 0xbe, 0xef];

        let envelope = keys.seal(&plaintext, &context()).unwrap();
        let unsealed = keys.unseal(&envelope, &context()).unwrap();

        assert_eq!(unsealed, plaintext);
        assert_eq!(envelope_key_id(&envelope).unwrap(), key_id(&ACTIVE_KEY));
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(1 + KEY_ID_LEN)]
    #[case(ENVELOPE_HEADER_LEN)]
    #[case(usize::MAX)]
    fn tampering_is_rejected(#[case] index: usize) {
        let keys = key_set(ACTIVE_KEY, vec![]);
        let mut envelope = keys.seal(b"signed transaction", &context()).unwrap();
        let index = if index == usize::MAX {
            envelope.len() - 1
        } else {
            index
        };
        envelope[index] ^= 0x01;

        assert!(keys.unseal(&envelope, &context()).is_err());
    }

    #[rstest]
    #[case("deployment")]
    #[case("chain")]
    #[case("signer")]
    #[case("intent")]
    #[case("nonce")]
    #[case("hash")]
    fn aad_context_mismatch_is_rejected(#[case] field: &str) {
        let keys = key_set(ACTIVE_KEY, vec![]);
        let envelope = keys.seal(b"signed transaction", &context()).unwrap();
        let mut other = context();
        match field {
            "deployment" => other.deployment_id = "deployment-b".to_string(),
            "chain" => other.chain_id += 1,
            "signer" => other.signer = Address::ZERO,
            "intent" => other.intent_id += 1,
            "nonce" => other.signer_nonce += 1,
            "hash" => other.transaction_hash = B256::ZERO,
            _ => unreachable!(),
        }

        assert!(keys.unseal(&envelope, &other).is_err());
    }

    #[rstest]
    fn wrong_key_is_rejected() {
        let keys = key_set(ACTIVE_KEY, vec![]);
        let envelope = keys.seal(b"signed transaction", &context()).unwrap();
        let other = key_set(OTHER_KEY, vec![]);

        assert!(other.unseal(&envelope, &context()).is_err());
    }

    #[rstest]
    fn retired_key_unseals_but_active_key_seals() {
        let retired = key_set(RETIRED_KEY, vec![]);
        let old_envelope = retired.seal(b"old transaction", &context()).unwrap();
        let rotated = key_set(ACTIVE_KEY, vec![RETIRED_KEY]);

        let old_plaintext = rotated.unseal(&old_envelope, &context()).unwrap();
        let new_envelope = rotated.seal(b"new transaction", &context()).unwrap();

        assert_eq!(old_plaintext, b"old transaction");
        assert_eq!(envelope_key_id(&new_envelope).unwrap(), key_id(&ACTIVE_KEY));
    }

    #[rstest]
    fn oversized_values_are_rejected_before_crypto() {
        let keys = key_set(ACTIVE_KEY, vec![]);
        let plaintext = vec![0; MAX_SIGNED_TRANSACTION_BYTES + 1];
        let envelope = vec![0; MAX_SEALED_TRANSACTION_BYTES + 1];

        assert!(keys.seal(&plaintext, &context()).is_err());
        assert!(keys.unseal(&envelope, &context()).is_err());
    }

    #[rstest]
    fn malformed_key_and_envelope_are_rejected() {
        assert!(decode_key("abcd").is_err());
        assert!(parse_envelope(&[]).is_err());
    }
}

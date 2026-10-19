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

//! EIP-1559 transaction building, signing, and fee and gas policy for execution operations.

use alloy::{
    consensus::{SignableTransaction, TxEip1559, TxEnvelope},
    eips::eip2718::{Decodable2718, Encodable2718},
    primitives::{Address, B256, Bytes, TxKind, U256},
    signers::{Signer, local::PrivateKeySigner},
};
use anyhow::Context;

const BPS_DENOMINATOR: u128 = 10_000;

/// Current version of the durable execution-intent schema.
pub const EXECUTION_SCHEMA_VERSION: i16 = 2;

/// The purpose of an execution transaction, persisted with the transaction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionPurpose {
    /// WETH `deposit()` wrapping native currency into the wrapped native token.
    Wrap,
    /// ERC-20 `approve` granting the router an allowance.
    Approve,
    /// Uniswap V3 `exactInputSingle` swap executing an order.
    Swap,
}

impl TransactionPurpose {
    /// Returns the persisted string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Wrap => "wrap",
            Self::Approve => "approve",
            Self::Swap => "swap",
        }
    }

    /// Parses a persisted transaction purpose.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wrap" => Some(Self::Wrap),
            "approve" => Some(Self::Approve),
            "swap" => Some(Self::Swap),
            _ => None,
        }
    }
}

/// The observation status of a persisted execution transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Intent persisted before nonce reservation or signing.
    Prepared,
    /// Signed hash persisted before a broadcast attempt.
    Signed,
    /// Broadcast attempted or accepted, awaiting chain inclusion.
    Broadcast,
    /// Broadcast accepted (or possibly accepted), awaiting inclusion.
    ///
    /// Retained for records written by execution schema version 1.
    Pending,
    /// Definitively rejected by the RPC node before acceptance.
    ///
    /// Retained for records written by execution schema version 1.
    Rejected,
    /// Receipt observed in a canonical block, but not yet finalized.
    Included,
    /// Successful receipt proved canonical at the finalized boundary.
    Finalized,
    /// Failed receipt proved canonical at the finalized boundary.
    Reverted,
    /// A different transaction hash consumed the owned signer nonce.
    Replaced,
    /// No canonical receipt was found in the bounded observation window.
    Dropped,
    /// A previously observed inclusion is no longer canonical.
    Reorged,
    /// No signed or possibly broadcast transaction remains and ownership may be released.
    Recoverable,
}

impl TransactionStatus {
    /// Returns the persisted string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Signed => "signed",
            Self::Broadcast => "broadcast",
            Self::Pending => "pending",
            Self::Rejected => "rejected",
            Self::Included => "included",
            Self::Finalized => "finalized",
            Self::Reverted => "reverted",
            Self::Replaced => "replaced",
            Self::Dropped => "dropped",
            Self::Reorged => "reorged",
            Self::Recoverable => "recoverable",
        }
    }
}

/// Builds an unsigned EIP-1559 transaction for a contract call.
#[must_use]
#[expect(clippy::too_many_arguments)]
pub fn build_eip1559_transaction(
    chain_id: u64,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    to: Address,
    value: U256,
    input: Bytes,
) -> TxEip1559 {
    TxEip1559 {
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: TxKind::Call(to),
        value,
        access_list: Default::default(),
        input,
    }
}

/// Signs an unsigned EIP-1559 transaction locally and returns the transaction hash together
/// with the raw EIP-2718 encoding accepted by `eth_sendRawTransaction`.
///
/// # Errors
///
/// Returns an error if signing fails.
pub async fn sign_eip1559_transaction(
    tx: TxEip1559,
    signer: &PrivateKeySigner,
) -> anyhow::Result<(B256, Vec<u8>)> {
    let signature = signer
        .sign_hash(&tx.signature_hash())
        .await
        .context("failed to sign transaction")?;
    let signed = tx.into_signed(signature);
    let tx_hash = *signed.hash();

    Ok((tx_hash, signed.encoded_2718()))
}

/// Durable identity and configured policy required to authenticate a signed transaction.
pub(super) struct SignedTransactionIntent {
    pub hash: B256,
    pub signer: Address,
    pub durable_signer: Address,
    pub chain_id: u32,
    pub intent_chain_id: u32,
    pub row_chain_id: u32,
    pub nonce: u64,
    pub to: Address,
    pub value: U256,
    pub input: Bytes,
    pub gas_limit: u64,
    pub max_fee_per_gas: u64,
}

/// Authenticated fields decoded from one complete signed EIP-1559 transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecodedSignedTransaction {
    pub hash: B256,
    pub signer: Address,
    pub chain_id: u64,
    pub nonce: u64,
    pub to: Address,
    pub value: U256,
    pub input: Bytes,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

/// Decodes and authenticates one complete signed EIP-1559 transaction.
pub(super) fn decode_signed_transaction(
    raw_transaction: &[u8],
) -> anyhow::Result<DecodedSignedTransaction> {
    let envelope = TxEnvelope::decode_2718_exact(raw_transaction).map_err(|_| {
        anyhow::anyhow!("Persisted signed transaction is not a complete EIP-2718 envelope")
    })?;
    let TxEnvelope::Eip1559(signed) = envelope else {
        anyhow::bail!("Persisted signed transaction is not EIP-1559");
    };
    anyhow::ensure!(
        signed.signature().normalize_s().is_none(),
        "Persisted transaction signature is not EIP-2 normalized"
    );
    let signer = signed
        .signature()
        .recover_address_from_prehash(&signed.signature_hash())
        .context("failed to recover persisted transaction signer")?;
    let hash = *signed.hash();
    let tx = signed.tx();
    let TxKind::Call(to) = tx.to else {
        anyhow::bail!("Signed transaction creates a contract instead of calling a destination");
    };
    anyhow::ensure!(
        tx.access_list.is_empty(),
        "Signed transaction access list is not empty"
    );

    Ok(DecodedSignedTransaction {
        hash,
        signer,
        chain_id: tx.chain_id,
        nonce: tx.nonce,
        to,
        value: tx.value,
        input: tx.input.clone(),
        gas_limit: tx.gas_limit,
        max_fee_per_gas: tx.max_fee_per_gas,
        max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
    })
}

/// Authenticates one complete signed EIP-1559 call against its durable intent and policy.
pub(super) fn validate_signed_transaction(
    raw_transaction: &[u8],
    intent: &SignedTransactionIntent,
) -> anyhow::Result<()> {
    let tx = decode_signed_transaction(raw_transaction)?;

    anyhow::ensure!(
        tx.hash == intent.hash,
        "Persisted transaction hash {} does not match signed transaction hash {}",
        intent.hash,
        tx.hash
    );
    anyhow::ensure!(
        intent.durable_signer == intent.signer,
        "Persisted transaction signer {} does not match configured wallet {}",
        intent.durable_signer,
        intent.signer
    );
    anyhow::ensure!(
        tx.signer == intent.signer,
        "Signed transaction signer {} does not match configured wallet {}",
        tx.signer,
        intent.signer
    );
    anyhow::ensure!(
        intent.intent_chain_id == intent.chain_id,
        "Persisted intent chain ID {} does not match configured chain ID {}",
        intent.intent_chain_id,
        intent.chain_id
    );
    anyhow::ensure!(
        intent.row_chain_id == intent.chain_id,
        "Persisted transaction row chain ID {} does not match configured chain ID {}",
        intent.row_chain_id,
        intent.chain_id
    );

    anyhow::ensure!(
        tx.chain_id == u64::from(intent.chain_id),
        "Signed transaction chain ID {} does not match configured chain ID {}",
        tx.chain_id,
        intent.chain_id
    );
    anyhow::ensure!(
        tx.nonce == intent.nonce,
        "Signed transaction nonce {} does not match persisted nonce {}",
        tx.nonce,
        intent.nonce
    );
    anyhow::ensure!(
        tx.to == intent.to,
        "Signed transaction destination {} does not match persisted destination {}",
        tx.to,
        intent.to
    );
    anyhow::ensure!(
        tx.value == intent.value,
        "Signed transaction value does not match persisted value"
    );
    anyhow::ensure!(
        tx.input == intent.input,
        "Signed transaction calldata does not match persisted calldata"
    );
    anyhow::ensure!(
        tx.gas_limit <= intent.gas_limit,
        "Signed transaction gas limit {} exceeds configured ceiling {}",
        tx.gas_limit,
        intent.gas_limit
    );
    anyhow::ensure!(
        tx.max_fee_per_gas <= u128::from(intent.max_fee_per_gas),
        "Signed transaction max fee per gas {} wei exceeds configured ceiling {} wei",
        tx.max_fee_per_gas,
        intent.max_fee_per_gas
    );
    anyhow::ensure!(
        tx.max_priority_fee_per_gas <= tx.max_fee_per_gas,
        "Signed transaction priority fee per gas {} wei exceeds max fee per gas {} wei",
        tx.max_priority_fee_per_gas,
        tx.max_fee_per_gas
    );

    Ok(())
}

/// Applies `gas_buffer_bps` over the `eth_estimateGas` result.
///
/// A buffered estimate above `gas_limit` rejects the transaction rather than clamping to the
/// ceiling: on Arbitrum the estimate folds the L1 data fee into gas units, and clamping
/// guarantees a paid-for out-of-gas revert.
///
/// # Errors
///
/// Returns an error if the buffered estimate exceeds `gas_limit` or the arithmetic overflows.
pub fn derive_gas_limit(estimate: u64, gas_buffer_bps: u32, gas_limit: u64) -> anyhow::Result<u64> {
    let buffered = apply_buffer_bps(u128::from(estimate), gas_buffer_bps)?;

    if buffered > u128::from(gas_limit) {
        anyhow::bail!(
            "Estimated gas {estimate} with {gas_buffer_bps} bps buffer ({buffered}) exceeds gas limit {gas_limit}"
        );
    }

    u64::try_from(buffered).context("buffered gas limit overflow")
}

/// Derives EIP-1559 fees from the latest base fee and the node's suggested priority fee.
///
/// `max_fee_per_gas` is the base fee with `base_fee_buffer_bps` applied plus the priority fee;
/// `max_priority_fee_per_gas` is the priority fee itself. `max_fee_per_gas_wei` is a hard
/// ceiling that rejects the transaction when current conditions exceed it.
///
/// # Errors
///
/// Returns an error if the derived max fee exceeds `max_fee_per_gas_wei` or the arithmetic
/// overflows.
pub fn derive_fees(
    base_fee_per_gas_wei: u128,
    priority_fee_per_gas_wei: u128,
    base_fee_buffer_bps: u32,
    max_fee_per_gas_wei: u128,
) -> anyhow::Result<(u128, u128)> {
    let max_fee = compute_max_fee(
        base_fee_per_gas_wei,
        priority_fee_per_gas_wei,
        base_fee_buffer_bps,
    )?;

    if max_fee > max_fee_per_gas_wei {
        anyhow::bail!(
            "Derived max fee per gas {max_fee} wei exceeds ceiling {max_fee_per_gas_wei} wei"
        );
    }

    Ok((max_fee, priority_fee_per_gas_wei))
}

/// Computes the EIP-1559 max fee per gas: the base fee with `base_fee_buffer_bps` applied
/// plus the priority fee, without applying any ceiling.
///
/// # Errors
///
/// Returns an error if the arithmetic overflows.
pub fn compute_max_fee(
    base_fee_per_gas_wei: u128,
    priority_fee_per_gas_wei: u128,
    base_fee_buffer_bps: u32,
) -> anyhow::Result<u128> {
    let buffered_base_fee = apply_buffer_bps(base_fee_per_gas_wei, base_fee_buffer_bps)?;

    buffered_base_fee
        .checked_add(priority_fee_per_gas_wei)
        .context("max fee per gas overflow")
}

fn apply_buffer_bps(value: u128, buffer_bps: u32) -> anyhow::Result<u128> {
    let buffer_bps = u128::from(buffer_bps);
    let buffer_whole = (value / BPS_DENOMINATOR)
        .checked_mul(buffer_bps)
        .context("buffered value overflow")?;

    // Round up so the buffer is never silently reduced by integer division
    let buffer_remainder = (value % BPS_DENOMINATOR)
        .checked_mul(buffer_bps)
        .and_then(|v| v.checked_add(BPS_DENOMINATOR - 1))
        .map(|v| v / BPS_DENOMINATOR)
        .context("buffered value overflow")?;

    value
        .checked_add(buffer_whole)
        .and_then(|v| v.checked_add(buffer_remainder))
        .context("buffered value overflow")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::{
        consensus::TxEip2930,
        eips::eip2930::{AccessList, AccessListItem},
        primitives::{Signature, address, b256},
    };
    use nautilus_core::hex;
    use rstest::rstest;

    use super::*;

    // Reference vector produced independently with eth_account 0.13.7 (Python):
    // anvil development key 0xac0974...ff80 signing a WETH deposit() call on Arbitrum
    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const EXPECTED_RAW_TX: &str = "02f87682a4b10783989680840bebc20082fde89482af49447d8a07e3bd95bd0d56f35241523fbab187038d7ea4c6800084d0e30db0c080a0ecbbf3b95a4509c94cf0fe219c93a404c09de776a7073f2765709fe04f32f024a07b6a1f8332b39ca80ad3e61d124147af984ffcba1dd5579dbcf11e921ea3cecb";

    #[derive(Debug, Clone, Copy)]
    enum InvalidSignedField {
        Hash,
        Signer,
        DurableSigner,
        IntentChain,
        RowChain,
        TransactionChain,
        Nonce,
        CallType,
        Destination,
        Value,
        Input,
        GasLimit,
        MaxFee,
        PriorityFee,
        AccessList,
    }

    fn validation_transaction() -> TxEip1559 {
        build_eip1559_transaction(
            42161,
            7,
            65_000,
            200_000_000,
            10_000_000,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            U256::from(1_000_000_000_000_000u64),
            Bytes::from(hex::decode("d0e30db0").unwrap()),
        )
    }

    fn validation_intent(hash: B256, signer: Address) -> SignedTransactionIntent {
        SignedTransactionIntent {
            hash,
            signer,
            durable_signer: signer,
            chain_id: 42161,
            intent_chain_id: 42161,
            row_chain_id: 42161,
            nonce: 7,
            to: address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            value: U256::from(1_000_000_000_000_000u64),
            input: Bytes::from(hex::decode("d0e30db0").unwrap()),
            gas_limit: 1_000_000,
            max_fee_per_gas: 1_000_000_000,
        }
    }

    #[rstest]
    fn test_derive_gas_limit_applies_buffer_rounding_up() {
        assert_eq!(derive_gas_limit(50_001, 2_000, 1_000_000).unwrap(), 60_002);
    }

    #[rstest]
    fn test_derive_gas_limit_allows_estimate_at_ceiling() {
        assert_eq!(
            derive_gas_limit(1_000_000, 0, 1_000_000).unwrap(),
            1_000_000
        );
    }

    #[rstest]
    fn test_derive_gas_limit_rejects_above_ceiling_without_clamping() {
        let result = derive_gas_limit(900_000, 2_000, 1_000_000);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds gas limit 1000000")
        );
    }

    #[rstest]
    fn test_derive_fees_combines_buffered_base_fee_and_priority_fee() {
        let (max_fee, priority_fee) = derive_fees(100, 5, 2_000, 1_000).unwrap();

        assert_eq!(max_fee, 125);
        assert_eq!(priority_fee, 5);
    }

    #[rstest]
    fn test_derive_fees_rejects_above_ceiling() {
        let result = derive_fees(100, 5, 2_000, 124);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds ceiling 124 wei")
        );
    }

    #[rstest]
    fn test_derive_fees_allows_fee_at_ceiling() {
        assert!(derive_fees(100, 5, 2_000, 125).is_ok());
    }

    #[rstest]
    fn test_compute_max_fee_zero_buffer_accepts_maximum_value() {
        assert_eq!(compute_max_fee(u128::MAX, 0, 0).unwrap(), u128::MAX);
    }

    #[rstest]
    fn test_compute_max_fee_rejects_buffered_value_overflow() {
        let error = compute_max_fee(u128::MAX, 0, 1).unwrap_err();

        assert_eq!(error.to_string(), "buffered value overflow");
    }

    #[rstest]
    fn test_build_eip1559_transaction_populates_all_fields() {
        let to = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
        let input = Bytes::from(hex::decode("d0e30db0").unwrap());

        let tx = build_eip1559_transaction(
            42161,
            7,
            65_000,
            200_000_000,
            10_000_000,
            to,
            U256::from(1_000_000_000_000_000u64),
            input.clone(),
        );

        assert_eq!(tx.chain_id, 42161);
        assert_eq!(tx.nonce, 7);
        assert_eq!(tx.gas_limit, 65_000);
        assert_eq!(tx.max_fee_per_gas, 200_000_000);
        assert_eq!(tx.max_priority_fee_per_gas, 10_000_000);
        assert_eq!(tx.to, TxKind::Call(to));
        assert_eq!(tx.value, U256::from(1_000_000_000_000_000u64));
        assert!(tx.access_list.is_empty());
        assert_eq!(tx.input, input);
    }

    #[tokio::test]
    async fn test_sign_eip1559_transaction_matches_reference_vector() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            signer.address(),
            address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
        );

        let tx = build_eip1559_transaction(
            42161,
            7,
            65_000,
            200_000_000,
            10_000_000,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            U256::from(1_000_000_000_000_000u64),
            Bytes::from(hex::decode("d0e30db0").unwrap()),
        );

        let (tx_hash, raw_tx) = sign_eip1559_transaction(tx, &signer).await.unwrap();

        assert_eq!(
            tx_hash,
            b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a")
        );
        assert_eq!(hex::encode(&raw_tx), EXPECTED_RAW_TX);
    }

    #[tokio::test]
    async fn test_validate_signed_transaction_accepts_builder_output_at_policy_ceilings() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let mut transaction = validation_transaction();
        transaction.gas_limit = 1_000_000;
        transaction.max_fee_per_gas = 1_000_000_000;
        transaction.max_priority_fee_per_gas = 1_000_000_000;
        let (hash, raw_transaction) = sign_eip1559_transaction(transaction, &signer)
            .await
            .unwrap();
        let intent = validation_intent(hash, signer.address());

        validate_signed_transaction(&raw_transaction, &intent).unwrap();
    }

    #[rstest]
    fn test_validate_signed_transaction_rejects_malformed_bytes() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let intent = validation_intent(B256::ZERO, signer.address());

        let error = validate_signed_transaction(&[0x02, 0xc0], &intent).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Persisted signed transaction is not a complete EIP-2718 envelope"
        );
    }

    #[tokio::test]
    async fn test_validate_signed_transaction_rejects_trailing_bytes_without_exposing_them() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let (hash, mut raw_transaction) =
            sign_eip1559_transaction(validation_transaction(), &signer)
                .await
                .unwrap();
        raw_transaction.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let intent = validation_intent(hash, signer.address());

        let error = validate_signed_transaction(&raw_transaction, &intent).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Persisted signed transaction is not a complete EIP-2718 envelope"
        );
        assert!(!error.to_string().contains("deadbeef"));
    }

    #[tokio::test]
    async fn test_validate_signed_transaction_rejects_other_envelope_type() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let transaction = validation_transaction();
        let transaction = TxEip2930 {
            chain_id: transaction.chain_id,
            nonce: transaction.nonce,
            gas_price: transaction.max_fee_per_gas,
            gas_limit: transaction.gas_limit,
            to: transaction.to,
            value: transaction.value,
            access_list: transaction.access_list,
            input: transaction.input,
        };
        let signature = signer
            .sign_hash(&transaction.signature_hash())
            .await
            .unwrap();
        let raw_transaction = transaction.into_signed(signature).encoded_2718();
        let intent = validation_intent(B256::ZERO, signer.address());

        let error = validate_signed_transaction(&raw_transaction, &intent).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Persisted signed transaction is not EIP-1559"
        );
    }

    #[tokio::test]
    async fn test_validate_signed_transaction_rejects_noncanonical_signature() {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let transaction = validation_transaction();
        let signature = signer
            .sign_hash(&transaction.signature_hash())
            .await
            .unwrap();
        let curve_order =
            U256::from_str("0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                .unwrap();
        let signature = Signature::new(signature.r(), curve_order - signature.s(), !signature.v());
        let signed = transaction.into_signed(signature);
        let hash = *signed.hash();
        let raw_transaction = signed.encoded_2718();
        let intent = validation_intent(hash, signer.address());

        let error = validate_signed_transaction(&raw_transaction, &intent).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Persisted transaction signature is not EIP-2 normalized"
        );
    }

    #[rstest]
    #[case::hash(InvalidSignedField::Hash, "Persisted transaction hash")]
    #[case::signer(InvalidSignedField::Signer, "does not match configured wallet")]
    #[case::durable_signer(InvalidSignedField::DurableSigner, "Persisted transaction signer")]
    #[case::intent_chain(InvalidSignedField::IntentChain, "Persisted intent chain ID")]
    #[case::row_chain(InvalidSignedField::RowChain, "Persisted transaction row chain ID")]
    #[case::transaction_chain(InvalidSignedField::TransactionChain, "Signed transaction chain ID")]
    #[case::nonce(InvalidSignedField::Nonce, "Signed transaction nonce")]
    #[case::call_type(InvalidSignedField::CallType, "creates a contract")]
    #[case::destination(InvalidSignedField::Destination, "Signed transaction destination")]
    #[case::value(InvalidSignedField::Value, "Signed transaction value")]
    #[case::input(InvalidSignedField::Input, "Signed transaction calldata")]
    #[case::gas_limit(InvalidSignedField::GasLimit, "Signed transaction gas limit")]
    #[case::max_fee(InvalidSignedField::MaxFee, "Signed transaction max fee per gas")]
    #[case::priority_fee(
        InvalidSignedField::PriorityFee,
        "Signed transaction priority fee per gas"
    )]
    #[case::access_list(InvalidSignedField::AccessList, "Signed transaction access list")]
    #[tokio::test]
    async fn test_validate_signed_transaction_rejects_field_mismatch(
        #[case] field: InvalidSignedField,
        #[case] expected_error: &str,
    ) {
        let signer = PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap();
        let other = address!("0000000000000000000000000000000000000001");
        let mut transaction = validation_transaction();
        let mut intent = validation_intent(B256::ZERO, signer.address());

        match field {
            InvalidSignedField::Hash => {}
            InvalidSignedField::Signer => {
                intent.signer = other;
                intent.durable_signer = other;
            }
            InvalidSignedField::DurableSigner => intent.durable_signer = other,
            InvalidSignedField::IntentChain => intent.intent_chain_id = 1,
            InvalidSignedField::RowChain => intent.row_chain_id = 1,
            InvalidSignedField::TransactionChain => transaction.chain_id = 1,
            InvalidSignedField::Nonce => transaction.nonce = 8,
            InvalidSignedField::CallType => transaction.to = TxKind::Create,
            InvalidSignedField::Destination => transaction.to = TxKind::Call(other),
            InvalidSignedField::Value => transaction.value = U256::from(2u64),
            InvalidSignedField::Input => transaction.input = Bytes::from(vec![0x01]),
            InvalidSignedField::GasLimit => transaction.gas_limit = 1_000_001,
            InvalidSignedField::MaxFee => transaction.max_fee_per_gas = 1_000_000_001,
            InvalidSignedField::PriorityFee => {
                transaction.max_fee_per_gas = 10_000_000;
                transaction.max_priority_fee_per_gas = 10_000_001;
            }
            InvalidSignedField::AccessList => {
                transaction.access_list = AccessList(vec![AccessListItem {
                    address: other,
                    storage_keys: Vec::new(),
                }]);
            }
        }

        let (hash, raw_transaction) = sign_eip1559_transaction(transaction, &signer)
            .await
            .unwrap();

        if !matches!(field, InvalidSignedField::Hash) {
            intent.hash = hash;
        }

        let error = validate_signed_transaction(&raw_transaction, &intent).unwrap_err();

        assert!(error.to_string().contains(expected_error), "was: {error}");
    }
}

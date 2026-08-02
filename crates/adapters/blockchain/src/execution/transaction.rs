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
    consensus::{SignableTransaction, TxEip1559},
    eips::eip2718::Encodable2718,
    primitives::{Address, B256, Bytes, TxKind, U256},
    signers::{Signer, local::PrivateKeySigner},
};
use anyhow::Context;

const BPS_DENOMINATOR: u128 = 10_000;

/// The purpose of an execution transaction, persisted with the transaction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionPurpose {
    /// WETH `deposit()` wrapping native currency into the wrapped native token.
    Wrap,
    /// ERC-20 `approve` granting the router an allowance.
    Approve,
}

impl TransactionPurpose {
    /// Returns the persisted string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Wrap => "wrap",
            Self::Approve => "approve",
        }
    }
}

/// The observation status of a persisted execution transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Broadcast accepted (or possibly accepted), awaiting inclusion.
    Pending,
    /// Included on-chain with a successful receipt.
    Included,
    /// Included on-chain with a reverted receipt.
    Reverted,
}

impl TransactionStatus {
    /// Returns the persisted string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Included => "included",
            Self::Reverted => "reverted",
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
    let multiplier = BPS_DENOMINATOR
        .checked_add(u128::from(buffer_bps))
        .context("buffer bps overflow")?;

    // Round up so the buffer is never silently reduced by integer division
    value
        .checked_mul(multiplier)
        .and_then(|v| v.checked_add(BPS_DENOMINATOR - 1))
        .map(|v| v / BPS_DENOMINATOR)
        .context("buffered value overflow")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::primitives::{address, b256};
    use nautilus_core::hex;
    use rstest::rstest;

    use super::*;

    // Reference vector produced independently with eth_account 0.13.7 (Python):
    // anvil development key 0xac0974...ff80 signing a WETH deposit() call on Arbitrum
    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const EXPECTED_RAW_TX: &str = "02f87682a4b10783989680840bebc20082fde89482af49447d8a07e3bd95bd0d56f35241523fbab187038d7ea4c6800084d0e30db0c080a0ecbbf3b95a4509c94cf0fe219c93a404c09de776a7073f2765709fe04f32f024a07b6a1f8332b39ca80ad3e61d124147af984ffcba1dd5579dbcf11e921ea3cecb";

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
}

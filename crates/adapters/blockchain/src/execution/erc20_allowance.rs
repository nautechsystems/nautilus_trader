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

use std::{
    collections::HashSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::primitives::{Address, U256};
use anyhow::{Context, anyhow, bail};
use async_trait::async_trait;
use nautilus_model::defi::TransactionReceipt;

use crate::{
    contracts::erc20::Erc20Contract,
    execution::signer::{
        RemoteSignerClient, SignRequest, SignedTx, assert_rpc_tx_hash_matches_computed,
    },
    rpc::http::BlockchainHttpRpcClient,
};

const APPROVE_SELECTOR: &str = "0x095ea7b3";
const ERROR_CODE_APPROVE_FAILED: &str = "APPROVE_FAILED";
const ERROR_CODE_ALLOWANCE_NOT_UPDATED: &str = "ALLOWANCE_NOT_UPDATED";
const SIGNER_NOTIONAL_MAX_DECIMAL: &str = "79228162514264337593543950335";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    Exact,
    Unlimited,
    UnlimitedResetFirst,
}

#[derive(Debug, Clone)]
pub struct Erc20AllowanceConfig {
    pub router: Address,
    pub policy: ApprovalPolicy,
    pub unlimited_allowlist: HashSet<Address>,
    pub unlimited_approval_max_amount: Option<U256>,
    pub chain_id: u64,
    pub max_fee_per_gas: u64,
    pub max_priority_fee_per_gas: u64,
    pub receipt_max_polls: u32,
    pub receipt_poll_interval: Duration,
    pub deadline_ttl_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AllowanceFlowResult {
    pub skipped: bool,
    pub approval_tx_hashes: Vec<String>,
    pub final_allowance: U256,
}

#[async_trait(?Send)]
pub trait AllowanceTxSigner {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx>;
}

#[async_trait(?Send)]
impl AllowanceTxSigner for RemoteSignerClient {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx> {
        <RemoteSignerClient>::sign_evm_tx(self, request)
            .await
            .map_err(anyhow::Error::from)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn ensure_allowance<S: AllowanceTxSigner>(
    rpc_client: &BlockchainHttpRpcClient,
    erc20_contract: &Erc20Contract,
    signer: &S,
    owner: Address,
    token: Address,
    spender: Address,
    required_amount: U256,
    config: &Erc20AllowanceConfig,
) -> anyhow::Result<AllowanceFlowResult> {
    if spender != config.router {
        bail!(
            "allowance flow fail-closed: spender {} does not match configured router {}",
            spender,
            config.router
        );
    }
    if config.receipt_max_polls == 0 {
        bail!("allowance flow config invalid: receipt_max_polls must be > 0");
    }

    let current_allowance = erc20_contract
        .allowance(&token, &owner, &spender)
        .await
        .with_context(|| {
            format!(
                "failed to read allowance owner={} token={} spender={}",
                owner, token, spender
            )
        })?;

    if current_allowance >= required_amount {
        return Ok(AllowanceFlowResult {
            skipped: true,
            approval_tx_hashes: Vec::new(),
            final_allowance: current_allowance,
        });
    }

    let approval_amounts =
        determine_approval_amounts(current_allowance, required_amount, token, config)?;
    let approval_notionals = approval_amounts
        .iter()
        .map(|amount| expected_notional_for_approve_amount(*amount))
        .collect::<anyhow::Result<Vec<_>>>()
        .context("failed to derive signer-compatible expected_notional for approve flow")?;
    let mut nonce = rpc_client
        .get_transaction_count(&owner, Some("pending"))
        .await
        .with_context(|| format!("failed to fetch approve nonce for owner {}", owner))?;

    let mut approval_tx_hashes = Vec::with_capacity(approval_amounts.len());
    for (amount, expected_notional) in approval_amounts
        .into_iter()
        .zip(approval_notionals.into_iter())
    {
        let approve_data = Erc20Contract::encode_approve_call(&spender, amount);
        let approve_data_hex = format!("0x{}", hex::encode(&approve_data));

        let estimate_call = serde_json::json!({
            "from": owner,
            "to": token,
            "data": approve_data_hex,
            "value": "0x0",
        });
        let gas_estimate = rpc_client
            .estimate_gas(estimate_call, Some("latest"))
            .await
            .with_context(|| {
                format!(
                    "failed to estimate gas for approve token={} spender={} amount={}",
                    token, spender, amount
                )
            })?;
        let gas_limit = to_u64_checked(gas_estimate).with_context(|| {
            format!(
                "estimated gas does not fit in u64 for approve token={} amount={} value={}",
                token, amount, gas_estimate
            )
        })?;

        let deadline = unix_deadline_from_ttl(config.deadline_ttl_secs)
            .context("failed to compute approve transaction deadline")?;
        let sign_request = SignRequest {
            chain_id: config.chain_id,
            nonce,
            to: token,
            data: approve_data_hex,
            value: "0x00".to_string(),
            gas: gas_limit,
            max_fee_per_gas: Some(config.max_fee_per_gas),
            max_priority_fee_per_gas: Some(config.max_priority_fee_per_gas),
            gas_price: None,
            deadline,
            expected_notional,
            expected_selector: APPROVE_SELECTOR.to_string(),
        };

        let signed = signer.sign_evm_tx(sign_request).await.with_context(|| {
            format!(
                "failed to sign approve transaction token={} spender={} nonce={} amount={}",
                token, spender, nonce, amount
            )
        })?;

        let rpc_tx_hash = rpc_client
            .send_raw_transaction(&signed.raw_tx_hex)
            .await
            .with_context(|| {
                format!(
                    "failed to send approve raw transaction token={} nonce={} amount={}",
                    token, nonce, amount
                )
            })?;

        assert_rpc_tx_hash_matches_computed(&rpc_tx_hash, &signed.tx_hash).with_context(|| {
            format!(
                "approve broadcast hash mismatch token={} nonce={} computed={} rpc={}",
                token, nonce, signed.tx_hash, rpc_tx_hash
            )
        })?;

        let receipt = poll_for_receipt(
            rpc_client,
            &signed.tx_hash,
            config.receipt_max_polls,
            config.receipt_poll_interval,
        )
        .await
        .with_context(|| {
            format!(
                "failed while polling approve receipt tx_hash={}",
                signed.tx_hash
            )
        })?;

        if receipt.status != 1 {
            bail!(
                "[{}] approve transaction failed with status={} tx_hash={} token={} spender={}",
                ERROR_CODE_APPROVE_FAILED,
                receipt.status,
                signed.tx_hash,
                token,
                spender
            );
        }

        approval_tx_hashes.push(signed.tx_hash);
        nonce = nonce.saturating_add(1);
    }

    // Invariant: mined approve path must be validated with a post-transaction allowance read.
    let final_allowance = erc20_contract
        .allowance(&token, &owner, &spender)
        .await
        .with_context(|| {
            format!(
                "failed to re-check allowance after approve owner={} token={} spender={}",
                owner, token, spender
            )
        })?;

    if final_allowance < required_amount {
        bail!(
            "[{}] post-approve allowance check failed owner={} token={} spender={} required={} final={}",
            ERROR_CODE_ALLOWANCE_NOT_UPDATED,
            owner,
            token,
            spender,
            required_amount,
            final_allowance
        );
    }

    Ok(AllowanceFlowResult {
        skipped: false,
        approval_tx_hashes,
        final_allowance,
    })
}

fn determine_approval_amounts(
    current_allowance: U256,
    required_amount: U256,
    token: Address,
    config: &Erc20AllowanceConfig,
) -> anyhow::Result<Vec<U256>> {
    match config.policy {
        // `approve(amount)` sets absolute allowance, so Exact must set target to required amount.
        ApprovalPolicy::Exact => Ok(vec![required_amount]),
        ApprovalPolicy::Unlimited => {
            ensure_unlimited_allowlisted(token, config)?;
            let target = unlimited_target_amount(config);
            ensure_unlimited_target_sufficient(target, required_amount)?;
            Ok(vec![target])
        }
        ApprovalPolicy::UnlimitedResetFirst => {
            ensure_unlimited_allowlisted(token, config)?;
            let target = unlimited_target_amount(config);
            ensure_unlimited_target_sufficient(target, required_amount)?;
            let mut amounts = Vec::with_capacity(2);
            if current_allowance > U256::ZERO {
                amounts.push(U256::ZERO);
            }
            amounts.push(target);
            Ok(amounts)
        }
    }
}

fn ensure_unlimited_allowlisted(
    token: Address,
    config: &Erc20AllowanceConfig,
) -> anyhow::Result<()> {
    if !config.unlimited_allowlist.contains(&token) {
        bail!(
            "unlimited approval policy rejected: token {} is not in unlimited_allowlist",
            token
        );
    }
    Ok(())
}

fn unlimited_target_amount(config: &Erc20AllowanceConfig) -> U256 {
    config.unlimited_approval_max_amount.unwrap_or(U256::MAX)
}

fn ensure_unlimited_target_sufficient(target: U256, required: U256) -> anyhow::Result<()> {
    if target < required {
        bail!(
            "unlimited approval target is below required amount: target={} required={}",
            target,
            required
        );
    }
    Ok(())
}

fn expected_notional_for_approve_amount(amount: U256) -> anyhow::Result<String> {
    if amount == U256::ZERO {
        return Ok("1".to_string());
    }

    let amount_decimal = amount.to_string();
    let cap = SIGNER_NOTIONAL_MAX_DECIMAL;
    if amount_decimal.len() > cap.len()
        || (amount_decimal.len() == cap.len() && amount_decimal.as_str() > cap)
    {
        bail!(
            "approve amount {} cannot be represented for signer expected_notional; configure unlimited_approval_max_amount <= {}",
            amount,
            SIGNER_NOTIONAL_MAX_DECIMAL
        );
    }

    Ok(amount_decimal)
}

fn unix_deadline_from_ttl(ttl_secs: u64) -> anyhow::Result<i64> {
    if ttl_secs == 0 {
        bail!("deadline_ttl_secs must be greater than zero");
    } else {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow!("system clock before unix epoch: {e}"))?
            .as_secs();
        let deadline = now.checked_add(ttl_secs).ok_or_else(|| {
            anyhow!(
                "deadline overflow when adding ttl={} to now={}",
                ttl_secs,
                now
            )
        })?;

        i64::try_from(deadline).map_err(|_| anyhow!("deadline does not fit in i64: {}", deadline))
    }
}

fn to_u64_checked(value: U256) -> anyhow::Result<u64> {
    u64::try_from(value).map_err(|_| anyhow!("value does not fit into u64: {}", value))
}

async fn poll_for_receipt(
    rpc_client: &BlockchainHttpRpcClient,
    tx_hash: &str,
    max_polls: u32,
    interval: Duration,
) -> anyhow::Result<TransactionReceipt> {
    for attempt in 0..max_polls {
        let receipt = rpc_client
            .get_transaction_receipt(tx_hash)
            .await
            .with_context(|| {
                format!(
                    "getTransactionReceipt failed for tx_hash={} attempt={}/{}",
                    tx_hash,
                    attempt + 1,
                    max_polls
                )
            })?;

        if let Some(receipt) = receipt {
            return Ok(receipt);
        }

        if attempt + 1 < max_polls {
            tokio::time::sleep(interval).await;
        }
    }

    bail!(
        "receipt polling exhausted for tx_hash={} after {} polls",
        tx_hash,
        max_polls
    );
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U256;

    use super::{SIGNER_NOTIONAL_MAX_DECIMAL, expected_notional_for_approve_amount};

    #[test]
    fn test_expected_notional_for_very_large_amount_fails_closed() {
        let amount = U256::MAX;
        let error = expected_notional_for_approve_amount(amount).expect_err("must fail closed");
        assert!(
            error
                .to_string()
                .contains("cannot be represented for signer expected_notional")
        );
    }

    #[test]
    fn test_expected_notional_zero_amount_maps_to_positive_decimal() {
        assert_eq!(
            expected_notional_for_approve_amount(U256::ZERO).unwrap(),
            "1"
        );
    }

    #[test]
    fn test_expected_notional_max_supported_value_is_accepted() {
        let amount = U256::from_str_radix(SIGNER_NOTIONAL_MAX_DECIMAL, 10).unwrap();
        assert_eq!(
            expected_notional_for_approve_amount(amount).unwrap(),
            SIGNER_NOTIONAL_MAX_DECIMAL
        );
    }
}

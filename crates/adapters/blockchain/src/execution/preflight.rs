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

//! Structured preflight readiness report for execution operations.

use alloy::primitives::{Address, U256};
use nautilus_model::identifiers::InstrumentId;

/// Deployed-bytecode check for a single contract address.
#[derive(Debug, Clone)]
pub struct ContractCodeCheck {
    /// The checked address.
    pub address: Address,
    /// Whether non-empty bytecode is deployed at the address.
    pub has_deployed_code: bool,
}

/// Preflight check for the selected pool.
#[derive(Debug, Clone)]
pub struct PoolPreflightCheck {
    /// The instrument ID the pool was selected by.
    pub instrument_id: InstrumentId,
    /// The pool address.
    pub address: Address,
    /// Whether non-empty bytecode is deployed at the pool address.
    pub has_deployed_code: bool,
    /// The pool fee tier (hundredths of a bip), required for swap calldata.
    pub fee: Option<u32>,
    /// The resolved base (input) token address.
    pub base_token: Address,
    /// The resolved quote (output) token address.
    pub quote_token: Address,
}

/// Preflight check for a pool token, including wallet balance and router allowances.
#[derive(Debug, Clone)]
pub struct TokenPreflightCheck {
    /// The token address.
    pub address: Address,
    /// The token symbol.
    pub symbol: String,
    /// Whether non-empty bytecode is deployed at the token address.
    pub has_deployed_code: bool,
    /// The wallet's raw token balance.
    pub wallet_balance: U256,
    /// Exact allowances the wallet has granted each allowlisted router.
    pub router_allowances: Vec<(Address, U256)>,
}

/// Structured, sanitized result of an execution preflight.
///
/// Contains no RPC URLs, keys, or raw signed transactions.
#[derive(Debug, Clone)]
pub struct BlockchainPreflightReport {
    /// The configured expected chain ID.
    pub expected_chain_id: u64,
    /// The chain ID reported by the RPC node.
    pub actual_chain_id: u64,
    /// Whether the reported chain ID matches the configuration.
    pub chain_id_matches: bool,
    /// The pool check.
    pub pool: PoolPreflightCheck,
    /// Deployed-bytecode checks for each allowlisted router.
    pub routers: Vec<ContractCodeCheck>,
    /// Checks for the pool's base and quote tokens.
    pub tokens: Vec<TokenPreflightCheck>,
    /// The wallet's raw native currency balance in wei.
    pub native_balance_wei: U256,
    /// The latest base fee per gas in wei.
    pub base_fee_per_gas_wei: u128,
    /// The node's suggested max priority fee per gas in wei.
    pub max_priority_fee_per_gas_wei: u128,
    /// The derived max fee per gas in wei after the configured base fee buffer.
    pub derived_max_fee_per_gas_wei: u128,
    /// Whether the derived max fee is within the configured ceiling.
    pub fee_within_ceiling: bool,
    /// Whether every check passed and the wallet is ready for execution operations.
    pub ready: bool,
    /// Human-readable descriptions of every failed check.
    pub issues: Vec<String>,
}

impl BlockchainPreflightReport {
    /// Builds a report from preflight check inputs, deriving `ready` and `issues`.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        expected_chain_id: u64,
        actual_chain_id: u64,
        pool: PoolPreflightCheck,
        routers: Vec<ContractCodeCheck>,
        tokens: Vec<TokenPreflightCheck>,
        native_balance_wei: U256,
        base_fee_per_gas_wei: u128,
        max_priority_fee_per_gas_wei: u128,
        derived_max_fee_per_gas_wei: u128,
        max_fee_per_gas_wei: u128,
    ) -> Self {
        let chain_id_matches = expected_chain_id == actual_chain_id;
        let fee_within_ceiling = derived_max_fee_per_gas_wei <= max_fee_per_gas_wei;

        let mut issues = Vec::new();

        if !chain_id_matches {
            issues.push(format!(
                "Chain ID mismatch: expected {expected_chain_id}, node reported {actual_chain_id}"
            ));
        }

        if !pool.has_deployed_code {
            issues.push(format!(
                "No deployed bytecode at pool address {}",
                pool.address
            ));
        }

        if pool.fee.is_none() {
            issues.push("Pool fee tier is missing".to_string());
        }

        for router in &routers {
            if !router.has_deployed_code {
                issues.push(format!(
                    "No deployed bytecode at router address {}",
                    router.address
                ));
            }
        }

        for token in &tokens {
            if !token.has_deployed_code {
                issues.push(format!(
                    "No deployed bytecode at {} token address {}",
                    token.symbol, token.address
                ));
            }
        }

        if native_balance_wei.is_zero() {
            issues.push("Native currency balance is zero; cannot pay gas".to_string());
        }

        let base_token_check = tokens.iter().find(|t| t.address == pool.base_token);
        let input_balance_ok = base_token_check.is_some_and(|t| !t.wallet_balance.is_zero());

        if !input_balance_ok {
            issues.push(format!("Input token {} balance is zero", pool.base_token));
        }

        let execution_router = routers.first().map(|router| router.address);
        let allowance_ok = base_token_check.is_some_and(|token| {
            token
                .router_allowances
                .iter()
                .any(|(router, amount)| Some(*router) == execution_router && !amount.is_zero())
        });

        if !allowance_ok {
            issues.push(format!(
                "No router allowance for input token {}",
                pool.base_token
            ));
        }

        if !fee_within_ceiling {
            issues.push(format!(
                "Derived max fee per gas {derived_max_fee_per_gas_wei} wei exceeds ceiling {max_fee_per_gas_wei} wei"
            ));
        }

        let ready = issues.is_empty();

        Self {
            expected_chain_id,
            actual_chain_id,
            chain_id_matches,
            pool,
            routers,
            tokens,
            native_balance_wei,
            base_fee_per_gas_wei,
            max_priority_fee_per_gas_wei,
            derived_max_fee_per_gas_wei,
            fee_within_ceiling,
            ready,
            issues,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{U256, address};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn report_requires_allowance_on_execution_router() {
        let base_token = address!("1111111111111111111111111111111111111111");
        let quote_token = address!("2222222222222222222222222222222222222222");
        let execution_router = address!("3333333333333333333333333333333333333333");
        let alternate_router = address!("4444444444444444444444444444444444444444");
        let report = BlockchainPreflightReport::new(
            42161,
            42161,
            PoolPreflightCheck {
                instrument_id: "0x5555555555555555555555555555555555555555.Arbitrum:UniswapV3"
                    .parse()
                    .unwrap(),
                address: address!("5555555555555555555555555555555555555555"),
                has_deployed_code: true,
                fee: Some(500),
                base_token,
                quote_token,
            },
            vec![
                ContractCodeCheck {
                    address: execution_router,
                    has_deployed_code: true,
                },
                ContractCodeCheck {
                    address: alternate_router,
                    has_deployed_code: true,
                },
            ],
            vec![
                TokenPreflightCheck {
                    address: base_token,
                    symbol: "BASE".to_string(),
                    has_deployed_code: true,
                    wallet_balance: U256::from(1),
                    router_allowances: vec![
                        (execution_router, U256::ZERO),
                        (alternate_router, U256::from(1)),
                    ],
                },
                TokenPreflightCheck {
                    address: quote_token,
                    symbol: "QUOTE".to_string(),
                    has_deployed_code: true,
                    wallet_balance: U256::ZERO,
                    router_allowances: Vec::new(),
                },
            ],
            U256::from(1),
            100_000_000,
            10_000_000,
            130_000_000,
            1_000_000_000,
        );

        assert!(!report.ready);
        assert_eq!(
            report.issues,
            vec![format!("No router allowance for input token {base_token}")]
        );
    }
}

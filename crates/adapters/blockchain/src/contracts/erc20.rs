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

use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
    sol_types::SolCall,
};
use strum::Display;
use thiserror::Error;

use super::base::{BaseContract, ContractCall, Multicall3};
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract ERC20 {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
    }
}

#[derive(Debug, Display)]
pub enum Erc20Field {
    Name,
    Symbol,
    Decimals,
}

/// Represents the essential metadata information for an ERC20 token.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// The full name of the token.
    pub name: String,
    /// The ticker symbol of the token.
    pub symbol: String,
    /// The number of decimal places the token uses for representing fractional amounts.
    pub decimals: u8,
}

/// Represents errors that can occur when interacting with a blockchain RPC client.
#[derive(Debug, Error)]
pub enum TokenInfoError {
    #[error("RPC error: {0}")]
    RpcError(#[from] BlockchainRpcClientError),
    #[error("Token {field} is empty for address {address}")]
    EmptyTokenField { field: Erc20Field, address: Address },
    #[error("Multicall returned unexpected number of results: expected {expected}, was {actual}")]
    UnexpectedResultCount { expected: usize, actual: usize },
    #[error("Call failed for {field} at address {address}: {reason} (raw data: {raw_data})")]
    CallFailed {
        field: String,
        address: Address,
        reason: String,
        raw_data: String,
    },
    #[error("Failed to decode {field} for address {address}: {reason} (raw data: {raw_data})")]
    DecodingError {
        field: String,
        address: Address,
        reason: String,
        raw_data: String,
    },
}

/// Interface for interacting with ERC20 token contracts on a blockchain.
///
/// This struct provides methods to fetch token metadata (name, symbol, decimals).
/// From ERC20-compliant tokens on any EVM-compatible blockchain.
#[derive(Debug)]
pub struct Erc20Contract {
    /// The base contract providing common RPC execution functionality.
    base: BaseContract,
    /// Whether to enforce that token name and symbol fields must be non-empty.
    enforce_token_fields: bool,
}

impl Erc20Contract {
    /// Creates a new ERC20 contract interface with the specified RPC client.
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>, enforce_token_fields: bool) -> Self {
        Self {
            base: BaseContract::new(client),
            enforce_token_fields,
        }
    }

    /// Creates a new ERC20 contract interface with the specified RPC client and a per-request
    /// RPC timeout applied to its reads.
    #[must_use]
    pub fn new_with_timeout(
        client: Arc<BlockchainHttpRpcClient>,
        rpc_timeout_secs: Option<u64>,
        enforce_token_fields: bool,
    ) -> Self {
        Self {
            base: BaseContract::new_with_multicall_limit_and_timeout(
                client,
                super::base::DEFAULT_MULTICALL_CALLS_PER_RPC_REQUEST,
                rpc_timeout_secs,
            ),
            enforce_token_fields,
        }
    }

    /// Fetches complete token information (name, symbol, decimals) from an ERC20 contract.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the contract calls fail.
    /// - [`BlockchainRpcClientError::ClientError`] if an RPC call fails.
    /// - [`BlockchainRpcClientError::AbiDecodingError`] if ABI decoding fails.
    pub async fn fetch_token_info(
        &self,
        token_address: &Address,
    ) -> Result<TokenInfo, TokenInfoError> {
        let calls = vec![
            ContractCall {
                target: *token_address,
                allow_failure: true,
                call_data: ERC20::nameCall.abi_encode(),
            },
            ContractCall {
                target: *token_address,
                allow_failure: true,
                call_data: ERC20::symbolCall.abi_encode(),
            },
            ContractCall {
                target: *token_address,
                allow_failure: true,
                call_data: ERC20::decimalsCall.abi_encode(),
            },
        ];

        let results = self.base.execute_multicall(calls, None).await?;

        if results.len() != 3 {
            return Err(TokenInfoError::UnexpectedResultCount {
                expected: 3,
                actual: results.len(),
            });
        }

        let name = parse_erc20_string_result(&results[0], Erc20Field::Name, token_address)?;
        let symbol = parse_erc20_string_result(&results[1], Erc20Field::Symbol, token_address)?;
        let decimals = parse_erc20_decimals_result(&results[2], token_address)?;

        if self.enforce_token_fields && name.is_empty() {
            return Err(TokenInfoError::EmptyTokenField {
                field: Erc20Field::Name,
                address: *token_address,
            });
        }

        if self.enforce_token_fields && symbol.is_empty() {
            return Err(TokenInfoError::EmptyTokenField {
                field: Erc20Field::Symbol,
                address: *token_address,
            });
        }

        Ok(TokenInfo {
            name,
            symbol,
            decimals,
        })
    }

    /// Fetches token information for multiple tokens in a single multicall.
    ///
    /// If the multicall fails (typically due to expired/broken contracts causing RPC "out of gas"),
    /// automatically falls back to individual token fetches to isolate problematic contracts.
    ///
    /// # Errors
    ///
    /// Returns an error only if the operation cannot proceed. Multicall failures trigger
    /// automatic fallback to individual fetches. Individual token failures are captured
    /// in the Result values of the returned `HashMap`.
    pub async fn batch_fetch_token_info(
        &self,
        token_addresses: &[Address],
    ) -> Result<HashMap<Address, Result<TokenInfo, TokenInfoError>>, BlockchainRpcClientError> {
        // Build calls for all tokens (3 calls per token)
        let mut calls = Vec::with_capacity(token_addresses.len() * 3);

        for token_address in token_addresses {
            calls.extend([
                ContractCall {
                    target: *token_address,
                    allow_failure: true, // Allow individual token failures
                    call_data: ERC20::nameCall.abi_encode(),
                },
                ContractCall {
                    target: *token_address,
                    allow_failure: true,
                    call_data: ERC20::symbolCall.abi_encode(),
                },
                ContractCall {
                    target: *token_address,
                    allow_failure: true,
                    call_data: ERC20::decimalsCall.abi_encode(),
                },
            ]);
        }

        // Try batch multicall first
        let results = match self.base.execute_multicall(calls, None).await {
            Ok(results) => results,
            Err(e) => {
                // Multicall failed (likely expired/broken contract causing RPC failure)
                log::warn!(
                    "Batch multicall failed: {}. Falling back to individual fetches for {} tokens",
                    e,
                    token_addresses.len()
                );

                // Fallback: fetch each token individually to isolate problematic contracts
                let mut token_infos = HashMap::with_capacity(token_addresses.len());
                for token_address in token_addresses {
                    match self.fetch_token_info(token_address).await {
                        Ok(info) => {
                            token_infos.insert(*token_address, Ok(info));
                        }
                        Err(e) => {
                            log::debug!(
                                "Token {token_address} failed individual fetch (likely expired/broken): {e}"
                            );
                            token_infos.insert(*token_address, Err(e));
                        }
                    }
                }

                return Ok(token_infos);
            }
        };

        let mut token_infos = HashMap::with_capacity(token_addresses.len());
        for (i, token_address) in token_addresses.iter().enumerate() {
            let base_idx = i * 3;

            // Check if we have all 3 results for this token.
            if base_idx + 2 >= results.len() {
                log::error!("Incomplete results from multicall for token {token_address}");
                token_infos.insert(
                    *token_address,
                    Err(TokenInfoError::UnexpectedResultCount {
                        expected: 3,
                        actual: results.len().saturating_sub(base_idx),
                    }),
                );
                continue;
            }

            let token_info =
                parse_batch_token_results(&results[base_idx..base_idx + 3], token_address);
            token_infos.insert(*token_address, token_info);
        }

        Ok(token_infos)
    }

    /// Fetches the balance of a specific account for this ERC20 token.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails.
    /// - [`BlockchainRpcClientError::ClientError`] if an RPC call fails.
    /// - [`BlockchainRpcClientError::AbiDecodingError`] if ABI decoding fails.
    pub async fn balance_of(
        &self,
        token_address: &Address,
        account: &Address,
    ) -> Result<U256, BlockchainRpcClientError> {
        self.balance_of_with_block(token_address, account, None)
            .await
    }

    #[cfg(feature = "hypersync")]
    pub(crate) async fn balance_of_at(
        &self,
        token_address: &Address,
        account: &Address,
        block: u64,
    ) -> Result<U256, BlockchainRpcClientError> {
        self.balance_of_with_block(token_address, account, Some(block))
            .await
    }

    async fn balance_of_with_block(
        &self,
        token_address: &Address,
        account: &Address,
        block: Option<u64>,
    ) -> Result<U256, BlockchainRpcClientError> {
        let call_data = ERC20::balanceOfCall { account: *account }.abi_encode();
        let result = self
            .base
            .execute_call(token_address, &call_data, block)
            .await?;

        ERC20::balanceOfCall::abi_decode_returns(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
    }

    /// Fetches the exact allowance an owner has granted a spender for this ERC20 token.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails.
    /// - [`BlockchainRpcClientError::ClientError`] if an RPC call fails.
    /// - [`BlockchainRpcClientError::AbiDecodingError`] if ABI decoding fails.
    pub async fn allowance(
        &self,
        token_address: &Address,
        owner: &Address,
        spender: &Address,
    ) -> Result<U256, BlockchainRpcClientError> {
        self.allowance_with_block(token_address, owner, spender, None)
            .await
    }

    #[cfg(feature = "hypersync")]
    pub(crate) async fn allowance_at(
        &self,
        token_address: &Address,
        owner: &Address,
        spender: &Address,
        block: u64,
    ) -> Result<U256, BlockchainRpcClientError> {
        self.allowance_with_block(token_address, owner, spender, Some(block))
            .await
    }

    async fn allowance_with_block(
        &self,
        token_address: &Address,
        owner: &Address,
        spender: &Address,
        block: Option<u64>,
    ) -> Result<U256, BlockchainRpcClientError> {
        let call_data = ERC20::allowanceCall {
            owner: *owner,
            spender: *spender,
        }
        .abi_encode();
        let result = self
            .base
            .execute_call(token_address, &call_data, block)
            .await?;

        ERC20::allowanceCall::abi_decode_returns(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
    }

    #[cfg(feature = "hypersync")]
    pub(crate) async fn simulate_approve(
        &self,
        token_address: &Address,
        owner: &Address,
        spender: &Address,
        amount: U256,
    ) -> Result<bool, BlockchainRpcClientError> {
        let call_data = ERC20::approveCall {
            spender: *spender,
            amount,
        }
        .abi_encode();
        let result = self
            .base
            .execute_call_from(owner, token_address, &call_data, None)
            .await?;

        // Empty return data is the supported legacy ERC-20 success convention
        if result.is_empty() {
            return Ok(true);
        }

        ERC20::approveCall::abi_decode_returns_validate(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
    }
}

/// Attempts to decode a revert reason from failed call data.
/// Returns a human-readable error message.
fn decode_revert_reason(data: &Bytes) -> String {
    // For now, just return a simple description
    // Could be enhanced to decode actual revert reasons in the future
    if data.is_empty() {
        "Call failed without revert data".to_string()
    } else {
        format!("Call failed with data: {data}")
    }
}

/// Generic parser for ERC20 string results (name, symbol)
fn parse_erc20_string_result(
    result: &Multicall3::Result,
    field_name: Erc20Field,
    token_address: &Address,
) -> Result<String, TokenInfoError> {
    // Common validation
    if !result.success {
        let reason = if result.returnData.is_empty() {
            "Call failed without revert data".to_string()
        } else {
            // Try to decode revert reason if present
            decode_revert_reason(&result.returnData)
        };

        return Err(TokenInfoError::CallFailed {
            field: field_name.to_string(),
            address: *token_address,
            reason,
            raw_data: result.returnData.to_string(),
        });
    }

    if result.returnData.is_empty() {
        return Err(TokenInfoError::EmptyTokenField {
            field: field_name,
            address: *token_address,
        });
    }

    match field_name {
        Erc20Field::Name => ERC20::nameCall::abi_decode_returns(&result.returnData),
        Erc20Field::Symbol => ERC20::symbolCall::abi_decode_returns(&result.returnData),
        Erc20Field::Decimals => {
            return Err(TokenInfoError::DecodingError {
                field: field_name.to_string(),
                address: *token_address,
                reason: "Expected Name or Symbol for parse_erc20_string_result function argument"
                    .to_string(),
                raw_data: result.returnData.to_string(),
            });
        }
    }
    .map_err(|e| TokenInfoError::DecodingError {
        field: field_name.to_string(),
        address: *token_address,
        reason: e.to_string(),
        raw_data: result.returnData.to_string(),
    })
}

/// Generic parser for ERC20 decimals result
fn parse_erc20_decimals_result(
    result: &Multicall3::Result,
    token_address: &Address,
) -> Result<u8, TokenInfoError> {
    // Common validation
    if !result.success {
        let reason = if result.returnData.is_empty() {
            "Call failed without revert data".to_string()
        } else {
            decode_revert_reason(&result.returnData)
        };

        return Err(TokenInfoError::CallFailed {
            field: "decimals".to_string(),
            address: *token_address,
            reason,
            raw_data: result.returnData.to_string(),
        });
    }

    if result.returnData.is_empty() {
        return Err(TokenInfoError::EmptyTokenField {
            field: Erc20Field::Decimals,
            address: *token_address,
        });
    }

    ERC20::decimalsCall::abi_decode_returns(&result.returnData).map_err(|e| {
        TokenInfoError::DecodingError {
            field: "decimals".to_string(),
            address: *token_address,
            reason: e.to_string(),
            raw_data: result.returnData.to_string(),
        }
    })
}

/// Parses token information from a slice of 3 multicall results.
///
/// Expects results in order: name, symbol, decimals.
/// Returns Ok(TokenInfo) if all three calls succeeded, or an Err with a
/// descriptive error message if any call failed.
fn parse_batch_token_results(
    results: &[Multicall3::Result],
    token_address: &Address,
) -> Result<TokenInfo, TokenInfoError> {
    if results.len() != 3 {
        return Err(TokenInfoError::UnexpectedResultCount {
            expected: 3,
            actual: results.len(),
        });
    }

    let name = parse_erc20_string_result(&results[0], Erc20Field::Name, token_address)?;
    let symbol = parse_erc20_string_result(&results[1], Erc20Field::Symbol, token_address)?;
    let decimals = parse_erc20_decimals_result(&results[2], token_address)?;

    Ok(TokenInfo {
        name,
        symbol,
        decimals,
    })
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Bytes, address};
    use nautilus_core::hex;
    use rstest::{fixture, rstest};

    use super::*;
    use crate::rpc::http::tests::mock::{MockRpcState, start_mock_rpc_server};

    const CALL_BALANCE: &str = include_str!("../../test_data/execution/rpc_eth_call_balance.json");
    const CALL_ALLOWANCE: &str =
        include_str!("../../test_data/execution/rpc_eth_call_allowance.json");
    #[cfg(feature = "hypersync")]
    const CALL_MAX: &str = include_str!("../../test_data/execution/rpc_eth_call_max.json");

    #[fixture]
    fn token_address() -> Address {
        address!("25b76A90E389bD644a29db919b136Dc63B174Ec7")
    }

    #[fixture]
    fn successful_name_result() -> Multicall3::Result {
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(hex::decode("00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000007546f6b656e204100000000000000000000000000000000000000000000000000").unwrap()),
        }
    }

    #[fixture]
    fn successful_symbol_result() -> Multicall3::Result {
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(hex::decode("0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000776546f6b656e4100000000000000000000000000000000000000000000000000").unwrap()),
        }
    }

    #[fixture]
    fn failed_name_result() -> Multicall3::Result {
        Multicall3::Result {
            success: false,
            returnData: Bytes::from(vec![]),
        }
    }

    #[fixture]
    fn failed_token_address() -> Address {
        address!("00000000049084A92F8964B76845ab6DE54EB229")
    }

    #[fixture]
    fn success_but_empty_result() -> Multicall3::Result {
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(vec![]),
        }
    }

    #[fixture]
    fn empty_token_address() -> Address {
        address!("a5b00cEc63694319495d605AA414203F9714F47E")
    }

    #[fixture]
    fn non_abi_encoded_string_result() -> Multicall3::Result {
        // Returns raw string bytes without ABI encoding - "Rico" as raw bytes
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(
                hex::decode("5269636f00000000000000000000000000000000000000000000000000000000")
                    .unwrap(),
            ),
        }
    }

    #[fixture]
    fn non_abi_encoded_token_address() -> Address {
        address!("5374EcC160A4bd68446B43B5A6B132F9c001C54C")
    }

    #[fixture]
    fn non_standard_selector_result() -> Multicall3::Result {
        // Returns function selector instead of actual data
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(
                hex::decode("06fdde0300000000000000000000000000000000000000000000000000000000")
                    .unwrap(),
            ),
        }
    }

    #[fixture]
    fn non_abi_encoded_long_string_result() -> Multicall3::Result {
        // Returns raw string bytes without ABI encoding - longer string example
        Multicall3::Result {
            success: true,
            returnData: Bytes::from(
                hex::decode("5269636f62616e6b205269736b20536861726500000000000000000000000000")
                    .unwrap(),
            ),
        }
    }

    #[rstest]
    fn test_parse_erc20_string_result_name_success(
        successful_name_result: Multicall3::Result,
        token_address: Address,
    ) {
        let result =
            parse_erc20_string_result(&successful_name_result, Erc20Field::Name, &token_address);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Token A");
    }

    #[rstest]
    fn test_parse_erc20_string_result_symbol_success(
        successful_symbol_result: Multicall3::Result,
        token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &successful_symbol_result,
            Erc20Field::Symbol,
            &token_address,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "vTokenA");
    }

    #[rstest]
    fn test_parse_erc20_string_result_name_failed_with_specific_address(
        failed_name_result: Multicall3::Result,
        failed_token_address: Address,
    ) {
        let result =
            parse_erc20_string_result(&failed_name_result, Erc20Field::Name, &failed_token_address);
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::CallFailed {
                field,
                address,
                reason,
                raw_data: _,
            } => {
                assert_eq!(field, "Name");
                assert_eq!(address, failed_token_address);
                assert_eq!(reason, "Call failed without revert data");
            }
            _ => panic!("Expected DecodingError"),
        }
    }

    #[rstest]
    fn test_parse_erc20_string_result_success_but_empty_name(
        success_but_empty_result: Multicall3::Result,
        empty_token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &success_but_empty_result,
            Erc20Field::Name,
            &empty_token_address,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::EmptyTokenField { field, address } => {
                assert!(matches!(field, Erc20Field::Name));
                assert_eq!(address, empty_token_address);
            }
            _ => panic!("Expected EmptyTokenField error"),
        }
    }

    #[rstest]
    fn test_parse_erc20_decimals_result_success_but_empty(
        success_but_empty_result: Multicall3::Result,
        empty_token_address: Address,
    ) {
        let result = parse_erc20_decimals_result(&success_but_empty_result, &empty_token_address);
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::EmptyTokenField { field, address } => {
                assert!(matches!(field, Erc20Field::Decimals));
                assert_eq!(address, empty_token_address);
            }
            _ => panic!("Expected EmptyTokenField error"),
        }
    }

    #[rstest]
    fn test_parse_non_abi_encoded_string(
        non_abi_encoded_string_result: Multicall3::Result,
        non_abi_encoded_token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &non_abi_encoded_string_result,
            Erc20Field::Name,
            &non_abi_encoded_token_address,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::DecodingError {
                field,
                address,
                reason,
                raw_data,
            } => {
                assert_eq!(field, "Name");
                assert_eq!(address, non_abi_encoded_token_address);
                assert!(reason.contains("type check failed"));
                assert_eq!(
                    raw_data,
                    "0x5269636f00000000000000000000000000000000000000000000000000000000"
                );
                // Raw bytes "Rico" without ABI encoding
            }
            _ => panic!("Expected DecodingError"),
        }
    }

    #[rstest]
    fn test_parse_non_standard_selector_return(
        non_standard_selector_result: Multicall3::Result,
        token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &non_standard_selector_result,
            Erc20Field::Name,
            &token_address,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::DecodingError {
                field,
                address,
                reason,
                raw_data,
            } => {
                assert_eq!(field, "Name");
                assert_eq!(address, token_address);
                assert!(reason.contains("type check failed"));
                assert_eq!(
                    raw_data,
                    "0x06fdde0300000000000000000000000000000000000000000000000000000000"
                );
            }
            _ => panic!("Expected DecodingError"),
        }
    }

    #[rstest]
    fn test_parse_non_abi_encoded_long_string(
        non_abi_encoded_long_string_result: Multicall3::Result,
        non_abi_encoded_token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &non_abi_encoded_long_string_result,
            Erc20Field::Name,
            &non_abi_encoded_token_address,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            TokenInfoError::DecodingError {
                field,
                address,
                reason,
                raw_data,
            } => {
                assert_eq!(field, "Name");
                assert_eq!(address, non_abi_encoded_token_address);
                assert!(reason.contains("type check failed"));
                assert_eq!(
                    raw_data,
                    "0x5269636f62616e6b205269736b20536861726500000000000000000000000000"
                );
                // Example of longer non-ABI encoded string
            }
            _ => panic!("Expected DecodingError"),
        }
    }

    #[rstest]
    fn test_approve_calldata_matches_known_vector() {
        let spender = address!("E592427A0AEce92De3Edee1F18E0157C05861564");
        let calldata = ERC20::approveCall {
            spender,
            amount: U256::from(1_000_000_000_000_000_000u64),
        }
        .abi_encode();

        // approve(address,uint256) selector 0x095ea7b3, spender word, amount word
        let expected = format!(
            "095ea7b3{:0>64}{:0>64}",
            "e592427a0aece92de3edee1f18e0157c05861564", "0de0b6b3a7640000"
        );
        assert_eq!(hex::encode(&calldata), expected);
    }

    #[rstest]
    fn test_allowance_calldata_matches_known_vector() {
        let owner = address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        let spender = address!("E592427A0AEce92De3Edee1F18E0157C05861564");
        let calldata = ERC20::allowanceCall { owner, spender }.abi_encode();

        // allowance(address,address) selector 0xdd62ed3e, owner word, spender word
        let expected = format!(
            "dd62ed3e{:0>64}{:0>64}",
            "f39fd6e51aad88f6f4ce6ab8827279cfffb92266", "e592427a0aece92de3edee1f18e0157c05861564"
        );
        assert_eq!(hex::encode(&calldata), expected);
    }

    #[tokio::test]
    async fn test_balance_of_against_mock_rpc() {
        let state = MockRpcState::default().with_call_response("0x70a08231", CALL_BALANCE);
        let addr = start_mock_rpc_server(state.clone()).await;
        let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            format!("http://{addr}"),
            None,
            None,
        ));
        let contract = Erc20Contract::new(rpc_client, true);

        let balance = contract
            .balance_of(
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                &address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
            )
            .await
            .unwrap();

        assert_eq!(balance, U256::from(500_000_000_000_000_000u64));
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"][1], "latest");
    }

    #[cfg(feature = "hypersync")]
    #[tokio::test]
    async fn test_balance_of_at_against_mock_rpc() {
        let state = MockRpcState::default().with_call_response("0x70a08231", CALL_BALANCE);
        let addr = start_mock_rpc_server(state.clone()).await;
        let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            format!("http://{addr}"),
            None,
            None,
        ));
        let contract = Erc20Contract::new(rpc_client, true);

        let balance = contract
            .balance_of_at(
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                &address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                30_346_561,
            )
            .await
            .unwrap();

        assert_eq!(balance, U256::from(500_000_000_000_000_000u64));
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"][1], "0x1cf0d41");
    }

    #[tokio::test]
    async fn test_allowance_against_mock_rpc() {
        let state = MockRpcState::default().with_call_response("0xdd62ed3e", CALL_ALLOWANCE);
        let addr = start_mock_rpc_server(state.clone()).await;
        let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            format!("http://{addr}"),
            None,
            None,
        ));
        let contract = Erc20Contract::new(rpc_client, true);

        let allowance = contract
            .allowance(
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                &address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                &address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap();

        assert_eq!(allowance, U256::from(1_000_000_000_000_000_000u64));
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"][1], "latest");
    }

    #[cfg(feature = "hypersync")]
    #[tokio::test]
    async fn test_allowance_at_against_mock_rpc() {
        let state = MockRpcState::default().with_call_response("0xdd62ed3e", CALL_ALLOWANCE);
        let addr = start_mock_rpc_server(state.clone()).await;
        let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            format!("http://{addr}"),
            None,
            None,
        ));
        let contract = Erc20Contract::new(rpc_client, true);

        let allowance = contract
            .allowance_at(
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                &address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                &address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
                30_346_561,
            )
            .await
            .unwrap();

        assert_eq!(allowance, U256::from(1_000_000_000_000_000_000u64));
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"][1], "0x1cf0d41");
    }

    #[cfg(feature = "hypersync")]
    #[tokio::test]
    async fn test_simulate_approve_rejects_malformed_bool() {
        let state = MockRpcState::default().with_response("eth_call", CALL_MAX);
        let addr = start_mock_rpc_server(state.clone()).await;
        let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            format!("http://{addr}"),
            None,
            None,
        ));
        let contract = Erc20Contract::new(rpc_client, true);
        let owner = address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266");

        let error = contract
            .simulate_approve(
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                &owner,
                &address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
                U256::MAX,
            )
            .await
            .unwrap_err();

        assert!(
            matches!(error, BlockchainRpcClientError::AbiDecodingError(_)),
            "was: {error}"
        );
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"][0]["from"], owner.to_string());
        assert_eq!(requests[0]["params"][1], "latest");
    }
}

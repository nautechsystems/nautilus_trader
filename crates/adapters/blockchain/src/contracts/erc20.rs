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
                tracing::warn!(
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
                            tracing::debug!(
                                "Token {} failed individual fetch (likely expired/broken): {}",
                                token_address,
                                e
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
                tracing::error!("Incomplete results from multicall for token {token_address}");
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
        let call_data = ERC20::balanceOfCall { account: *account }.abi_encode();
        let result = self
            .base
            .execute_call(token_address, &call_data, None)
            .await?;

        ERC20::balanceOfCall::abi_decode_returns(&result)
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
        _ => panic!("Expected Name or Symbol for for parse_erc20_string_result function argument"),
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
    use rstest::{fixture, rstest};

    use super::*;

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
        token_address: Address,
    ) {
        let result = parse_erc20_string_result(
            &non_abi_encoded_long_string_result,
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
                    "0x5269636f62616e6b205269736b20536861726500000000000000000000000000"
                );
                // Example of longer non-ABI encoded string
            }
            _ => panic!("Expected DecodingError"),
        }
    }
}

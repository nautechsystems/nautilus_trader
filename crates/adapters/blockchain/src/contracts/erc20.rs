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

use alloy::{primitives::Address, sol, sol_types::SolCall};

use super::base::{BaseContract, ContractCall, Multicall3};
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract ERC20 {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
    }
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

/// Interface for interacting with ERC20 token contracts on a blockchain.
///
/// This struct provides methods to fetch token metadata (name, symbol, decimals).
/// From ERC20-compliant tokens on any EVM-compatible blockchain.
#[derive(Debug)]
pub struct Erc20Contract {
    /// The base contract providing common RPC execution functionality.
    base: BaseContract,
}

impl Erc20Contract {
    /// Creates a new ERC20 contract interface with the specified RPC client.
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>) -> Self {
        Self {
            base: BaseContract::new(client),
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
    ) -> Result<TokenInfo, BlockchainRpcClientError> {
        let calls = vec![
            ContractCall {
                target: *token_address,
                allow_failure: false,
                call_data: ERC20::nameCall.abi_encode(),
            },
            ContractCall {
                target: *token_address,
                allow_failure: false,
                call_data: ERC20::symbolCall.abi_encode(),
            },
            ContractCall {
                target: *token_address,
                allow_failure: false,
                call_data: ERC20::decimalsCall.abi_encode(),
            },
        ];

        let results = self.base.execute_multicall(calls).await?;

        if results.len() != 3 {
            return Err(BlockchainRpcClientError::AbiDecodingError(
                "Unexpected number of results from multicall".to_string(),
            ));
        }

        let name = parse_erc20_string_result(&results[0], "name")?;
        let symbol = parse_erc20_string_result(&results[1], "symbol")?;
        let decimals = parse_erc20_decimals_result(&results[2])?;

        Ok(TokenInfo {
            name,
            symbol,
            decimals,
        })
    }

    /// Fetches token information for multiple tokens in a single multicall.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall itself fails. Individual token failures
    /// are captured in the Result values of the returned HashMap.
    pub async fn batch_fetch_token_info(
        &self,
        token_addresses: &[Address],
    ) -> Result<HashMap<Address, TokenInfo>, BlockchainRpcClientError> {
        if token_addresses.is_empty() {
            return Ok(HashMap::new());
        }

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

        let results = self.base.execute_multicall(calls).await?;

        let mut token_infos = HashMap::with_capacity(token_addresses.len());
        for (i, token_address) in token_addresses.iter().enumerate() {
            let base_idx = i * 3;

            // Check if we have all 3 results for this token.
            if base_idx + 2 >= results.len() {
                tracing::error!("Incomplete results from multicall for token {token_address}");
                continue;
            }

            match parse_batch_token_results(&results[base_idx..base_idx + 3]) {
                Ok(token_info) => {
                    token_infos.insert(*token_address, token_info);
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to parse token info for token {token_address} with error: {e}"
                    );
                }
            }
        }

        Ok(token_infos)
    }
}

/// Generic parser for ERC20 string results (name, symbol)
fn parse_erc20_string_result(
    result: &Multicall3::Result,
    field_name: &str,
) -> Result<String, BlockchainRpcClientError> {
    // Common validation
    if !result.success {
        return Err(BlockchainRpcClientError::AbiDecodingError(format!(
            "Multicall failed for {field_name}"
        )));
    }

    if result.returnData.is_empty() {
        return Err(BlockchainRpcClientError::AbiDecodingError(format!(
            "Empty response for {field_name}"
        )));
    }

    // Decode based on field
    match field_name {
        "name" => ERC20::nameCall::abi_decode_returns(&result.returnData),
        "symbol" => ERC20::symbolCall::abi_decode_returns(&result.returnData),
        _ => {
            return Err(BlockchainRpcClientError::AbiDecodingError(format!(
                "Unknown field: {field_name}"
            )));
        }
    }
    .map_err(|e| {
        BlockchainRpcClientError::AbiDecodingError(format!("Failed to decode {field_name}: {e}"))
    })
}

/// Generic parser for ERC20 decimals result
fn parse_erc20_decimals_result(
    result: &Multicall3::Result,
) -> Result<u8, BlockchainRpcClientError> {
    // Common validation
    if !result.success {
        return Err(BlockchainRpcClientError::AbiDecodingError(
            "Multicall failed for decimals".to_string(),
        ));
    }

    if result.returnData.is_empty() {
        return Err(BlockchainRpcClientError::AbiDecodingError(
            "Empty response for decimals".to_string(),
        ));
    }

    ERC20::decimalsCall::abi_decode_returns(&result.returnData).map_err(|e| {
        BlockchainRpcClientError::AbiDecodingError(format!("Failed to decode decimals: {e}"))
    })
}

/// Parses token information from a slice of 3 multicall results.
///
/// Expects results in order: name, symbol, decimals.
/// Returns Ok(TokenInfo) if all three calls succeeded, or an Err with a
/// descriptive error message if any call failed.
fn parse_batch_token_results(
    results: &[Multicall3::Result],
) -> Result<TokenInfo, BlockchainRpcClientError> {
    if results.len() != 3 {
        return Err(BlockchainRpcClientError::AbiDecodingError(
            "Expected exactly 3 results per token".to_string(),
        ));
    }

    let name = parse_erc20_string_result(&results[0], "name")?;
    let symbol = parse_erc20_string_result(&results[1], "symbol")?;
    let decimals = parse_erc20_decimals_result(&results[2])?;

    Ok(TokenInfo {
        name,
        symbol,
        decimals,
    })
}

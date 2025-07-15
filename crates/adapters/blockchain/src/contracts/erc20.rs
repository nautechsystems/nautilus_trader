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

use std::sync::Arc;

use alloy::{primitives::Address, sol, sol_types::SolCall};

use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract ERC20 {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
    }
}

sol! {
    #[sol(rpc)]
    contract Multicall3 {
        struct Call3 {
            address target;
            bool allowFailure;
            bytes callData;
        }
        
        struct Result {
            bool success;
            bytes returnData;
        }
        
        function aggregate3(Call3[] calldata calls) external payable returns (Result[] memory returnData);
    }
}

/// Standard Multicall3 address (same on all EVM chains)
const MULTICALL3_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

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
    /// The HTTP RPC client used to communicate with the blockchain node.
    client: Arc<BlockchainHttpRpcClient>,
}

/// Decodes a hexadecimal string response from a blockchain RPC call.
///
/// # Errors
///
/// Returns an `BlockchainRpcClientError::AbiDecodingError` if the hex decoding fails.
fn decode_hex_response(encoded_response: &str) -> Result<Vec<u8>, BlockchainRpcClientError> {
    // Remove the "0x" prefix if present
    let encoded_str = encoded_response
        .strip_prefix("0x")
        .unwrap_or(encoded_response);
    hex::decode(encoded_str).map_err(|e| {
        BlockchainRpcClientError::AbiDecodingError(format!("Error decoding hex response: {e}"))
    })
}

impl Erc20Contract {
    /// Creates a new ERC20 contract interface with the specified RPC client.
    #[must_use]
    pub const fn new(client: Arc<BlockchainHttpRpcClient>) -> Self {
        Self { client }
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
        // Try multicall first, fallback to individual calls if it fails
        match self.fetch_token_info_multicall(token_address).await {
            Ok(info) => Ok(info),
            Err(_) => {
                // Fallback to individual calls
                let token_name = self.fetch_name(token_address).await?;
                let token_symbol = self.fetch_symbol(token_address).await?;
                let token_decimals = self.fetch_decimals(token_address).await?;

                Ok(TokenInfo {
                    name: token_name,
                    symbol: token_symbol,
                    decimals: token_decimals,
                })
            }
        }
    }

    /// Fetches complete token information using multicall for efficiency.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall fails or decoding fails.
    pub async fn fetch_token_info_multicall(
        &self,
        token_address: &Address,
    ) -> Result<TokenInfo, BlockchainRpcClientError> {
        let multicall_address = Address::parse_checksummed(MULTICALL3_ADDRESS, None)
            .map_err(|e| {
                BlockchainRpcClientError::AbiDecodingError(format!("Invalid multicall address: {e}"))
            })?;

        // Prepare the three calls
        let calls = vec![
            Multicall3::Call3 {
                target: *token_address,
                allowFailure: false,
                callData: ERC20::nameCall.abi_encode().into(),
            },
            Multicall3::Call3 {
                target: *token_address,
                allowFailure: false,
                callData: ERC20::symbolCall.abi_encode().into(),
            },
            Multicall3::Call3 {
                target: *token_address,
                allowFailure: false,
                callData: ERC20::decimalsCall.abi_encode().into(),
            },
        ];

        // Encode the multicall
        let multicall_data = Multicall3::aggregate3Call { calls }.abi_encode();

        // Execute the multicall
        let rpc_request = self
            .client
            .construct_eth_call(&multicall_address.to_string(), multicall_data.as_slice());

        let encoded_response = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| BlockchainRpcClientError::ClientError(format!("Multicall failed: {e}")))?;

        let bytes = decode_hex_response(&encoded_response)?;

        // Decode the multicall results
        let results = Multicall3::aggregate3Call::abi_decode_returns(&bytes)
            .map_err(|e| {
                BlockchainRpcClientError::AbiDecodingError(format!(
                    "Failed to decode multicall results: {e}"
                ))
            })?;

        // Parse individual results
        if results.len() != 3 {
            return Err(BlockchainRpcClientError::AbiDecodingError(
                "Unexpected number of results from multicall".to_string(),
            ));
        }

        let name = parse_multicall_string_result(&results[0], "name")?;
        let symbol = parse_multicall_string_result(&results[1], "symbol")?;
        let decimals = parse_multicall_u8_result(&results[2], "decimals")?;

        Ok(TokenInfo {
            name,
            symbol,
            decimals,
        })
    }

    /// Fetches the name of an ERC20 token.
    async fn fetch_name(
        &self,
        token_address: &Address,
    ) -> Result<String, BlockchainRpcClientError> {
        let name_call = ERC20::nameCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(&token_address.to_string(), name_call.as_slice());
        let encoded_name = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching name: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_name)?;

        if bytes.is_empty() {
            return Err(BlockchainRpcClientError::AbiDecodingError(
                "Token does not implement name() function or returned empty response".to_string(),
            ));
        }

        ERC20::nameCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract name with error {e}"
            ))
        })
    }

    /// Fetches the symbol of an ERC20 token.
    async fn fetch_symbol(
        &self,
        token_address: &Address,
    ) -> Result<String, BlockchainRpcClientError> {
        let symbol_call = ERC20::symbolCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(&token_address.to_string(), symbol_call.as_slice());
        let encoded_symbol = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching symbol: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_symbol)?;

        if bytes.is_empty() {
            return Err(BlockchainRpcClientError::AbiDecodingError(
                "Token not implement symbol() function or returned empty response".to_string(),
            ));
        }

        ERC20::symbolCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract symbol with error {e}"
            ))
        })
    }

    /// Fetches the number of decimals used by an ERC20 token.
    async fn fetch_decimals(
        &self,
        token_address: &Address,
    ) -> Result<u8, BlockchainRpcClientError> {
        let decimals_call = ERC20::decimalsCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(&token_address.to_string(), decimals_call.as_slice());
        let encoded_decimals = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching decimals: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_decimals)?;

        if bytes.is_empty() {
            return Err(BlockchainRpcClientError::AbiDecodingError(
                "Token does not implement decimals() function or returned empty response"
                    .to_string(),
            ));
        }

        ERC20::decimalsCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract decimals with error {e}"
            ))
        })
    }
}

/// Parses a string result from a multicall response.
///
/// # Errors
///
/// Returns an error if the call failed or decoding fails.
fn parse_multicall_string_result(
    result: &Multicall3::Result,
    field_name: &str,
) -> Result<String, BlockchainRpcClientError> {
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

    match field_name {
        "name" => ERC20::nameCall::abi_decode_returns(&result.returnData).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Failed to decode {field_name}: {e}"
            ))
        }),
        "symbol" => ERC20::symbolCall::abi_decode_returns(&result.returnData).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Failed to decode {field_name}: {e}"
            ))
        }),
        _ => Err(BlockchainRpcClientError::AbiDecodingError(format!(
            "Unknown field: {field_name}"
        ))),
    }
}

/// Parses a u8 result from a multicall response.
///
/// # Errors
///
/// Returns an error if the call failed or decoding fails.
fn parse_multicall_u8_result(
    result: &Multicall3::Result,
    field_name: &str,
) -> Result<u8, BlockchainRpcClientError> {
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

    ERC20::decimalsCall::abi_decode_returns(&result.returnData).map_err(|e| {
        BlockchainRpcClientError::AbiDecodingError(format!(
            "Failed to decode {field_name}: {e}"
        ))
    })
}

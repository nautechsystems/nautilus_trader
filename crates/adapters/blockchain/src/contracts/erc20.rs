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

use alloy::{sol, sol_types::SolCall};

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
        token_address: &str,
    ) -> Result<TokenInfo, BlockchainRpcClientError> {
        let token_name = self.fetch_name(token_address).await?;
        let token_symbol = self.fetch_symbol(token_address).await?;
        let token_decimals = self.fetch_decimals(token_address).await?;

        Ok(TokenInfo {
            name: token_name,
            symbol: token_symbol,
            decimals: token_decimals,
        })
    }

    /// Fetches the name of an ERC20 token.
    async fn fetch_name(&self, token_address: &str) -> Result<String, BlockchainRpcClientError> {
        let name_call = ERC20::nameCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(token_address, name_call.as_slice());
        let encoded_name = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching name: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_name)?;
        ERC20::nameCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract name with error {e}"
            ))
        })
    }

    /// Fetches the symbol of an ERC20 token.
    async fn fetch_symbol(&self, token_address: &str) -> Result<String, BlockchainRpcClientError> {
        let symbol_call = ERC20::symbolCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(token_address, symbol_call.as_slice());
        let encoded_symbol = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching symbol: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_symbol)?;
        ERC20::symbolCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract symbol with error {e}"
            ))
        })
    }

    /// Fetches the number of decimals used by an ERC20 token.
    async fn fetch_decimals(&self, token_address: &str) -> Result<u8, BlockchainRpcClientError> {
        let decimals_call = ERC20::decimalsCall.abi_encode();
        let rpc_request = self
            .client
            .construct_eth_call(token_address, decimals_call.as_slice());
        let encoded_decimals = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| {
                BlockchainRpcClientError::ClientError(format!("Error fetching decimals: {e}"))
            })?;
        let bytes = decode_hex_response(&encoded_decimals)?;
        ERC20::decimalsCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Error decoding ERC20 contract decimals with error {e}"
            ))
        })
    }
}

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
use nautilus_model::defi::validation::validate_address;

use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract Multicall3 {
        struct Call {
            address target;
            bytes callData;
        }

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
        function tryAggregate(bool requireSuccess, Call[] calldata calls) external payable returns (Result[] memory returnData);
    }
}

/// Standard Multicall3 address (same on all EVM chains).
pub const MULTICALL3_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

/// Base contract functionality for interacting with blockchain contracts.
///
/// This struct provides common RPC execution patterns that can be reused
/// by specific contract implementations like ERC20, ERC721, etc.
#[derive(Debug)]
pub struct BaseContract {
    /// The HTTP RPC client used to communicate with the blockchain node.
    client: Arc<BlockchainHttpRpcClient>,
    /// The Multicall3 contract address.
    multicall_address: Address,
}

/// Represents a single contract call for batching in multicall.
#[derive(Debug)]
pub struct ContractCall {
    /// The target contract address
    pub target: Address,
    /// Whether this call can fail without reverting the entire multicall.
    pub allow_failure: bool,
    /// The encoded call data.
    pub call_data: Vec<u8>,
}

impl BaseContract {
    /// Creates a new base contract interface with the specified RPC client.
    ///
    /// # Panics
    ///
    /// Panics if the multicall address is invalid (which should never happen with the hardcoded address).
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>) -> Self {
        let multicall_address =
            validate_address(MULTICALL3_ADDRESS).expect("Invalid multicall address");

        Self {
            client,
            multicall_address,
        }
    }

    /// Gets a reference to the RPC client.
    #[must_use]
    pub const fn client(&self) -> &Arc<BlockchainHttpRpcClient> {
        &self.client
    }

    /// Executes a single contract call and returns the raw response bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or response decoding fails.
    pub async fn execute_call(
        &self,
        contract_address: &Address,
        call_data: &[u8],
        block: Option<u64>,
    ) -> Result<Vec<u8>, BlockchainRpcClientError> {
        let rpc_request =
            self.client
                .construct_eth_call(&contract_address.to_string(), call_data, block);

        let encoded_response = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| BlockchainRpcClientError::ClientError(format!("RPC call failed: {e}")))?;

        decode_hex_response(&encoded_response)
    }

    /// Executes multiple contract calls in a single multicall transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall fails or decoding fails.
    pub async fn execute_multicall(
        &self,
        calls: Vec<ContractCall>,
        block: Option<u64>,
    ) -> Result<Vec<Multicall3::Result>, BlockchainRpcClientError> {
        // Convert to Multicall3 format.
        let multicall_calls: Vec<Multicall3::Call> = calls
            .into_iter()
            .map(|call| Multicall3::Call {
                target: call.target,
                callData: call.call_data.into(),
            })
            .collect();

        let multicall_data = Multicall3::tryAggregateCall {
            requireSuccess: false,
            calls: multicall_calls,
        }
        .abi_encode();
        let rpc_request = self.client.construct_eth_call(
            &self.multicall_address.to_string(),
            multicall_data.as_slice(),
            block,
        );

        let encoded_response = self
            .client
            .execute_eth_call::<String>(rpc_request)
            .await
            .map_err(|e| BlockchainRpcClientError::ClientError(format!("Multicall failed: {e}")))?;

        let bytes = decode_hex_response(&encoded_response)?;
        let results = Multicall3::tryAggregateCall::abi_decode_returns(&bytes).map_err(|e| {
            BlockchainRpcClientError::AbiDecodingError(format!(
                "Failed to decode multicall results: {e}"
            ))
        })?;

        Ok(results)
    }
}

/// Decodes a hexadecimal string response from a blockchain RPC call.
///
/// # Errors
///
/// Returns an `BlockchainRpcClientError::AbiDecodingError` if the hex decoding fails.
pub fn decode_hex_response(encoded_response: &str) -> Result<Vec<u8>, BlockchainRpcClientError> {
    // Remove the "0x" prefix if present
    let encoded_str = encoded_response
        .strip_prefix("0x")
        .unwrap_or(encoded_response);
    hex::decode(encoded_str).map_err(|e| {
        BlockchainRpcClientError::AbiDecodingError(format!("Error decoding hex response: {e}"))
    })
}

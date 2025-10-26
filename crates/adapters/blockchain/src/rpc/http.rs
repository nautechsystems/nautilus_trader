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

use std::{collections::HashMap, num::NonZeroU32};

use bytes::Bytes;
use nautilus_model::defi::rpc::RpcNodeHttpResponse;
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::Method;
use serde::de::DeserializeOwned;

use crate::rpc::error::BlockchainRpcClientError;

/// Client for making HTTP-based RPC requests to blockchain nodes.
///
/// This client is designed to interact with Ethereum-compatible blockchain networks, providing
/// methods to execute RPC calls and handle responses in a type-safe manner.
#[derive(Debug)]
pub struct BlockchainHttpRpcClient {
    /// The HTTP URL for the blockchain node's RPC endpoint.
    http_rpc_url: String,
    /// The HTTP client for making RPC http-based requests.
    http_client: HttpClient,
}

impl BlockchainHttpRpcClient {
    /// Creates a new HTTP RPC client with the given endpoint URL and optional rate limit.
    ///
    /// # Panics
    ///
    /// Panics if `rpc_request_per_second` is `Some(0)`, since a zero rate limit is invalid.
    #[must_use]
    pub fn new(http_rpc_url: String, rpc_request_per_second: Option<u32>) -> Self {
        let default_quota = rpc_request_per_second.map(|rpc_request_per_second| {
            Quota::per_second(NonZeroU32::new(rpc_request_per_second).unwrap())
        });
        let http_client = HttpClient::new(HashMap::new(), vec![], Vec::new(), default_quota, None);
        Self {
            http_rpc_url,
            http_client,
        }
    }

    /// Generic method that sends a JSON-RPC request and returns the raw response in bytes.
    async fn send_rpc_request(
        &self,
        rpc_request: serde_json::Value,
    ) -> Result<Bytes, BlockchainRpcClientError> {
        let body_bytes = serde_json::to_vec(&rpc_request).map_err(|e| {
            BlockchainRpcClientError::ClientError(format!("Failed to serialize request: {e}"))
        })?;

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        match self
            .http_client
            .request(
                Method::POST,
                self.http_rpc_url.clone(),
                Some(headers),
                Some(body_bytes),
                None,
                None,
            )
            .await
        {
            Ok(response) => Ok(response.body),
            Err(e) => Err(BlockchainRpcClientError::ClientError(e.to_string())),
        }
    }

    /// Executes an Ethereum JSON-RPC call and deserializes the response into the specified type T.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP RPC request fails or the response cannot be parsed.
    pub async fn execute_eth_call<T: DeserializeOwned>(
        &self,
        rpc_request: serde_json::Value,
    ) -> anyhow::Result<T> {
        match self.send_rpc_request(rpc_request).await {
            Ok(bytes) => match serde_json::from_slice::<RpcNodeHttpResponse<T>>(bytes.as_ref()) {
                Ok(parsed) => {
                    if let Some(error) = parsed.error {
                        Err(anyhow::anyhow!(
                            "RPC error {}: {}",
                            error.code,
                            error.message
                        ))
                    } else if let Some(result) = parsed.result {
                        Ok(result)
                    } else {
                        Err(anyhow::anyhow!(
                            "Response missing both result and error fields"
                        ))
                    }
                }
                Err(e) => {
                    // Try to convert bytes to string for better error reporting
                    let raw_response = String::from_utf8_lossy(bytes.as_ref());
                    let preview = if raw_response.len() > 500 {
                        format!(
                            "{}... (truncated, {} bytes total)",
                            &raw_response[..500],
                            raw_response.len()
                        )
                    } else {
                        raw_response.to_string()
                    };

                    Err(anyhow::anyhow!(
                        "Failed to parse eth call response: {}\nRaw response: {}",
                        e,
                        preview
                    ))
                }
            },
            Err(e) => Err(anyhow::anyhow!(
                "Failed to execute eth call RPC request: {}",
                e
            )),
        }
    }

    /// Creates a properly formatted `eth_call` JSON-RPC request object targeting a specific contract address with encoded function data.
    #[must_use]
    pub fn construct_eth_call(
        &self,
        to: &str,
        call_data: &[u8],
        block: Option<u64>,
    ) -> serde_json::Value {
        let encoded_data = format!("0x{}", hex::encode(call_data));
        let call = serde_json::json!({
            "to": to,
            "data": encoded_data
        });

        let block_param = if let Some(block_number) = block {
            serde_json::json!(format!("0x{:x}", block_number))
        } else {
            serde_json::json!("latest")
        };

        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_call",
            "params": [call, block_param]
        })
    }
}

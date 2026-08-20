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

use std::{collections::HashMap, fmt::Debug, num::NonZeroU32, str::FromStr};

use alloy::primitives::{Address, B256, Bytes, U256};
use bytes::Bytes as HttpBytes;
use nautilus_core::{hex, string::secret::REDACTED};
use nautilus_model::defi::rpc::{RpcLog, RpcNodeHttpResponse};
use nautilus_network::{
    http::{HttpClient, HttpClientError, Method},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use crate::rpc::{
    error::{BlockchainRpcClientError, BroadcastError},
    types::{RpcBlock, RpcBlockResponse, RpcTransactionReceipt},
};

/// Per-request timeout for execution RPC calls, in seconds.
pub const EXECUTION_RPC_TIMEOUT_SECS: u64 = 10;

/// Client for making HTTP-based RPC requests to blockchain nodes.
///
/// This client is designed to interact with Ethereum-compatible blockchain networks, providing
/// methods to execute RPC calls and handle responses in a type-safe manner.
pub struct BlockchainHttpRpcClient {
    /// The HTTP URL for the blockchain node's RPC endpoint.
    http_rpc_url: String,
    /// The HTTP client for making RPC http-based requests.
    http_client: HttpClient,
}

impl Debug for BlockchainHttpRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BlockchainHttpRpcClient))
            .field("http_rpc_url", &REDACTED)
            .field("http_client", &self.http_client)
            .finish()
    }
}

impl BlockchainHttpRpcClient {
    /// Creates a new HTTP RPC client with the given endpoint URL and optional rate limit.
    ///
    /// If `rpc_request_per_second` is `Some(0)` or an invalid value, rate limiting is disabled.
    ///
    /// # Panics
    ///
    /// Panics if the internal HTTP client cannot be created.
    #[must_use]
    pub fn new(
        http_rpc_url: String,
        rpc_request_per_second: Option<u32>,
        proxy_url: Option<String>,
    ) -> Self {
        let default_quota =
            rpc_request_per_second.and_then(|rps| Quota::per_second(NonZeroU32::new(rps)?));
        let http_client = HttpClient::new(
            HashMap::new(),
            vec![],
            Vec::new(),
            default_quota,
            None, // timeout_secs
            proxy_url,
        )
        .expect("Failed to create HTTP client");
        Self {
            http_rpc_url,
            http_client,
        }
    }

    /// Generic method that sends a JSON-RPC request and returns the raw response in bytes.
    async fn send_rpc_request(
        &self,
        rpc_request: serde_json::Value,
        timeout_secs: Option<u64>,
    ) -> Result<HttpBytes, BlockchainRpcClientError> {
        let body_bytes = serde_json::to_vec(&rpc_request).map_err(|e| {
            BlockchainRpcClientError::ClientError(format!("Failed to serialize request: {e}"))
        })?;

        self.post_json_body(body_bytes, timeout_secs)
            .await
            .map_err(|e| BlockchainRpcClientError::ClientError(e.to_string()))
    }

    async fn post_json_body(
        &self,
        body_bytes: Vec<u8>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpBytes, HttpClientError> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let response = self
            .http_client
            .request_with_url_redacted(
                Method::POST,
                self.http_rpc_url.clone(),
                None,
                Some(headers),
                Some(body_bytes),
                timeout_secs,
                None,
            )
            .await?;

        Ok(response.body)
    }

    /// Executes an Ethereum JSON-RPC call and deserializes the response into the specified type T.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP RPC request fails or the response cannot be parsed.
    pub async fn execute_rpc_call<T: DeserializeOwned>(
        &self,
        rpc_request: serde_json::Value,
    ) -> anyhow::Result<T> {
        self.execute_rpc_call_with_timeout(rpc_request, None).await
    }

    /// Executes an Ethereum JSON-RPC call with an optional per-request timeout and deserializes
    /// the response into the specified type T.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP RPC request fails or the response cannot be parsed.
    pub async fn execute_rpc_call_with_timeout<T: DeserializeOwned>(
        &self,
        rpc_request: serde_json::Value,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<T> {
        match self.send_rpc_request(rpc_request, timeout_secs).await {
            Ok(bytes) => match serde_json::from_slice::<RpcNodeHttpResponse<T>>(bytes.as_ref()) {
                Ok(parsed) => {
                    // Check for non-standard rate limit error (e.g., Infura)
                    // These responses have code/message at top level without jsonrpc field
                    if parsed.jsonrpc.is_none()
                        && let (Some(code), Some(message)) = (parsed.code, parsed.message)
                    {
                        anyhow::bail!("RPC provider error {code}: {message}");
                    }

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
                        "Failed to parse eth call response: {e}\nRaw response: {preview}"
                    ))
                }
            },
            Err(e) => Err(anyhow::anyhow!(
                "Failed to execute eth call RPC request: {e}"
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
        self.construct_eth_call_request(None, to, call_data, block)
    }

    #[cfg(feature = "hypersync")]
    #[must_use]
    pub(crate) fn construct_eth_call_from(
        &self,
        from: &Address,
        to: &str,
        call_data: &[u8],
        block: Option<u64>,
    ) -> serde_json::Value {
        self.construct_eth_call_request(Some(from), to, call_data, block)
    }

    fn construct_eth_call_request(
        &self,
        from: Option<&Address>,
        to: &str,
        call_data: &[u8],
        block: Option<u64>,
    ) -> serde_json::Value {
        let encoded_data = hex::encode_prefixed(call_data);
        let mut call = serde_json::json!({
            "to": to,
            "data": encoded_data
        });

        if let Some(from) = from {
            call["from"] = serde_json::Value::String(from.to_string());
        }

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

    /// Retrieves the balance of the specified Ethereum address at the given block.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or if the returned balance string cannot be parsed as a valid U256.
    pub async fn get_balance(&self, address: &Address, block: Option<u64>) -> anyhow::Result<U256> {
        self.get_balance_with_timeout(address, block, None).await
    }

    /// Retrieves the balance of the specified Ethereum address at the given block with an
    /// optional per-request timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or if the returned balance string cannot be parsed as a valid U256.
    pub async fn get_balance_with_timeout(
        &self,
        address: &Address,
        block: Option<u64>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<U256> {
        let block_param = if let Some(block_number) = block {
            serde_json::json!(format!("0x{:x}", block_number))
        } else {
            serde_json::json!("latest")
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBalance",
            "params": [address, block_param]
        });
        let hex_string: String = self
            .execute_rpc_call_with_timeout(request, timeout_secs)
            .await?;

        U256::from_str(&hex_string)
            .map_err(|e| anyhow::anyhow!("Failed to parse balance hex string '{hex_string}': {e}"))
    }

    /// Retrieves logs matching the given filter criteria.
    ///
    /// This method calls the `eth_getLogs` RPC method to fetch event logs from the blockchain.
    /// It's commonly used for querying historical events like token transfers, swaps, etc.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the response cannot be parsed.
    pub async fn get_logs(
        &self,
        address: Option<&Address>,
        topics: Option<Vec<Option<String>>>,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<Vec<RpcLog>> {
        let mut filter = serde_json::Map::new();

        filter.insert(
            "fromBlock".to_string(),
            serde_json::json!(format!("0x{:x}", from_block)),
        );
        filter.insert(
            "toBlock".to_string(),
            serde_json::json!(format!("0x{:x}", to_block)),
        );

        if let Some(addr) = address {
            filter.insert(
                "address".to_string(),
                serde_json::json!(format!("{:?}", addr)),
            );
        }

        if let Some(topics) = topics {
            filter.insert("topics".to_string(), serde_json::json!(topics));
        }

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getLogs",
            "params": [filter]
        });

        self.execute_rpc_call(request).await
    }

    /// Executes an execution-path JSON-RPC call with a per-request timeout and sanitized errors.
    ///
    /// A `null` result maps to `Ok(None)`, which is a legitimate pending response (for example a
    /// receipt that does not exist yet), not an error. Errors carry the RPC method and numeric
    /// code only, never the endpoint URL or request payload.
    async fn execute_execution_rpc_call<T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> anyhow::Result<Option<T>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let bytes = self
            .send_rpc_request(request, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await
            .map_err(|_| anyhow::anyhow!("{method} request failed"))?;

        let parsed = serde_json::from_slice::<RpcNodeHttpResponse<T>>(bytes.as_ref())
            .map_err(|_| anyhow::anyhow!("Failed to parse {method} response"))?;

        if parsed.jsonrpc.is_none()
            && let (Some(code), Some(_message)) = (parsed.code, parsed.message)
        {
            anyhow::bail!("{method} RPC error {code}");
        }

        if let Some(error) = parsed.error {
            anyhow::bail!("{method} RPC error {}", error.code);
        }

        Ok(parsed.result)
    }

    /// Returns the chain ID reported by the RPC node via `eth_chainId`.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn chain_id(&self) -> anyhow::Result<u64> {
        let result: Option<String> = self
            .execute_execution_rpc_call("eth_chainId", serde_json::json!([]))
            .await?;
        parse_hex_quantity_result("eth_chainId", result)
            .and_then(|v| u64::try_from(v).map_err(Into::into))
    }

    /// Returns the deployed bytecode at the given address via `eth_getCode` at the latest block.
    ///
    /// An address with no deployed contract returns empty bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn get_code(&self, address: &Address) -> anyhow::Result<Bytes> {
        let result: Option<String> = self
            .execute_execution_rpc_call("eth_getCode", serde_json::json!([address, "latest"]))
            .await?;
        let hex_string = result.ok_or_else(|| anyhow::anyhow!("eth_getCode returned no result"))?;
        let stripped = hex_string.strip_prefix("0x").unwrap_or(&hex_string);
        let bytes = hex::decode(stripped)
            .map_err(|e| anyhow::anyhow!("Failed to decode eth_getCode result: {e}"))?;
        Ok(Bytes::from(bytes))
    }

    /// Returns the next nonce for the given address via `eth_getTransactionCount`
    /// with the `pending` tag, making the pending pool state authoritative.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn get_transaction_count_pending(&self, address: &Address) -> anyhow::Result<u64> {
        let result: Option<String> = self
            .execute_execution_rpc_call(
                "eth_getTransactionCount",
                serde_json::json!([address, "pending"]),
            )
            .await?;
        parse_hex_quantity_result("eth_getTransactionCount", result)
            .and_then(|v| u64::try_from(v).map_err(Into::into))
    }

    /// Returns the latest mined nonce for an address.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn get_transaction_count_latest(&self, address: &Address) -> anyhow::Result<u64> {
        let result: Option<String> = self
            .execute_execution_rpc_call(
                "eth_getTransactionCount",
                serde_json::json!([address, "latest"]),
            )
            .await?;
        parse_hex_quantity_result("eth_getTransactionCount", result)
            .and_then(|v| u64::try_from(v).map_err(Into::into))
    }

    /// Estimates the gas required for a transaction via `eth_estimateGas`.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails (including node-side revert of the simulated
    /// transaction) or the result is missing or malformed.
    pub async fn estimate_gas(
        &self,
        from: &Address,
        to: &Address,
        value: U256,
        data: &[u8],
    ) -> anyhow::Result<u64> {
        let call = serde_json::json!({
            "from": from,
            "to": to,
            "value": format!("0x{value:x}"),
            "data": hex::encode_prefixed(data),
        });
        let result: Option<String> = self
            .execute_execution_rpc_call("eth_estimateGas", serde_json::json!([call]))
            .await?;
        parse_hex_quantity_result("eth_estimateGas", result)
            .and_then(|v| u64::try_from(v).map_err(Into::into))
    }

    /// Returns the node's suggested max priority fee per gas in wei
    /// via `eth_maxPriorityFeePerGas`.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn max_priority_fee_per_gas(&self) -> anyhow::Result<u128> {
        let result: Option<String> = self
            .execute_execution_rpc_call("eth_maxPriorityFeePerGas", serde_json::json!([]))
            .await?;
        parse_hex_quantity_result("eth_maxPriorityFeePerGas", result)
    }

    /// Returns the latest block via `eth_getBlockByNumber` with the `latest` tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn latest_block(&self) -> anyhow::Result<RpcBlock> {
        self.block_by_tag("latest", false).await
    }

    /// Returns the consensus finalized block.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint does not support the `finalized` tag, the RPC call
    /// fails, or the result is missing or malformed.
    pub async fn finalized_block(&self) -> anyhow::Result<RpcBlock> {
        self.block_by_tag("finalized", false).await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to read the consensus finalized block; the execution endpoint must support the finalized tag: {e}"
            )
        })
    }

    /// Returns a numbered canonical block, optionally with full transactions.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result is missing or malformed.
    pub async fn block_by_number(
        &self,
        number: u64,
        full_transactions: bool,
    ) -> anyhow::Result<RpcBlock> {
        self.block_by_tag(&format!("0x{number:x}"), full_transactions)
            .await
    }

    async fn block_by_tag(&self, tag: &str, full_transactions: bool) -> anyhow::Result<RpcBlock> {
        let result: Option<RpcBlockResponse> = self
            .execute_execution_rpc_call(
                "eth_getBlockByNumber",
                serde_json::json!([tag, full_transactions]),
            )
            .await?;
        let response = result.ok_or_else(|| {
            anyhow::anyhow!("eth_getBlockByNumber returned no result for block tag {tag}")
        })?;
        let mut block = response.block;
        if full_transactions {
            block.transactions = response
                .transactions
                .into_iter()
                .map(|transaction| {
                    serde_json::from_value(transaction).map_err(|_| {
                        anyhow::anyhow!(
                            "Failed to parse full transaction in eth_getBlockByNumber response"
                        )
                    })
                })
                .collect::<anyhow::Result<_>>()?;
        }
        Ok(block)
    }

    /// Returns the receipt for the given transaction hash via `eth_getTransactionReceipt`.
    ///
    /// A `null` result maps to `Ok(None)`: the transaction is pending and no receipt
    /// exists yet.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the response is malformed.
    pub async fn get_transaction_receipt(
        &self,
        tx_hash: &B256,
    ) -> anyhow::Result<Option<RpcTransactionReceipt>> {
        let receipt: Option<RpcTransactionReceipt> = self
            .execute_execution_rpc_call("eth_getTransactionReceipt", serde_json::json!([tx_hash]))
            .await?;

        if receipt
            .as_ref()
            .is_some_and(|receipt| receipt.transaction_hash != *tx_hash)
        {
            anyhow::bail!(
                "eth_getTransactionReceipt returned a receipt with a mismatched transaction hash"
            );
        }

        Ok(receipt)
    }

    /// Broadcasts a signed raw transaction via `eth_sendRawTransaction`.
    ///
    /// Broadcast failures classify before retry: an `already known` response is acceptance
    /// (returns `expected_tx_hash`), and a timeout after sending is
    /// [`BroadcastError::TimeoutAfterSend`] so the caller reconciles through the persisted
    /// record rather than rebroadcasting.
    ///
    /// # Errors
    ///
    /// Returns a [`BroadcastError`] classifying the failure. Errors are sanitized: they carry
    /// the numeric RPC code only, never the endpoint URL or the signed payload.
    pub async fn send_raw_transaction(
        &self,
        raw_tx: &[u8],
        expected_tx_hash: &B256,
    ) -> Result<B256, BroadcastError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendRawTransaction",
            "params": [hex::encode_prefixed(raw_tx)],
        });

        let body_bytes = serde_json::to_vec(&request)
            .map_err(|e| BroadcastError::Failed(format!("Failed to serialize request: {e}")))?;

        let body = self
            .post_json_body(body_bytes, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await
            .map_err(|e| classify_broadcast_transport_error(&e))?;

        let parsed =
            serde_json::from_slice::<RpcNodeHttpResponse<String>>(&body).map_err(|_| {
                BroadcastError::Failed("Failed to parse broadcast response".to_string())
            })?;

        if let Some(error) = parsed.error {
            let message = error.message.to_ascii_lowercase();
            if message.contains("already known") {
                log::warn!(
                    "Broadcast returned 'already known' for transaction {expected_tx_hash}; treating as acceptance"
                );
                return Ok(*expected_tx_hash);
            }

            if message.contains("nonce too low") {
                return Err(BroadcastError::Failed(format!(
                    "node RPC error {} reported a consumed nonce",
                    error.code
                )));
            }
            return Err(BroadcastError::Rejected { code: error.code });
        }

        let hex_string = parsed
            .result
            .ok_or_else(|| BroadcastError::Failed("Broadcast returned no result".to_string()))?;

        B256::from_str(&hex_string)
            .map_err(|e| BroadcastError::Failed(format!("Failed to parse broadcast result: {e}")))
    }
}

/// Classifies a broadcast transport failure: a timeout after sending reconciles through the
/// persisted record. Any other transport failure is treated as ambiguous-after-send too, even
/// though some (for example connection refused) may never have reached the node; the HTTP
/// client does not expose the connect-phase distinction, so the conservative outcome never
/// risks a rebroadcast. Sanitized to never carry the endpoint URL or request payload.
fn classify_broadcast_transport_error(error: &HttpClientError) -> BroadcastError {
    match error {
        HttpClientError::TimeoutError(_) => BroadcastError::TimeoutAfterSend,
        _ => BroadcastError::Failed("transport error".to_string()),
    }
}

/// Parses a required hex-quantity JSON-RPC result, erroring on a missing result.
fn parse_hex_quantity_result(method: &str, result: Option<String>) -> anyhow::Result<u128> {
    let hex_string = result.ok_or_else(|| anyhow::anyhow!("{method} returned no result"))?;
    let stripped = hex_string.strip_prefix("0x").unwrap_or(&hex_string);
    u128::from_str_radix(stripped, 16)
        .map_err(|e| anyhow::anyhow!("Failed to parse {method} result '{hex_string}': {e}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use alloy::primitives::{address, b256};
    use rstest::rstest;

    use super::*;

    /// Mock JSON-RPC HTTP server for tests, serving canned responses from fixture files.
    pub(crate) mod mock {
        use std::{
            collections::{HashMap, VecDeque},
            net::SocketAddr,
            sync::{Arc, Mutex},
            time::Duration,
        };

        use axum::{Router, extract::State, routing::post};
        use serde_json::Value;

        type ResponseSequences<K> = Arc<Mutex<HashMap<K, VecDeque<String>>>>;

        /// State for the mock JSON-RPC server: canned responses per method plus a request log.
        #[derive(Clone, Default)]
        pub(crate) struct MockRpcState {
            responses: HashMap<String, String>,
            parameter_responses: HashMap<(String, String), String>,
            parameter_response_sequences: ResponseSequences<(String, String)>,
            response_sequences: ResponseSequences<String>,
            call_responses: HashMap<String, String>,
            sleep_methods: HashMap<String, Duration>,
            requests: Arc<Mutex<Vec<Value>>>,
            receipt_hash_from_request: bool,
            send_raw_echo: bool,
        }

        impl MockRpcState {
            /// Serves the given raw JSON-RPC response body for `method`.
            #[must_use]
            pub(crate) fn with_response(mut self, method: &str, response_json: &str) -> Self {
                self.responses
                    .insert(method.to_string(), response_json.to_string());
                self
            }

            /// Serves a response for a method whose first parameter exactly matches `parameter`.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_parameter_response(
                mut self,
                method: &str,
                parameter: &str,
                response_json: &str,
            ) -> Self {
                self.parameter_responses.insert(
                    (method.to_string(), parameter.to_string()),
                    response_json.to_string(),
                );
                self
            }

            /// Serves responses in order for a method whose first parameter exactly matches.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_parameter_response_sequence(
                self,
                method: &str,
                parameter: &str,
                responses: &[&str],
            ) -> Self {
                self.parameter_response_sequences.lock().unwrap().insert(
                    (method.to_string(), parameter.to_string()),
                    responses.iter().map(ToString::to_string).collect(),
                );
                self
            }

            /// Serves the given raw JSON-RPC response bodies for `method` in order.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_response_sequence(self, method: &str, responses: &[&str]) -> Self {
                self.response_sequences.lock().unwrap().insert(
                    method.to_string(),
                    responses.iter().map(ToString::to_string).collect(),
                );
                self
            }

            /// Rewrites receipt fixture hashes to the hash requested by each poll.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_receipt_hash_from_request(mut self) -> Self {
                self.receipt_hash_from_request = true;
                self
            }

            /// Answers `eth_sendRawTransaction` with the hash of the submitted bytes when no
            /// canned response is configured, mirroring an honest node.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_send_raw_transaction_echo(mut self) -> Self {
                self.send_raw_echo = true;
                self
            }

            /// Serves the given raw JSON-RPC response body for `eth_call` requests whose calldata
            /// starts with `selector` (hex with `0x` prefix).
            #[must_use]
            pub(crate) fn with_call_response(
                mut self,
                selector: &str,
                response_json: &str,
            ) -> Self {
                self.call_responses
                    .insert(selector.to_string(), response_json.to_string());
                self
            }

            /// Delays responses to `method` by `duration`, simulating an unresponsive node.
            #[cfg(feature = "hypersync")]
            #[must_use]
            pub(crate) fn with_sleep(mut self, method: &str, duration: Duration) -> Self {
                self.sleep_methods.insert(method.to_string(), duration);
                self
            }

            /// Returns every JSON-RPC request body the server received, in order.
            #[must_use]
            pub(crate) fn recorded_requests(&self) -> Vec<Value> {
                self.requests.lock().unwrap().clone()
            }
        }

        async fn handle(State(state): State<MockRpcState>, body: String) -> String {
            let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            state.requests.lock().unwrap().push(request.clone());

            let method = request["method"].as_str().unwrap_or_default();

            if let Some(duration) = state.sleep_methods.get(method) {
                tokio::time::sleep(*duration).await;
            }

            if method == "eth_call" {
                let data = request["params"][0]["data"].as_str().unwrap_or_default();
                let selector_len = "0x".len() + 8;
                if data.len() >= selector_len
                    && let Some(response) = state.call_responses.get(&data[..selector_len])
                {
                    return response.clone();
                }
            }

            let queued_response = state
                .response_sequences
                .lock()
                .unwrap()
                .get_mut(method)
                .and_then(VecDeque::pop_front);

            let parameter = request["params"].get(0).and_then(Value::as_str);
            let queued_parameter_response = parameter.and_then(|parameter| {
                state
                    .parameter_response_sequences
                    .lock()
                    .unwrap()
                    .get_mut(&(method.to_string(), parameter.to_string()))
                    .and_then(VecDeque::pop_front)
            });
            let parameter_response = parameter.and_then(|parameter| {
                state
                    .parameter_responses
                    .get(&(method.to_string(), parameter.to_string()))
            });
            let response = if let Some(response) = queued_response {
                response
            } else if let Some(response) = queued_parameter_response {
                response
            } else if let Some(response) = parameter_response {
                response.clone()
            } else if let Some(response) = state.responses.get(method) {
                response.clone()
            } else if method == "eth_sendRawTransaction" && state.send_raw_echo {
                echo_send_raw_transaction_hash(&request)
            } else {
                method_not_found_response()
            };

            if state.receipt_hash_from_request && method == "eth_getTransactionReceipt" {
                receipt_response_with_requested_hash(response, &request)
            } else {
                response
            }
        }

        fn receipt_response_with_requested_hash(response: String, request: &Value) -> String {
            let Some(requested_hash) = request["params"][0].as_str() else {
                return response;
            };
            let Ok(mut value) = serde_json::from_str::<Value>(&response) else {
                return response;
            };
            let Some(receipt) = value["result"].as_object_mut() else {
                return response;
            };
            receipt.insert(
                "transactionHash".to_string(),
                Value::String(requested_hash.to_string()),
            );
            value.to_string()
        }

        /// Computes the transaction hash for an `eth_sendRawTransaction` request, mirroring
        /// an honest node: the keccak256 digest of the submitted EIP-2718 bytes.
        fn echo_send_raw_transaction_hash(request: &Value) -> String {
            let Some(raw) = request["params"][0].as_str() else {
                return method_not_found_response();
            };
            let stripped = raw.strip_prefix("0x").unwrap_or(raw);
            match nautilus_core::hex::decode(stripped) {
                Ok(bytes) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": alloy::primitives::keccak256(bytes).to_string()
                })
                .to_string(),
                Err(_) => method_not_found_response(),
            }
        }

        fn method_not_found_response() -> String {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {"code": -32601, "message": "method not found"}
            })
            .to_string()
        }

        /// Starts a mock JSON-RPC server on a random localhost port and returns its address.
        pub(crate) async fn start_mock_rpc_server(state: MockRpcState) -> SocketAddr {
            let app = Router::new().route("/", post(handle)).with_state(state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addr
        }
    }

    use mock::{MockRpcState, start_mock_rpc_server};

    const CHAIN_ID_ARBITRUM: &str =
        include_str!("../../test_data/execution/rpc_eth_chain_id_arbitrum.json");
    const GET_CODE_DEPLOYED: &str =
        include_str!("../../test_data/execution/rpc_eth_get_code_deployed.json");
    const GET_CODE_EMPTY: &str =
        include_str!("../../test_data/execution/rpc_eth_get_code_empty.json");
    const TRANSACTION_COUNT: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_count.json");
    const ESTIMATE_GAS: &str = include_str!("../../test_data/execution/rpc_eth_estimate_gas.json");
    const MAX_PRIORITY_FEE: &str =
        include_str!("../../test_data/execution/rpc_eth_max_priority_fee_per_gas.json");
    const BLOCK_BY_NUMBER: &str =
        include_str!("../../test_data/execution/rpc_eth_get_block_by_number.json");
    const RECEIPT_NULL: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_null.json");
    const RECEIPT_SUCCESS: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_success.json");
    const RECEIPT_REVERTED: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_reverted.json");
    const RECEIPT_STATUS_MISSING: &str = include_str!(
        "../../test_data/execution/rpc_eth_get_transaction_receipt_status_missing.json"
    );
    const RECEIPT_STATUS_MALFORMED: &str = include_str!(
        "../../test_data/execution/rpc_eth_get_transaction_receipt_status_malformed.json"
    );
    const RECEIPT_STATUS_OTHER: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_status_other.json");
    const RECEIPT_STATUS_NONCANONICAL: &str = include_str!(
        "../../test_data/execution/rpc_eth_get_transaction_receipt_status_noncanonical.json"
    );
    const SEND_RAW_TRANSACTION: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction.json");
    const SEND_RAW_TRANSACTION_ALREADY_KNOWN: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction_already_known.json");
    const SEND_RAW_TRANSACTION_REJECTED: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction_rejected.json");
    const SEND_RAW_TRANSACTION_NONCE_TOO_LOW: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction_nonce_too_low.json");

    async fn client_for(state: MockRpcState) -> (BlockchainHttpRpcClient, MockRpcState) {
        let addr = start_mock_rpc_server(state.clone()).await;
        (
            BlockchainHttpRpcClient::new(format!("http://{addr}"), None, None),
            state,
        )
    }

    #[rstest]
    fn debug_redacts_http_rpc_url() {
        const USERINFO_SECRET: &str = "http-client-userinfo-secret";
        const PATH_SECRET: &str = "http-client-path-secret";
        const QUERY_SECRET: &str = "http-client-query-secret";
        let http_rpc_url = format!(
            "https://rpc-user:{USERINFO_SECRET}@rpc.example.com/{PATH_SECRET}?api_key={QUERY_SECRET}"
        );
        let client = BlockchainHttpRpcClient::new(http_rpc_url.clone(), None, None);

        let debug = format!("{client:?}");

        assert!(debug.contains("http_rpc_url: \"<redacted>\""));
        assert!(!debug.contains(USERINFO_SECRET));
        assert!(!debug.contains(PATH_SECRET));
        assert!(!debug.contains(QUERY_SECRET));
        assert!(!debug.contains(&http_rpc_url));
    }

    #[tokio::test]
    async fn chain_id_parses_hex_quantity() {
        let (client, _) =
            client_for(MockRpcState::default().with_response("eth_chainId", CHAIN_ID_ARBITRUM))
                .await;

        assert_eq!(client.chain_id().await.unwrap(), 42161);
    }

    #[tokio::test]
    async fn chain_id_error_is_sanitized_to_method_and_code() {
        let (client, _) = client_for(MockRpcState::default()).await;

        let error = client.chain_id().await.unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("eth_chainId RPC error -32601"),
            "was: {message}"
        );
        assert!(!message.contains("method not found"), "was: {message}");
    }

    #[tokio::test]
    async fn get_code_returns_deployed_bytecode() {
        let (client, _) =
            client_for(MockRpcState::default().with_response("eth_getCode", GET_CODE_DEPLOYED))
                .await;

        let code = client
            .get_code(&address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"))
            .await
            .unwrap();

        assert!(!code.is_empty());
        assert!(code.starts_with(&[0x60, 0x80]));
    }

    #[tokio::test]
    async fn get_code_returns_empty_for_eoa() {
        let (client, _) =
            client_for(MockRpcState::default().with_response("eth_getCode", GET_CODE_EMPTY)).await;

        let code = client
            .get_code(&address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"))
            .await
            .unwrap();

        assert!(code.is_empty());
    }

    #[tokio::test]
    async fn transaction_count_uses_pending_tag() {
        let (client, state) = client_for(
            MockRpcState::default().with_response("eth_getTransactionCount", TRANSACTION_COUNT),
        )
        .await;

        let nonce = client
            .get_transaction_count_pending(&address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"))
            .await
            .unwrap();

        assert_eq!(nonce, 7);
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "eth_getTransactionCount");
        assert_eq!(requests[0]["params"][1], "pending");
    }

    #[tokio::test]
    async fn estimate_gas_sends_call_object() {
        let (client, state) =
            client_for(MockRpcState::default().with_response("eth_estimateGas", ESTIMATE_GAS))
                .await;

        let gas = client
            .estimate_gas(
                &address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                &address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000_000_000_000_000_000u64),
                &[0xd0, 0xe3, 0x0d, 0xb0],
            )
            .await
            .unwrap();

        assert_eq!(gas, 65_000);
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        let call = &requests[0]["params"][0];
        assert_eq!(call["from"], "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
        assert_eq!(call["to"], "0x82af49447d8a07e3bd95bd0d56f35241523fbab1");
        assert_eq!(call["value"], "0xde0b6b3a7640000");
        assert_eq!(call["data"], "0xd0e30db0");
    }

    #[tokio::test]
    async fn max_priority_fee_parses_hex_quantity() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_maxPriorityFeePerGas", MAX_PRIORITY_FEE),
        )
        .await;

        assert_eq!(client.max_priority_fee_per_gas().await.unwrap(), 10_000_000);
    }

    #[tokio::test]
    async fn latest_block_parses_number_timestamp_and_base_fee() {
        let (client, state) = client_for(
            MockRpcState::default().with_response("eth_getBlockByNumber", BLOCK_BY_NUMBER),
        )
        .await;

        let block = client.latest_block().await.unwrap();

        assert_eq!(block.number, 30_346_560);
        assert_eq!(
            block.hash,
            b256!("1111111111111111111111111111111111111111111111111111111111111111")
        );
        assert_eq!(block.timestamp, 1_761_888_800);
        assert_eq!(block.base_fee_per_gas, Some(100_000_000));
        assert!(block.transactions.is_empty());
        let requests = state.recorded_requests();
        assert_eq!(requests[0]["params"][0], "latest");
        assert_eq!(requests[0]["params"][1], false);
    }

    #[tokio::test]
    async fn finalized_block_uses_finalized_tag_without_transactions() {
        let (client, state) = client_for(
            MockRpcState::default().with_response("eth_getBlockByNumber", BLOCK_BY_NUMBER),
        )
        .await;

        let block = client.finalized_block().await.unwrap();

        assert_eq!(block.number, 30_346_560);
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["params"],
            serde_json::json!(["finalized", false])
        );
    }

    #[tokio::test]
    async fn transaction_receipt_maps_null_result_to_none() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
        )
        .await;

        let receipt = client
            .get_transaction_receipt(&b256!(
                "9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"
            ))
            .await
            .unwrap();

        assert!(receipt.is_none());
    }

    #[tokio::test]
    async fn transaction_receipt_parses_success_fields() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS),
        )
        .await;

        let receipt = client
            .get_transaction_receipt(&b256!(
                "9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"
            ))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            receipt.transaction_hash,
            b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a")
        );
        assert_eq!(receipt.block_number, 30_346_561);
        assert_eq!(
            receipt.block_hash,
            b256!("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert_eq!(receipt.gas_used, 50_112);
        assert_eq!(receipt.effective_gas_price, U256::from(100_000_000_u64));
        assert_eq!(receipt.transaction_index, 2);
        assert!(receipt.status);
        assert!(receipt.logs.is_empty());
    }

    #[tokio::test]
    async fn transaction_receipt_parses_reverted_fields() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_REVERTED),
        )
        .await;

        let receipt = client
            .get_transaction_receipt(&b256!(
                "9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"
            ))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            receipt.transaction_hash,
            b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a")
        );
        assert_eq!(receipt.block_number, 30_346_561);
        assert_eq!(
            receipt.block_hash,
            b256!("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert_eq!(receipt.gas_used, 50_112);
        assert_eq!(receipt.effective_gas_price, U256::from(100_000_000_u64));
        assert_eq!(receipt.transaction_index, 2);
        assert!(!receipt.status);
        assert!(receipt.logs.is_empty());
    }

    #[tokio::test]
    async fn transaction_receipt_rejects_mismatched_hash() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS),
        )
        .await;

        let error = client
            .get_transaction_receipt(&B256::ZERO)
            .await
            .expect_err("a receipt for another transaction should fail closed");

        assert_eq!(
            error.to_string(),
            "eth_getTransactionReceipt returned a receipt with a mismatched transaction hash"
        );
    }

    #[rstest]
    #[case::missing(RECEIPT_STATUS_MISSING)]
    #[case::malformed(RECEIPT_STATUS_MALFORMED)]
    #[case::other(RECEIPT_STATUS_OTHER)]
    #[case::noncanonical(RECEIPT_STATUS_NONCANONICAL)]
    #[tokio::test]
    async fn transaction_receipt_rejects_invalid_status(#[case] response: &str) {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_getTransactionReceipt", response),
        )
        .await;

        let error = client
            .get_transaction_receipt(&b256!(
                "9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"
            ))
            .await
            .expect_err("an invalid receipt status should fail closed");

        assert_eq!(
            error.to_string(),
            "Failed to parse eth_getTransactionReceipt response"
        );
    }

    #[tokio::test]
    async fn send_raw_transaction_returns_node_hash() {
        let (client, _) = client_for(
            MockRpcState::default().with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION),
        )
        .await;

        let hash = client
            .send_raw_transaction(
                &[0x02, 0xf8],
                &b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"),
            )
            .await
            .unwrap();

        assert_eq!(
            hash,
            b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a")
        );
    }

    #[tokio::test]
    async fn send_raw_transaction_treats_already_known_as_acceptance() {
        let (client, _) = client_for(
            MockRpcState::default()
                .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_ALREADY_KNOWN),
        )
        .await;

        let expected = b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a");
        let hash = client
            .send_raw_transaction(&[0x02, 0xf8], &expected)
            .await
            .unwrap();

        assert_eq!(hash, expected);
    }

    #[tokio::test]
    async fn send_raw_transaction_rejection_is_sanitized_to_code() {
        let (client, _) = client_for(
            MockRpcState::default()
                .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED),
        )
        .await;

        let error = client
            .send_raw_transaction(
                &[0x02, 0xf8],
                &b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"),
            )
            .await
            .unwrap_err();

        match error {
            BroadcastError::Rejected { code } => assert_eq!(code, -32000),
            other => panic!("Expected BroadcastError::Rejected, was {other}"),
        }
        let message = error.to_string();
        assert!(!message.contains("insufficient funds"), "was: {message}");
    }

    #[tokio::test]
    async fn send_raw_transaction_treats_nonce_too_low_as_ambiguous() {
        let (client, _) = client_for(
            MockRpcState::default()
                .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_NONCE_TOO_LOW),
        )
        .await;

        let error = client
            .send_raw_transaction(
                &[0x02, 0xf8],
                &b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a"),
            )
            .await
            .unwrap_err();

        let BroadcastError::Failed(message) = error else {
            panic!("Expected BroadcastError::Failed, was {error}");
        };
        assert_eq!(message, "node RPC error -32000 reported a consumed nonce");
        assert!(!message.contains("0xf39Fd6e51"), "was: {message}");
    }

    #[rstest]
    fn broadcast_error_classification_sanitizes_transport_failure() {
        const SECRET: &str = "https://rpc.example.com/private-api-key";
        let timeout = HttpClientError::TimeoutError("timed out".to_string());
        let transport = HttpClientError::Error(SECRET.to_string());

        assert!(matches!(
            classify_broadcast_transport_error(&timeout),
            BroadcastError::TimeoutAfterSend
        ));
        let classified = classify_broadcast_transport_error(&transport);
        assert_eq!(
            classified.to_string(),
            "Broadcast failed ambiguously: transport error"
        );
        assert!(!classified.to_string().contains(SECRET));
    }
}

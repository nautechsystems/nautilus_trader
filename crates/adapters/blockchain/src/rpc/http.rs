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

use std::{collections::HashMap, num::NonZeroU32};

use alloy::primitives::{Address, U256};
use bytes::Bytes;
use nautilus_model::defi::{
    Block, Transaction, TransactionReceipt,
    hex::from_str_hex_to_u64,
    rpc::{RpcError, RpcLog, RpcNodeHttpResponse},
};
use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
};
use serde::de::DeserializeOwned;

use crate::rpc::error::BlockchainRpcClientError;

const RETRY_AFTER_HEADER: &str = "retry-after";
const QUOTA_EXEC_PREFIX: &str = "rpc:exec";
const QUOTA_DATA_PREFIX: &str = "rpc:data";

const PROVIDER_RANGE_ERROR_HINTS: [&str; 10] = [
    "query returned more than",
    "too many results",
    "response size exceeded",
    "log response size exceeded",
    "please reduce",
    "result window is too large",
    "max block range",
    "block range is too wide",
    "request exceeds",
    "please narrow your query",
];

#[derive(Debug)]
struct RpcHttpResponse {
    status_code: u16,
    retry_after: Option<String>,
    body: Bytes,
}

#[derive(Debug)]
struct ParsedRpcError {
    code: i32,
    message: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
enum RpcQuotaScope {
    Exec,
    Data,
}

impl RpcQuotaScope {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Exec => QUOTA_EXEC_PREFIX,
            Self::Data => QUOTA_DATA_PREFIX,
        }
    }
}

/// Client for making HTTP-based RPC requests to blockchain nodes.
#[derive(Debug)]
pub struct BlockchainHttpRpcClient {
    /// The HTTP URL for the blockchain node's RPC endpoint.
    http_rpc_url: String,
    /// The HTTP client for making RPC http-based requests.
    http_client: HttpClient,
}

impl BlockchainHttpRpcClient {
    /// Creates a new HTTP RPC client with the given endpoint URL and optional rate limit.
    #[must_use]
    pub fn new(http_rpc_url: String, rpc_request_per_second: Option<u32>) -> Self {
        let default_quota =
            rpc_request_per_second.and_then(|rps| Quota::per_second(NonZeroU32::new(rps)?));

        // Capture Retry-After so callers can apply bounded backoff on provider throttling.
        let http_client = HttpClient::new(
            HashMap::new(),
            vec![RETRY_AFTER_HEADER.to_string()],
            Vec::new(),
            default_quota,
            None, // timeout_secs
            None, // proxy_url
        )
        .expect("Failed to create HTTP client");

        Self {
            http_rpc_url,
            http_client,
        }
    }

    /// Generic method that sends a JSON-RPC request and returns the raw HTTP response context.
    async fn send_rpc_request(
        &self,
        rpc_request: serde_json::Value,
        quota_scope: RpcQuotaScope,
    ) -> Result<RpcHttpResponse, BlockchainRpcClientError> {
        let method_name = rpc_request
            .get("method")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                BlockchainRpcClientError::InvalidParameters(
                    "RPC request missing string 'method' field".to_string(),
                )
            })?;

        let body_bytes = serde_json::to_vec(&rpc_request).map_err(|e| {
            BlockchainRpcClientError::ClientError(format!("Failed to serialize request: {e}"))
        })?;

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let quota_keys = build_quota_keys(quota_scope, method_name);
        match self
            .http_client
            .request(
                Method::POST,
                self.http_rpc_url.clone(),
                None,
                Some(headers),
                Some(body_bytes),
                None,
                Some(quota_keys),
            )
            .await
        {
            Ok(response) => Ok(RpcHttpResponse {
                status_code: response.status.as_u16(),
                retry_after: response.headers.get(RETRY_AFTER_HEADER).cloned(),
                body: response.body,
            }),
            Err(e) => Err(BlockchainRpcClientError::ClientError(e.to_string())),
        }
    }

    /// Executes an Ethereum JSON-RPC call and deserializes the response into the specified type T.
    pub async fn execute_rpc_call<T: DeserializeOwned>(
        &self,
        rpc_request: serde_json::Value,
    ) -> anyhow::Result<T> {
        self.execute_rpc_call_with_scope(rpc_request, RpcQuotaScope::Data)
            .await
    }

    async fn execute_rpc_call_with_scope<T: DeserializeOwned>(
        &self,
        rpc_request: serde_json::Value,
        quota_scope: RpcQuotaScope,
    ) -> anyhow::Result<T> {
        let http_response = self
            .send_rpc_request(rpc_request, quota_scope)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to execute RPC request over HTTP transport: {e}")
            })?;

        // Surface 429 + Retry-After explicitly so callers can apply retry strategy.
        if http_response.status_code == 429 {
            let rpc_error = parse_rpc_error(&http_response.body);
            let detail = format_http_error(
                http_response.status_code,
                http_response.retry_after.as_deref(),
                rpc_error,
                &http_response.body,
            );
            anyhow::bail!(detail);
        }

        if !(200..300).contains(&http_response.status_code) {
            let rpc_error = parse_rpc_error(&http_response.body);
            let detail = format_http_error(
                http_response.status_code,
                http_response.retry_after.as_deref(),
                rpc_error,
                &http_response.body,
            );
            anyhow::bail!(detail);
        }

        match serde_json::from_slice::<RpcNodeHttpResponse<T>>(http_response.body.as_ref()) {
            Ok(parsed) => {
                // Non-standard provider errors can place code/message at the top-level.
                if let (Some(code), Some(message)) = (parsed.code, parsed.message.as_deref()) {
                    anyhow::bail!("RPC provider error code={code} message={message}");
                }

                if let Some(error) = parsed.error {
                    let data = error
                        .data
                        .map_or_else(|| "null".to_string(), |value| value.to_string());
                    anyhow::bail!(
                        "RPC error code={} message={} data={}",
                        error.code,
                        error.message,
                        data
                    );
                }

                if let Some(result) = parsed.result {
                    Ok(result)
                } else if parsed.error.is_none()
                    && parsed.code.is_none()
                    && parsed.message.is_none()
                    && has_explicit_null_result(http_response.body.as_ref())
                {
                    serde_json::from_value::<T>(serde_json::Value::Null)
                        .map_err(|e| anyhow::anyhow!("Failed to decode null RPC result: {e}"))
                } else {
                    Err(anyhow::anyhow!(
                        "RPC response missing both result and error fields"
                    ))
                }
            }
            Err(e) => {
                let preview = body_preview(http_response.body.as_ref());
                Err(anyhow::anyhow!(
                    "Failed to parse RPC response: {e}; body={preview}"
                ))
            }
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

    /// Retrieves the balance of the specified Ethereum address at the given block.
    pub async fn get_balance(&self, address: &Address, block: Option<u64>) -> anyhow::Result<U256> {
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
        let hex_string: String = self.execute_rpc_call(request).await?;

        parse_hex_u256(&hex_string).map_err(|e| {
            anyhow::anyhow!("Failed to parse eth_getBalance value '{hex_string}': {e}")
        })
    }

    /// Returns the transaction count (nonce) for an account.
    pub async fn get_transaction_count(
        &self,
        address: &Address,
        block_tag: Option<&str>,
    ) -> anyhow::Result<u64> {
        let block_tag = block_tag.unwrap_or("pending");
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionCount",
            "params": [address, block_tag]
        });

        let result: String = self
            .execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await?;
        from_str_hex_to_u64(result.as_str())
            .map_err(|e| anyhow::anyhow!("Failed to parse nonce '{result}': {e}"))
    }

    /// Estimates gas for a transaction call object.
    pub async fn estimate_gas(
        &self,
        call_obj: serde_json::Value,
        block_tag: Option<&str>,
    ) -> anyhow::Result<U256> {
        let block_tag = block_tag.unwrap_or("latest");
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_estimateGas",
            "params": [call_obj, block_tag]
        });

        let result: String = self
            .execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await?;
        parse_hex_u256(result.as_str())
            .map_err(|e| anyhow::anyhow!("Failed to parse estimate gas value '{result}': {e}"))
    }

    /// Broadcasts a pre-signed raw transaction.
    pub async fn send_raw_transaction(&self, raw_tx_hex: &str) -> anyhow::Result<String> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendRawTransaction",
            "params": [raw_tx_hex]
        });

        self.execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await
    }

    /// Fetches a transaction by hash (`None` when not found by node).
    pub async fn get_transaction_by_hash(
        &self,
        tx_hash: &str,
    ) -> anyhow::Result<Option<Transaction>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionByHash",
            "params": [tx_hash]
        });

        self.execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await
    }

    /// Fetches a mined transaction receipt by hash (`None` when pending/not found).
    pub async fn get_transaction_receipt(
        &self,
        tx_hash: &str,
    ) -> anyhow::Result<Option<TransactionReceipt>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash]
        });

        self.execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await
    }

    /// Fetches a block by number, or latest when `None`.
    pub async fn get_block_by_number(
        &self,
        number_or_latest: Option<u64>,
    ) -> anyhow::Result<Option<Block>> {
        let block_tag = if let Some(number) = number_or_latest {
            format!("0x{number:x}")
        } else {
            "latest".to_string()
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBlockByNumber",
            "params": [block_tag, false]
        });

        self.execute_rpc_call_with_scope(request, RpcQuotaScope::Data)
            .await
    }

    /// Returns deployed bytecode for an address (`0x` for EOA/non-contract).
    pub async fn get_code(
        &self,
        address: &Address,
        block_tag: Option<&str>,
    ) -> anyhow::Result<String> {
        let block_tag = block_tag.unwrap_or("latest");
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getCode",
            "params": [address, block_tag]
        });

        self.execute_rpc_call_with_scope(request, RpcQuotaScope::Exec)
            .await
    }

    /// Returns node chain ID as integer.
    pub async fn chain_id(&self) -> anyhow::Result<u64> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_chainId",
            "params": []
        });

        let result: String = self
            .execute_rpc_call_with_scope(request, RpcQuotaScope::Data)
            .await?;
        from_str_hex_to_u64(result.as_str())
            .map_err(|e| anyhow::anyhow!("Failed to parse chain id '{result}': {e}"))
    }

    /// Retrieves logs matching the given filter criteria.
    ///
    /// Uses bounded range splitting when providers reject large backfill windows.
    pub async fn get_logs(
        &self,
        address: Option<&Address>,
        topics: Option<Vec<Option<String>>>,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<Vec<RpcLog>> {
        if from_block > to_block {
            anyhow::bail!("Invalid block range: from_block ({from_block}) > to_block ({to_block})");
        }

        self.get_logs_with_backfill_split(address, topics.as_ref(), from_block, to_block)
            .await
    }

    async fn get_logs_with_backfill_split(
        &self,
        address: Option<&Address>,
        topics: Option<&Vec<Option<String>>>,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<Vec<RpcLog>> {
        let mut all_logs = Vec::new();
        let mut pending_ranges = vec![(from_block, to_block)];

        while let Some((current_from, current_to)) = pending_ranges.pop() {
            let request = self.build_get_logs_request(address, topics, current_from, current_to);
            match self
                .execute_rpc_call_with_scope::<Vec<RpcLog>>(request, RpcQuotaScope::Data)
                .await
            {
                Ok(mut logs) => all_logs.append(&mut logs),
                Err(error) => {
                    if current_from < current_to
                        && is_provider_range_error(error.to_string().as_str())
                    {
                        let midpoint = current_from + ((current_to - current_from) / 2);
                        // Push right first so left sub-range is processed first (LIFO).
                        pending_ranges.push((midpoint + 1, current_to));
                        pending_ranges.push((current_from, midpoint));
                    } else {
                        return Err(error);
                    }
                }
            }
        }

        Ok(all_logs)
    }

    fn build_get_logs_request(
        &self,
        address: Option<&Address>,
        topics: Option<&Vec<Option<String>>>,
        from_block: u64,
        to_block: u64,
    ) -> serde_json::Value {
        let mut filter = serde_json::Map::new();
        filter.insert(
            "fromBlock".to_string(),
            serde_json::json!(format!("0x{from_block:x}")),
        );
        filter.insert(
            "toBlock".to_string(),
            serde_json::json!(format!("0x{to_block:x}")),
        );

        if let Some(addr) = address {
            filter.insert("address".to_string(), serde_json::json!(addr.to_string()));
        }

        if let Some(topics) = topics {
            filter.insert("topics".to_string(), serde_json::json!(topics));
        }

        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getLogs",
            "params": [filter]
        })
    }
}

fn build_quota_keys(scope: RpcQuotaScope, method_name: &str) -> Vec<String> {
    let prefix = scope.prefix();
    vec![format!("{prefix}:{method_name}"), prefix.to_string()]
}

fn parse_rpc_error(body: &[u8]) -> Option<ParsedRpcError> {
    let parsed = serde_json::from_slice::<RpcNodeHttpResponse<serde_json::Value>>(body).ok()?;

    if let Some(RpcError {
        code,
        message,
        data,
    }) = parsed.error
    {
        return Some(ParsedRpcError {
            code,
            message,
            data,
        });
    }

    if let (Some(code), Some(message)) = (parsed.code, parsed.message) {
        return Some(ParsedRpcError {
            code,
            message,
            data: None,
        });
    }

    None
}

fn format_http_error(
    status_code: u16,
    retry_after: Option<&str>,
    rpc_error: Option<ParsedRpcError>,
    body: &[u8],
) -> String {
    let mut details = vec![format!("HTTP {status_code} RPC request failed")];

    if let Some(retry_after) = retry_after {
        details.push(format!("retry_after={retry_after}"));
    }

    if let Some(error) = rpc_error {
        details.push(format!("rpc_code={}", error.code));
        details.push(format!("rpc_message={}", error.message));
        if let Some(data) = error.data {
            details.push(format!("rpc_data={data}"));
        }
    } else {
        details.push(format!("body={}", body_preview(body)));
    }

    details.join(" ")
}

fn body_preview(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.len() > 500 {
        format!(
            "{}... (truncated, {} bytes total)",
            &text[..500],
            text.len()
        )
    } else {
        text.to_string()
    }
}

fn has_explicit_null_result(body: &[u8]) -> bool {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => value.get("result").is_some_and(serde_json::Value::is_null),
        Err(_) => false,
    }
}

fn is_provider_range_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("http 429") || lowered.contains("too many requests") {
        return false;
    }
    PROVIDER_RANGE_ERROR_HINTS
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn parse_hex_u256(value: &str) -> anyhow::Result<U256> {
    let without_prefix = if value.starts_with("0x") || value.starts_with("0X") {
        &value[2..]
    } else {
        value
    };

    U256::from_str_radix(without_prefix, 16)
        .map_err(|e| anyhow::anyhow!("failed to parse hex quantity '{value}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_quota_keys_exec_scope() {
        let keys = build_quota_keys(RpcQuotaScope::Exec, "eth_sendRawTransaction");
        assert_eq!(
            keys,
            vec![
                "rpc:exec:eth_sendRawTransaction".to_string(),
                "rpc:exec".to_string()
            ]
        );
    }

    #[test]
    fn test_build_quota_keys_data_scope() {
        let keys = build_quota_keys(RpcQuotaScope::Data, "eth_getLogs");
        assert_eq!(keys, vec!["rpc:data:eth_getLogs", "rpc:data"]);
    }

    #[test]
    fn test_provider_range_error_detection() {
        assert!(is_provider_range_error(
            "RPC error code=-32005 message=query returned more than 10000 results"
        ));
        assert!(is_provider_range_error(
            "request exceeds max block range for eth_getLogs"
        ));
        assert!(!is_provider_range_error(
            "HTTP 429 RPC request failed rpc_code=-32005 rpc_message=too many requests"
        ));
        assert!(!is_provider_range_error("execution reverted"));
    }
}

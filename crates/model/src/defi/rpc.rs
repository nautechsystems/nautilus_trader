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

use serde::{Deserialize, de::DeserializeOwned};

/// A response structure received from a WebSocket JSON-RPC blockchain node subscription.
#[derive(Debug, Deserialize)]
pub struct RpcNodeWssResponse<T>
where
    T: DeserializeOwned,
{
    /// JSON-RPC version identifier.
    pub jsonrpc: String,
    /// Name of the RPC method that was called.
    pub method: String,
    /// Parameters containing subscription information and the deserialized result.
    #[serde(bound(deserialize = ""))]
    pub params: RpcNodeSubscriptionResponse<T>,
}

/// Container for subscription data within an RPC response, holding the subscription ID and the deserialized result.
#[derive(Debug, Deserialize)]
pub struct RpcNodeSubscriptionResponse<T>
where
    T: DeserializeOwned,
{
    /// ID of the subscription associated with the RPC response.
    pub subscription: String,
    /// Deserialized result.
    #[serde(bound(deserialize = ""))]
    pub result: T,
}

/// A response structure received from an HTTP JSON-RPC blockchain node request.
#[derive(Debug, Deserialize)]
pub struct RpcNodeHttpResponse<T>
where
    T: DeserializeOwned,
{
    /// JSON-RPC version identifier (optional for non-standard error responses like rate limits).
    pub jsonrpc: Option<String>,
    /// Request identifier returned by the server (optional for non-standard error responses).
    pub id: Option<u64>,
    /// Deserialized result.
    #[serde(bound(deserialize = ""))]
    pub result: Option<T>,
    /// Error information if the request failed.
    pub error: Option<RpcError>,
    /// Error code (for non-standard rate limit responses).
    pub code: Option<i32>,
    /// Error message (for non-standard rate limit responses).
    pub message: Option<String>,
}

/// JSON-RPC error structure.
#[derive(Debug, Deserialize)]
pub struct RpcError {
    /// Error code.
    pub code: i32,
    /// Error message.
    pub message: String,
}

/// Log entry in standard Ethereum JSON-RPC format.
///
/// This struct represents an event log returned by the `eth_getLogs` RPC method.
/// Field names use camelCase to match the Ethereum JSON-RPC specification.
///
/// Note: `log_index`, `transaction_index`, `transaction_hash`, `block_hash`, and
/// `block_number` can be null for pending logs, but are always present for confirmed logs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcLog {
    /// Whether the log was removed due to chain reorganization.
    pub removed: bool,
    /// Index position of the log in the block (hex string).
    pub log_index: Option<String>,
    /// Index position of the transaction in the block (hex string).
    pub transaction_index: Option<String>,
    /// Hash of the transaction that generated this log.
    pub transaction_hash: Option<String>,
    /// Hash of the block containing this log.
    pub block_hash: Option<String>,
    /// Block number containing this log (hex string).
    pub block_number: Option<String>,
    /// Address of the contract that emitted the event.
    pub address: String,
    /// Non-indexed event parameters (hex-encoded bytes).
    pub data: String,
    /// Indexed event parameters.
    pub topics: Vec<String>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_rpc_log_deserialize_pool_created_block_185() {
        let json = r#"{
            "removed": false,
            "logIndex": "0x0",
            "transactionIndex": "0x0",
            "transactionHash": "0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7",
            "blockHash": "0xd371b6c7b04ec33d6470f067a82e87d7b294b952bea7a46d7b939b4c7addc275",
            "blockNumber": "0xb9",
            "address": "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            "data": "0x000000000000000000000000000000000000000000000000000000000000003c000000000000000000000000b9fc136980d98c034a529aadbd5651c087365d5f",
            "topics": [
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                "0x0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4",
                "0x000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb",
                "0x0000000000000000000000000000000000000000000000000000000000000bb8"
            ]
        }"#;

        let log: RpcLog = serde_json::from_str(json).expect("Failed to deserialize RpcLog");

        assert!(!log.removed);
        assert_eq!(log.log_index, Some("0x0".to_string()));
        assert_eq!(log.transaction_index, Some("0x0".to_string()));
        assert_eq!(
            log.transaction_hash,
            Some("0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7".to_string())
        );
        assert_eq!(
            log.block_hash,
            Some("0xd371b6c7b04ec33d6470f067a82e87d7b294b952bea7a46d7b939b4c7addc275".to_string())
        );
        assert_eq!(log.block_number, Some("0xb9".to_string()));
        assert_eq!(log.address, "0x1f98431c8ad98523631ae4a59f267346ea31f984");
        assert_eq!(
            log.data,
            "0x000000000000000000000000000000000000000000000000000000000000003c000000000000000000000000b9fc136980d98c034a529aadbd5651c087365d5f"
        );
        assert_eq!(log.topics.len(), 4);
        assert_eq!(
            log.topics[0],
            "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118"
        );
        assert_eq!(
            log.topics[1],
            "0x0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4"
        );
        assert_eq!(
            log.topics[2],
            "0x000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb"
        );
        assert_eq!(
            log.topics[3],
            "0x0000000000000000000000000000000000000000000000000000000000000bb8"
        );
    }

    #[rstest]
    fn test_rpc_log_deserialize_pool_created_block_540() {
        let json = r#"{
            "removed": false,
            "logIndex": "0x0",
            "transactionIndex": "0x0",
            "transactionHash": "0x0810b3488eba9b0264d3544b4548b70d0c8667e05ac4a5d90686f4a9f70509df",
            "blockHash": "0x59bb10cdfd586affc6aa4a0b12f0662ec04599a1a459ac5b33129bc2c8705ccd",
            "blockNumber": "0x21c",
            "address": "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            "data": "0x000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000007d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4",
            "topics": [
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                "0x0000000000000000000000008dd7c686b11c115ffaba245cbfc418b371087f68",
                "0x000000000000000000000000be5381d826375492e55e05039a541eb2cb978e76",
                "0x00000000000000000000000000000000000000000000000000000000000001f4"
            ]
        }"#;

        let log: RpcLog = serde_json::from_str(json).expect("Failed to deserialize RpcLog");

        assert!(!log.removed);
        assert_eq!(log.log_index, Some("0x0".to_string()));
        assert_eq!(log.transaction_index, Some("0x0".to_string()));
        assert_eq!(
            log.transaction_hash,
            Some("0x0810b3488eba9b0264d3544b4548b70d0c8667e05ac4a5d90686f4a9f70509df".to_string())
        );
        assert_eq!(
            log.block_hash,
            Some("0x59bb10cdfd586affc6aa4a0b12f0662ec04599a1a459ac5b33129bc2c8705ccd".to_string())
        );
        assert_eq!(log.block_number, Some("0x21c".to_string()));
        assert_eq!(log.address, "0x1f98431c8ad98523631ae4a59f267346ea31f984");
        assert_eq!(
            log.data,
            "0x000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000007d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4"
        );
        assert_eq!(log.topics.len(), 4);
        assert_eq!(
            log.topics[0],
            "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118"
        );
        assert_eq!(
            log.topics[1],
            "0x0000000000000000000000008dd7c686b11c115ffaba245cbfc418b371087f68"
        );
        assert_eq!(
            log.topics[2],
            "0x000000000000000000000000be5381d826375492e55e05039a541eb2cb978e76"
        );
        assert_eq!(
            log.topics[3],
            "0x00000000000000000000000000000000000000000000000000000000000001f4"
        );
    }
}

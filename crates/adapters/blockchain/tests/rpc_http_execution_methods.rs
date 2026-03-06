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

use std::str::FromStr;

use alloy::primitives::{Address, U256};
use axum::http::StatusCode;
use nautilus_blockchain::rpc::http::BlockchainHttpRpcClient;
use serde_json::{Value, json};

mod common;

use common::{MockRpcResponse, MockRpcState, start_mock_rpc_server};

fn rpc_result(result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": result,
    })
}

fn rpc_error(code: i32, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": code,
            "message": message,
            "data": data,
        }
    })
}

fn parse_address(value: &str) -> Address {
    Address::from_str(value).expect("valid address")
}

async fn new_client(state: MockRpcState) -> (BlockchainHttpRpcClient, MockRpcState) {
    let addr = start_mock_rpc_server(state.clone()).await;
    let client = BlockchainHttpRpcClient::new(format!("http://{addr}/"), None);
    (client, state)
}

#[tokio::test]
async fn test_get_transaction_count_uses_pending_tag() {
    let state = MockRpcState::new();
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x2a")))
        .await;
    let (client, state) = new_client(state).await;

    let address = parse_address("0x1111111111111111111111111111111111111111");
    let nonce = client
        .get_transaction_count(&address, None)
        .await
        .expect("nonce response");

    assert_eq!(nonce, 42);
    assert_eq!(state.method_count("eth_getTransactionCount").await, 1);

    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_getTransactionCount");
    assert_eq!(request["params"][1], "pending");
}

#[tokio::test]
async fn test_estimate_gas_builds_expected_rpc_payload() {
    let state = MockRpcState::new();
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    let (client, state) = new_client(state).await;

    let call_obj = json!({
        "from": "0x1111111111111111111111111111111111111111",
        "to": "0x2222222222222222222222222222222222222222",
        "data": "0xdeadbeef"
    });

    let gas = client
        .estimate_gas(call_obj.clone(), Some("latest"))
        .await
        .expect("estimate gas response");

    assert_eq!(gas, U256::from(21_000u64));
    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_estimateGas");
    assert_eq!(request["params"][0], call_obj);
    assert_eq!(request["params"][1], "latest");
}

#[tokio::test]
async fn test_send_raw_transaction_returns_tx_hash() {
    let state = MockRpcState::new();
    let tx_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash)))
        .await;
    let (client, state) = new_client(state).await;

    let raw_tx = "0x02f8700180843b9aca0084b2d05e0082520894aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa880de0b6b3a764000080c001a0b54d66c0682d";
    let result = client
        .send_raw_transaction(raw_tx)
        .await
        .expect("send raw transaction response");

    assert_eq!(result, tx_hash);

    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_sendRawTransaction");
    assert_eq!(request["params"][0], raw_tx);
}

#[tokio::test]
async fn test_send_raw_transaction_error_parses_code_message_and_data() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_sendRawTransaction",
            rpc_error(
                -32000,
                "execution reverted",
                json!({"reason": "INSUFFICIENT_OUTPUT_AMOUNT"}),
            ),
        )
        .await;
    let (client, _state) = new_client(state).await;

    let err = client
        .send_raw_transaction("0x02deadbeef")
        .await
        .expect_err("expected rpc error");
    let err_text = err.to_string();

    assert!(err_text.contains("-32000"));
    assert!(err_text.contains("execution reverted"));
    assert!(err_text.contains("INSUFFICIENT_OUTPUT_AMOUNT"));
}

#[tokio::test]
async fn test_get_transaction_by_hash_parses_from_to_input_and_fee_fields() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_getTransactionByHash",
            rpc_result(json!({
                "chainId": "0x1",
                "hash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "gas": "0xe4e1c0",
                "gasPrice": "0x536bc8dc",
                "hash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "input": "0xabcdef",
                "maxFeePerGas": "0x559d2c91",
                "maxPriorityFeePerGas": "0x3b9aca00",
                "nonce": "0x4c5",
                "transactionIndex": "0x4a",
                "value": "0x0"
            })),
        )
        .await;
    let (client, state) = new_client(state).await;

    let tx = client
        .get_transaction_by_hash(
            "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
        )
        .await
        .expect("tx response")
        .expect("tx should be present");

    assert_eq!(tx.nonce, Some(0x4c5));
    assert_eq!(tx.input, Some("0xabcdef".to_string()));
    assert_eq!(tx.max_fee_per_gas, Some(U256::from(1_436_363_921u64)));
    assert_eq!(
        tx.max_priority_fee_per_gas,
        Some(U256::from(1_000_000_000u64))
    );
    assert_eq!(
        tx.to,
        Some(parse_address("0x8c0bfc04ada21fd496c55b8c50331f904306f564"))
    );

    let request_log = state.request_log().await;
    assert_eq!(request_log[0]["method"], "eth_getTransactionByHash");
}

#[tokio::test]
async fn test_get_transaction_receipt_parses_status_logs_and_gas_fields() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            rpc_result(json!({
                "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "cumulativeGasUsed": "0x992832",
                "gasUsed": "0x2dc6c",
                "effectiveGasPrice": "0x559d2c91",
                "status": "0x1",
                "logs": [{
                    "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                    "topics": [
                        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                    ],
                    "data": "0x00000000000000000000000000000000000000000000000000000000000003e8",
                    "logIndex": "0x2",
                    "transactionIndex": "0x4a",
                    "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                    "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                    "blockNumber": "0x154a1d6",
                    "removed": false
                }]
            })),
        )
        .await;
    let (client, state) = new_client(state).await;

    let receipt = client
        .get_transaction_receipt(
            "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
        )
        .await
        .expect("receipt response")
        .expect("receipt should be present");

    assert_eq!(receipt.status, 1);
    assert_eq!(receipt.gas_used, 187_500);
    assert_eq!(
        receipt.effective_gas_price,
        Some(U256::from(1_436_363_921u64))
    );
    assert_eq!(receipt.logs.len(), 1);
    assert_eq!(receipt.logs[0].log_index, Some(2));

    let request_log = state.request_log().await;
    assert_eq!(request_log[0]["method"], "eth_getTransactionReceipt");
}

#[tokio::test]
async fn test_get_block_by_number_latest_parses_timestamp() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_getBlockByNumber",
            rpc_result(json!({
                "hash": "0x71ece187051700b814592f62774e6ebd8ebdf5efbb54c90859a7d1522ce38e0a",
                "number": "0x1542e9f",
                "parentHash": "0x2abcce1ac985ebea2a2d6878a78387158f46de8d6db2cefca00ea36df4030a40",
                "miner": "0x4838b106fce9647bdf1e7877bf73ce8b0bad5f97",
                "gasLimit": "0x223b4a1",
                "gasUsed": "0xde3909",
                "timestamp": "0x6801f4bb"
            })),
        )
        .await;
    let (client, state) = new_client(state).await;

    let block = client
        .get_block_by_number(None)
        .await
        .expect("block response")
        .expect("block should be present");

    assert_eq!(block.number, 22_294_175);
    assert_eq!(block.timestamp.as_u64(), 1_744_958_651_000_000_000);

    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_getBlockByNumber");
    assert_eq!(request["params"][0], "latest");
    assert_eq!(request["params"][1], false);
}

#[tokio::test]
async fn test_get_block_by_number_zero_parses_hash() {
    let state = MockRpcState::new();
    let genesis_hash = "0x1111111111111111111111111111111111111111111111111111111111111111";
    state
        .enqueue_json(
            "eth_getBlockByNumber",
            rpc_result(json!({
                "hash": genesis_hash,
                "number": "0x0",
                "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "miner": "0x0000000000000000000000000000000000000000",
                "gasLimit": "0x0",
                "gasUsed": "0x0",
                "timestamp": "0x0"
            })),
        )
        .await;
    let (client, state) = new_client(state).await;

    let block = client
        .get_block_by_number(Some(0))
        .await
        .expect("block response")
        .expect("genesis block should be present");

    assert_eq!(block.hash, genesis_hash);
    assert_eq!(block.number, 0);

    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_getBlockByNumber");
    assert_eq!(request["params"][0], "0x0");
}

#[tokio::test]
async fn test_get_logs_parses_topic_filtered_results() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_getLogs",
            rpc_result(json!([
                {
                    "removed": false,
                    "logIndex": "0x2",
                    "transactionIndex": "0x1",
                    "transactionHash": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "blockHash": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "blockNumber": "0x65",
                    "address": "0x3333333333333333333333333333333333333333",
                    "data": "0x00",
                    "topics": ["0xfeed"]
                }
            ])),
        )
        .await;
    let (client, state) = new_client(state).await;

    let address = parse_address("0x3333333333333333333333333333333333333333");
    let logs = client
        .get_logs(
            Some(&address),
            Some(vec![Some("0xfeed".to_string())]),
            100,
            101,
        )
        .await
        .expect("get logs response");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].topics, vec!["0xfeed".to_string()]);
    assert_eq!(logs[0].block_number, Some("0x65".to_string()));

    let request_log = state.request_log().await;
    let request = &request_log[0];
    assert_eq!(request["method"], "eth_getLogs");
    assert_eq!(request["params"][0]["fromBlock"], "0x64");
    assert_eq!(request["params"][0]["toBlock"], "0x65");
}

#[tokio::test]
async fn test_get_logs_splits_backfill_on_provider_range_error() {
    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_getLogs",
            rpc_error(
                -32005,
                "query returned more than 10000 results",
                json!({"from": "0x1", "to": "0x4"}),
            ),
        )
        .await;
    state
        .enqueue_json("eth_getLogs", rpc_result(json!([])))
        .await;
    state
        .enqueue_json(
            "eth_getLogs",
            rpc_result(json!([
                {
                    "removed": false,
                    "logIndex": "0x1",
                    "transactionIndex": "0x1",
                    "transactionHash": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "blockHash": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "blockNumber": "0x4",
                    "address": "0x3333333333333333333333333333333333333333",
                    "data": "0x01",
                    "topics": ["0xfeed"]
                }
            ])),
        )
        .await;

    let (client, state) = new_client(state).await;

    let logs = client
        .get_logs(None, Some(vec![Some("0xfeed".to_string())]), 1, 4)
        .await
        .expect("split get logs response");

    assert_eq!(logs.len(), 1);
    assert_eq!(state.method_count("eth_getLogs").await, 3);

    let request_log = state.request_log().await;
    assert_eq!(request_log[0]["params"][0]["fromBlock"], "0x1");
    assert_eq!(request_log[0]["params"][0]["toBlock"], "0x4");
    assert_eq!(request_log[1]["params"][0]["fromBlock"], "0x1");
    assert_eq!(request_log[1]["params"][0]["toBlock"], "0x2");
    assert_eq!(request_log[2]["params"][0]["fromBlock"], "0x3");
    assert_eq!(request_log[2]["params"][0]["toBlock"], "0x4");
}

#[tokio::test]
async fn test_get_code_returns_empty_or_bytecode() {
    let state = MockRpcState::new();
    state
        .enqueue_json("eth_getCode", rpc_result(json!("0x")))
        .await;
    state
        .enqueue_json("eth_getCode", rpc_result(json!("0x6000600055")))
        .await;

    let (client, state) = new_client(state).await;
    let address = parse_address("0x4444444444444444444444444444444444444444");

    let empty_code = client
        .get_code(&address, Some("latest"))
        .await
        .expect("empty code response");
    let bytecode = client
        .get_code(&address, Some("latest"))
        .await
        .expect("bytecode response");

    assert_eq!(empty_code, "0x");
    assert_eq!(bytecode, "0x6000600055");
    assert_eq!(state.method_count("eth_getCode").await, 2);
}

#[tokio::test]
async fn test_chain_id_parses_expected_network() {
    let state = MockRpcState::new();
    state
        .enqueue_json("eth_chainId", rpc_result(json!("0x38")))
        .await;
    let (client, state) = new_client(state).await;

    let chain_id = client.chain_id().await.expect("chain id response");
    assert_eq!(chain_id, 56);

    let request_log = state.request_log().await;
    assert_eq!(request_log[0]["method"], "eth_chainId");
}

#[tokio::test]
async fn test_rpc_error_body_maps_to_client_error() {
    let state = MockRpcState::new();
    state
        .enqueue_response(
            "eth_chainId",
            MockRpcResponse::json(rpc_error(
                -32001,
                "upstream unavailable",
                json!({"provider": "mock"}),
            ))
            .with_status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await;
    let (client, _state) = new_client(state).await;

    let err = client
        .chain_id()
        .await
        .expect_err("expected rpc body error");
    let err_text = err.to_string();

    assert!(err_text.contains("HTTP 500"));
    assert!(err_text.contains("-32001"));
    assert!(err_text.contains("upstream unavailable"));
    assert!(err_text.contains("provider"));
}

#[tokio::test]
async fn test_http_429_rate_limited_maps_to_retryable_rpc_error() {
    let state = MockRpcState::new();
    state
        .enqueue_response(
            "eth_getTransactionReceipt",
            MockRpcResponse::json(rpc_error(
                -32005,
                "rate limit",
                json!({"hint": "slow down"}),
            ))
            .with_status(StatusCode::TOO_MANY_REQUESTS),
        )
        .await;
    let (client, _state) = new_client(state).await;

    let err = client
        .get_transaction_receipt(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .expect_err("expected 429 error");
    let err_text = err.to_string();

    assert!(err_text.contains("HTTP 429"));
    assert!(err_text.contains("-32005"));
    assert!(err_text.contains("rate limit"));
}

#[tokio::test]
async fn test_http_429_retry_after_header_is_respected_when_present() {
    let state = MockRpcState::new();
    state
        .enqueue_response(
            "eth_sendRawTransaction",
            MockRpcResponse::json(rpc_error(
                -32005,
                "too many requests",
                json!({"detail": "burst exceeded"}),
            ))
            .with_status(StatusCode::TOO_MANY_REQUESTS)
            .with_header("retry-after", "7"),
        )
        .await;
    let (client, _state) = new_client(state).await;

    let err = client
        .send_raw_transaction("0x02deadbeef")
        .await
        .expect_err("expected retry-after error");
    let err_text = err.to_string();

    assert!(err_text.contains("HTTP 429"));
    assert!(err_text.contains("retry_after=7"));
}

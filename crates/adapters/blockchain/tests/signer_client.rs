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

use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use alloy::primitives::Address;
use axum::http::StatusCode;
use nautilus_blockchain::execution::signer::{
    RemoteSignerClient, SignRequest, SignerApiMode, SignerMtlsConfig,
    assert_rpc_tx_hash_matches_computed, tx_hash_hex_from_signer_raw_tx,
    types::RemoteSignerClientConfig,
};
use nautilus_network::retry::RetryConfig;
use serde_json::json;

mod common;

use common::mock_signer::{MockSignerResponse, MockSignerState, start_mock_signer_server};

const KNOWN_TX_HASH: &str = "0x1ae6c90fe2d927f8a395979934e0f757c7d63dc60dab8a3b0b5043a246f48a09";
const KNOWN_RAW_TX_HEX: &str = "0x02f8af380384068e778084068e77808286ed947977bf3e7e0c954d12cdca3e013adaf57e0b06e080b844a9059cbb000000000000000000000000c7bd78fa510c234b7e6f70f41452948e41d9a9210000000000000000000000000000000000000000000000007e2b7f14c776c000c001a08875591e4e10637a3498a8a05b1d4e498c1d027b2f763a77db767173955a8722a039defee4d3fb717502dcfcff5ccb9122c473e5efac8ad3c5c9c2e97e64a0885f";
const KNOWN_FROM: &str = "0x058e41ae42e322e5e6ea6fc9930776d67cdd3115";
const KNOWN_TO: &str = "0x7977bf3e7e0c954d12cdca3e013adaf57e0b06e0";
const KNOWN_SELECTOR: &str = "0xa9059cbb";
const KNOWN_DATA: &str = "0xa9059cbb000000000000000000000000c7bd78fa510c234b7e6f70f41452948e41d9a9210000000000000000000000000000000000000000000000007e2b7f14c776c000";

fn signer_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 2,
        initial_delay_ms: 10,
        max_delay_ms: 25,
        backoff_factor: 1.2,
        jitter_ms: 0,
        operation_timeout_ms: Some(1_000),
        immediate_first: true,
        max_elapsed_ms: Some(5_000),
    }
}

fn parse_address(value: &str) -> Address {
    Address::from_str(value).expect("valid address")
}

fn signer_config(endpoint: String) -> RemoteSignerClientConfig {
    RemoteSignerClientConfig {
        signer_endpoint: endpoint,
        signer_route: "/sign/eth".to_string(),
        signer_api_mode: SignerApiMode::OssV1Flat,
        signer_timeout_ms: 1_000,
        signer_require_tls: false,
        signer_wallet_address: parse_address(KNOWN_FROM),
        signer_retry_config: signer_retry_config(),
        signer_mtls: None,
    }
}

fn sign_request() -> SignRequest {
    SignRequest {
        chain_id: 56,
        nonce: 3,
        to: parse_address(KNOWN_TO),
        data: KNOWN_DATA.to_string(),
        value: "0x00".to_string(),
        gas: 0x86ed,
        max_fee_per_gas: Some(0x68e7780),
        max_priority_fee_per_gas: Some(0x68e7780),
        gas_price: None,
        deadline: 1_900_000_000,
        expected_notional: "1.0".to_string(),
        expected_selector: KNOWN_SELECTOR.to_string(),
    }
}

fn signer_success_response() -> serde_json::Value {
    json!({
        "r": "0x8875591e4e10637a3498a8a05b1d4e498c1d027b2f763a77db767173955a8722",
        "s": "0x39defee4d3fb717502dcfcff5ccb9122c473e5efac8ad3c5c9c2e97e64a0885f",
        "v": 1,
        "raw_tx_hex": KNOWN_RAW_TX_HEX,
    })
}

fn unix_now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_secs(),
    )
    .expect("unix time in i64")
}

async fn new_client(state: MockSignerState) -> (RemoteSignerClient, MockSignerState) {
    let addr = start_mock_signer_server(state.clone()).await;
    let config = signer_config(format!("http://{addr}"));
    let client = RemoteSignerClient::new(config).expect("signer client");
    (client, state)
}

async fn sign_once_and_capture_request() -> serde_json::Value {
    let state = MockSignerState::new();
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    let (client, state) = new_client(state).await;

    client
        .sign_evm_tx(sign_request())
        .await
        .expect("sign should succeed");

    state
        .request_log()
        .await
        .into_iter()
        .next()
        .expect("captured signer request")
}

#[tokio::test]
async fn test_sign_evm_tx_success_returns_raw_tx_and_metadata() {
    let state = MockSignerState::new();
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    let (client, state) = new_client(state).await;

    let signed = client
        .sign_evm_tx(sign_request())
        .await
        .expect("sign should succeed");

    assert_eq!(signed.raw_tx_hex, KNOWN_RAW_TX_HEX);
    assert_eq!(signed.tx_hash, KNOWN_TX_HASH);
    assert_eq!(signed.v, 1);
    assert_eq!(signed.request_id, 1);
    assert_eq!(state.call_count().await, 1);
}

#[tokio::test]
async fn test_sign_evm_tx_403_policy_deny_does_not_retry() {
    let state = MockSignerState::new();
    state
        .enqueue_response(
            MockSignerResponse::json(json!({"error": "policy_deny"}))
                .with_status(StatusCode::FORBIDDEN),
        )
        .await;
    let (client, state) = new_client(state).await;

    let err = client
        .sign_evm_tx(sign_request())
        .await
        .expect_err("403 must fail");

    assert!(err.to_string().contains("403"));
    assert_eq!(state.call_count().await, 1);
}

#[tokio::test]
async fn test_sign_evm_tx_429_rate_limited_does_not_retry() {
    let state = MockSignerState::new();
    state
        .enqueue_response(
            MockSignerResponse::json(json!({"error": "too many requests"}))
                .with_status(StatusCode::TOO_MANY_REQUESTS)
                .with_header("retry-after", "1"),
        )
        .await;
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    let (client, state) = new_client(state).await;

    let err = client
        .sign_evm_tx(sign_request())
        .await
        .expect_err("429 must fail without retry");

    assert!(err.to_string().contains("429"));
    assert_eq!(state.call_count().await, 1);
}

#[tokio::test]
async fn test_sign_evm_tx_500_retries_then_succeeds() {
    let state = MockSignerState::new();
    state
        .enqueue_response(
            MockSignerResponse::json(json!({"error": "upstream unavailable"}))
                .with_status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await;
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;

    let (client, state) = new_client(state).await;
    let signed = client
        .sign_evm_tx(sign_request())
        .await
        .expect("retry should succeed");

    assert_eq!(signed.tx_hash, KNOWN_TX_HASH);
    assert_eq!(state.call_count().await, 2);
}

#[tokio::test]
async fn test_sign_evm_tx_timeout_classified_as_retryable() {
    let state = MockSignerState::new();
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()).with_delay_ms(120))
        .await;
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;

    let addr = start_mock_signer_server(state.clone()).await;
    let mut config = signer_config(format!("http://{addr}"));
    config.signer_retry_config.operation_timeout_ms = Some(50);
    config.signer_retry_config.max_retries = 1;
    config.signer_retry_config.initial_delay_ms = 1;
    config.signer_retry_config.max_delay_ms = 1;
    let client = RemoteSignerClient::new(config).expect("signer client");

    let signed = client
        .sign_evm_tx(sign_request())
        .await
        .expect("timeout should retry and succeed");

    assert_eq!(signed.tx_hash, KNOWN_TX_HASH);
    assert_eq!(state.call_count().await, 2);
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_raw_tx_hex_not_matching_request() {
    let state = MockSignerState::new();
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    let (client, state) = new_client(state).await;

    let mut request = sign_request();
    request.nonce = 4;

    let err = client
        .sign_evm_tx(request)
        .await
        .expect_err("mismatched signed tx must fail closed");

    assert!(err.to_string().contains("nonce"));
    assert_eq!(state.call_count().await, 1);
}

#[test]
fn test_sign_evm_tx_rejects_http_endpoint_when_require_tls() {
    let mut config = signer_config("http://127.0.0.1:9999".to_string());
    config.signer_require_tls = true;

    let err = RemoteSignerClient::new(config).expect_err("http endpoint must be rejected");
    assert!(err.to_string().contains("https"));
}

#[tokio::test]
async fn test_sign_evm_tx_returns_tx_hash_deterministically() {
    let state = MockSignerState::new();
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    state
        .enqueue_response(MockSignerResponse::json(signer_success_response()))
        .await;
    let (client, state) = new_client(state).await;

    let signed_1 = client
        .sign_evm_tx(sign_request())
        .await
        .expect("first sign");
    let signed_2 = client
        .sign_evm_tx(sign_request())
        .await
        .expect("second sign");

    assert_eq!(signed_1.tx_hash, KNOWN_TX_HASH);
    assert_eq!(signed_2.tx_hash, KNOWN_TX_HASH);
    assert_eq!(state.call_count().await, 2);
}

#[test]
fn test_signer_tx_hash_keccak_raw_bytes_matches_known_type2_vector() {
    let tx_hash = tx_hash_hex_from_signer_raw_tx(KNOWN_RAW_TX_HEX).expect("tx hash");
    assert_eq!(tx_hash, KNOWN_TX_HASH);
}

#[test]
fn test_signer_tx_hash_mismatch_vs_rpc_response_fails_closed() {
    let err = assert_rpc_tx_hash_matches_computed(
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        KNOWN_TX_HASH,
    )
    .expect_err("mismatch must fail closed");

    assert!(err.to_string().contains("mismatch"));
}

#[test]
fn test_sign_evm_tx_mtls_config_builds_client() {
    let mut config = signer_config("https://signer.example.com".to_string());
    config.signer_require_tls = true;
    config.signer_mtls = Some(SignerMtlsConfig {
        client_cert_path: Some("/tmp/client.crt".to_string()),
        client_key_path: Some("/tmp/client.key".to_string()),
        ca_cert_path: Some("/tmp/ca.crt".to_string()),
        client_cert_pem: None,
        client_key_pem: None,
        ca_cert_pem: None,
    });

    let client = RemoteSignerClient::new(config);
    assert!(client.is_ok());
}

#[tokio::test]
async fn test_sign_evm_tx_sends_flat_oss_payload_shape() {
    let request = sign_once_and_capture_request().await;

    assert!(request.get("chainId").is_some());
    assert!(request.get("to").is_some());
    assert!(request.get("expected_notional").is_some());
    assert!(request.get("expectedNotional").is_none());
    assert!(request.get("tx").is_none());
    assert!(request.get("intent").is_none());
    assert!(request["chainId"].is_number());
    assert!(request["nonce"].is_number());
    assert!(request["gas"].is_number());
    assert!(request["maxFeePerGas"].is_number());
    assert!(request["maxPriorityFeePerGas"].is_number());
    assert!(request["deadline"].is_number());
    assert!(request.get("expectedSelector").is_none());
    assert!(request.get("gasPrice").is_none());
    assert!(request.get("functionSelector").is_none());
    assert!(request.get("router").is_none());
    assert!(request.get("maxSlippageBps").is_none());
}

#[tokio::test]
async fn test_sign_evm_tx_oss_conformance_nested_payload_is_not_emitted() {
    let request = sign_once_and_capture_request().await;

    assert!(request.get("tx").is_none());
    assert!(request.get("intent").is_none());
}

#[tokio::test]
async fn test_sign_evm_tx_oss_conformance_numeric_fields_are_json_numbers() {
    let request = sign_once_and_capture_request().await;

    assert!(request["chainId"].is_number());
    assert!(request["nonce"].is_number());
    assert!(request["gas"].is_number());
    assert!(request["maxFeePerGas"].is_number());
    assert!(request["maxPriorityFeePerGas"].is_number());
    assert!(request["deadline"].is_number());
    assert!(!request["chainId"].is_string());
    assert!(!request["nonce"].is_string());
    assert!(!request["gas"].is_string());
}

#[tokio::test]
async fn test_sign_evm_tx_oss_conformance_extra_intent_fields_not_emitted() {
    let request = sign_once_and_capture_request().await;

    assert!(request.get("expectedSelector").is_none());
    assert!(request.get("intentHash").is_none());
    assert!(request.get("functionSelector").is_none());
    assert!(request.get("router").is_none());
    assert!(request.get("maxSlippageBps").is_none());
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_gas_price_only_unsafe_path() {
    let state = MockSignerState::new();
    let (client, state) = new_client(state).await;

    let mut request = sign_request();
    request.max_fee_per_gas = None;
    request.max_priority_fee_per_gas = None;
    request.gas_price = Some(1_000_000_000);

    let err = client
        .sign_evm_tx(request)
        .await
        .expect_err("gasPrice-only path must fail preflight");

    assert!(err.to_string().contains("EIP-1559"));
    assert_eq!(state.call_count().await, 0);
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_non_positive_deadline_preflight() {
    let state = MockSignerState::new();
    let (client, state) = new_client(state).await;

    let mut request = sign_request();
    request.deadline = 0;

    let err = client
        .sign_evm_tx(request)
        .await
        .expect_err("non-positive deadline must fail preflight");

    assert!(err.to_string().contains("deadline"));
    assert_eq!(state.call_count().await, 0);
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_stale_deadline_preflight() {
    let state = MockSignerState::new();
    let (client, state) = new_client(state).await;

    let mut request = sign_request();
    request.deadline = unix_now();

    let err = client
        .sign_evm_tx(request)
        .await
        .expect_err("stale deadline must fail preflight");

    assert!(err.to_string().contains("deadline"));
    assert_eq!(state.call_count().await, 0);
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_non_positive_expected_notional_preflight() {
    let state = MockSignerState::new();
    let (client, state) = new_client(state).await;

    for expected_notional in ["0", "-1", "0.000"] {
        let mut request = sign_request();
        request.expected_notional = expected_notional.to_string();

        let err = client
            .sign_evm_tx(request)
            .await
            .expect_err("non-positive expected_notional must fail preflight");

        assert!(err.to_string().contains("expected_notional"));
    }

    assert_eq!(state.call_count().await, 0);
}

#[tokio::test]
async fn test_sign_evm_tx_rejects_invalid_expected_notional_preflight() {
    let state = MockSignerState::new();
    let (client, state) = new_client(state).await;

    let mut request = sign_request();
    request.expected_notional = "not-a-decimal".to_string();

    let err = client
        .sign_evm_tx(request)
        .await
        .expect_err("invalid expected_notional must fail preflight");

    assert!(err.to_string().contains("expected_notional"));
    assert_eq!(state.call_count().await, 0);
}

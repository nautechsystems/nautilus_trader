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

use std::{collections::HashSet, sync::Arc, time::Duration};

use alloy::primitives::{Address, U256, address};
use async_trait::async_trait;
use nautilus_blockchain::{
    contracts::erc20::Erc20Contract,
    execution::{
        erc20_allowance::{
            AllowanceTxSigner, ApprovalPolicy, Erc20AllowanceConfig, ensure_allowance,
        },
        signer::{SignRequest, SignedTx},
    },
    rpc::http::BlockchainHttpRpcClient,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

mod common;

use common::{MockRpcState, start_mock_rpc_server};

#[derive(Default, Clone)]
struct LocalMockSigner {
    inner: Arc<Mutex<LocalMockSignerInner>>,
}

#[derive(Default)]
struct LocalMockSignerInner {
    requests: Vec<SignRequest>,
    responses: std::collections::VecDeque<anyhow::Result<SignedTx>>,
}

impl LocalMockSigner {
    async fn enqueue_success(&self, tx_hash: &str) {
        let mut guard = self.inner.lock().await;
        guard.responses.push_back(Ok(SignedTx {
            raw_tx_hex: "0x01".to_string(),
            r: "0x0".to_string(),
            s: "0x0".to_string(),
            v: 0,
            tx_hash: tx_hash.to_string(),
            request_id: 1,
        }));
    }

    async fn requests(&self) -> Vec<SignRequest> {
        self.inner.lock().await.requests.clone()
    }
}

#[async_trait(?Send)]
impl AllowanceTxSigner for LocalMockSigner {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx> {
        let mut guard = self.inner.lock().await;
        guard.requests.push(request);
        match guard.responses.pop_front() {
            Some(response) => response,
            None => Err(anyhow::anyhow!("no mock signer response configured")),
        }
    }
}

fn rpc_result(result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": result,
    })
}

fn rpc_error(code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn abi_u256_hex(value: U256) -> String {
    format!("0x{:064x}", value)
}

fn success_receipt(tx_hash: &str, owner: Address, token: Address, status: u64) -> Value {
    rpc_result(json!({
        "transactionHash": tx_hash,
        "blockHash": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "blockNumber": "0x1",
        "from": owner,
        "to": token,
        "cumulativeGasUsed": "0x5208",
        "gasUsed": "0x5208",
        "status": format!("0x{status:x}"),
        "logs": []
    }))
}

fn tx_hash(n: u64) -> String {
    format!("0x{n:064x}")
}

fn default_config(router: Address, policy: ApprovalPolicy) -> Erc20AllowanceConfig {
    Erc20AllowanceConfig {
        router,
        policy,
        unlimited_allowlist: HashSet::new(),
        unlimited_approval_max_amount: None,
        chain_id: 56,
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
        receipt_max_polls: 3,
        receipt_poll_interval: Duration::from_millis(1),
        deadline_ttl_secs: 300,
    }
}

async fn setup_clients(
    state: MockRpcState,
) -> (Arc<BlockchainHttpRpcClient>, Erc20Contract, MockRpcState) {
    let addr = start_mock_rpc_server(state.clone()).await;
    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
        format!("http://{addr}/"),
        None,
    ));
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);
    (rpc_client, erc20, state)
}

fn request_methods(requests: &[Value]) -> Vec<String> {
    requests
        .iter()
        .filter_map(|request| {
            request
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn decode_approve_amount(data_hex: &str) -> U256 {
    let bytes = hex::decode(data_hex.trim_start_matches("0x")).unwrap();
    U256::from_be_slice(&bytes[36..68])
}

#[tokio::test]
async fn test_allowance_sufficient_skips_approve_and_signer_not_called() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);

    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(abi_u256_hex(required + U256::from(1u64)))),
        )
        .await;

    let signer = LocalMockSigner::default();
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Exact);

    let result = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect("allowance should be sufficient");

    assert!(result.skipped);
    assert_eq!(result.approval_tx_hashes.len(), 0);
    assert_eq!(signer.requests().await.len(), 0);
    assert_eq!(state.method_count("eth_call").await, 1);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 0);
}

#[tokio::test]
async fn test_allowance_insufficient_signs_and_sends_approve_tx() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);
    let tx_hash = tx_hash(1);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x5")))
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash, owner, token, 1),
        )
        .await;
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(required))))
        .await;

    let signer = LocalMockSigner::default();
    signer.enqueue_success(&tx_hash).await;
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Exact);

    let result = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect("approve flow should succeed");

    assert!(!result.skipped);
    assert_eq!(result.approval_tx_hashes, vec![tx_hash.clone()]);
    assert_eq!(signer.requests().await.len(), 1);
    assert_eq!(state.method_count("eth_call").await, 2);
    assert_eq!(state.method_count("eth_getTransactionCount").await, 1);
    assert_eq!(state.method_count("eth_estimateGas").await, 1);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 1);
    assert_eq!(state.method_count("eth_getTransactionReceipt").await, 1);

    let methods = request_methods(&state.request_log().await);
    assert_eq!(
        methods,
        vec![
            "eth_call",
            "eth_getTransactionCount",
            "eth_estimateGas",
            "eth_sendRawTransaction",
            "eth_getTransactionReceipt",
            "eth_call",
        ]
    );
}

#[tokio::test]
async fn test_exact_policy_nonzero_current_allowance_approves_required_amount() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);
    let current = U256::from(100u64);
    let tx_hash = tx_hash(11);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(current))))
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x9")))
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash, owner, token, 1),
        )
        .await;
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(required))))
        .await;

    let signer = LocalMockSigner::default();
    signer.enqueue_success(&tx_hash).await;
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Exact);

    let result = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect("exact flow should succeed with absolute approve amount");

    assert_eq!(result.approval_tx_hashes, vec![tx_hash]);
    let requests = signer.requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(decode_approve_amount(&requests[0].data), required);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 1);
}

#[tokio::test]
async fn test_unlimited_approval_rejected_when_token_not_allowlisted() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;

    let signer = LocalMockSigner::default();
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Unlimited);

    let err = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("token should be rejected by allowlist policy");

    assert!(err.to_string().contains("allowlist"));
    assert_eq!(signer.requests().await.len(), 0);
    assert_eq!(state.method_count("eth_call").await, 1);
    assert_eq!(state.method_count("eth_getTransactionCount").await, 0);
}

#[tokio::test]
async fn test_unlimited_approval_target_below_required_fails_early() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;

    let signer = LocalMockSigner::default();
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let mut config = default_config(router, ApprovalPolicy::Unlimited);
    config.unlimited_allowlist.insert(token);
    config.unlimited_approval_max_amount = Some(U256::from(100u64));

    let err = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("unlimited cap below required must fail before nonce/sign/send");

    assert!(err.to_string().contains("target is below required amount"));
    assert_eq!(signer.requests().await.len(), 0);
    assert_eq!(state.method_count("eth_getTransactionCount").await, 0);
    assert_eq!(state.method_count("eth_estimateGas").await, 0);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 0);
}

#[tokio::test]
async fn test_unlimited_approval_without_cap_fails_for_unrepresentable_notional() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(1u64);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;

    let signer = LocalMockSigner::default();
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let mut config = default_config(router, ApprovalPolicy::Unlimited);
    config.unlimited_allowlist.insert(token);

    let err = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("unlimited without cap should fail on unrepresentable expected_notional");

    assert!(
        err.to_string()
            .contains("failed to derive signer-compatible expected_notional")
    );
    assert_eq!(signer.requests().await.len(), 0);
    assert_eq!(state.method_count("eth_call").await, 1);
    assert_eq!(state.method_count("eth_getTransactionCount").await, 0);
    assert_eq!(state.method_count("eth_estimateGas").await, 0);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 0);
}

#[tokio::test]
async fn test_unlimited_reset_first_sends_zero_then_max_when_allowance_nonzero() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(10u64);
    let max_amount = U256::from(999u64);
    let tx_hash_1 = tx_hash(1);
    let tx_hash_2 = tx_hash(2);

    let state = MockRpcState::new();
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(abi_u256_hex(U256::from(5u64)))),
        )
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x7")))
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash_1)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash_1, owner, token, 1),
        )
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash_2)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash_2, owner, token, 1),
        )
        .await;
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(max_amount))))
        .await;

    let signer = LocalMockSigner::default();
    signer.enqueue_success(&tx_hash_1).await;
    signer.enqueue_success(&tx_hash_2).await;
    let (rpc_client, erc20, state) = setup_clients(state).await;

    let mut config = default_config(router, ApprovalPolicy::UnlimitedResetFirst);
    config.unlimited_allowlist.insert(token);
    config.unlimited_approval_max_amount = Some(max_amount);

    let result = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect("reset-first flow should succeed");

    assert_eq!(
        result.approval_tx_hashes,
        vec![tx_hash_1.clone(), tx_hash_2.clone()]
    );
    assert_eq!(state.method_count("eth_estimateGas").await, 2);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 2);
    assert_eq!(state.method_count("eth_getTransactionReceipt").await, 2);

    let requests = signer.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].nonce, 7);
    assert_eq!(requests[1].nonce, 8);
    assert_eq!(decode_approve_amount(&requests[0].data), U256::ZERO);
    assert_eq!(decode_approve_amount(&requests[1].data), max_amount);
}

#[tokio::test]
async fn test_approve_receipt_status_one_but_allowance_still_insufficient_fails_closed() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);
    let tx_hash = tx_hash(1);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash, owner, token, 1),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(abi_u256_hex(U256::from(499u64)))),
        )
        .await;

    let signer = LocalMockSigner::default();
    signer.enqueue_success(&tx_hash).await;
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Exact);

    let err = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("post-check must fail closed");

    assert!(err.to_string().contains("ALLOWANCE_NOT_UPDATED"));
    assert!(
        err.to_string()
            .contains("post-approve allowance check failed")
    );
    assert_eq!(state.method_count("eth_call").await, 2);
}

#[tokio::test]
async fn test_approve_receipt_status_zero_returns_error() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);
    let tx_hash = tx_hash(1);

    let state = MockRpcState::new();
    state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x5208")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash)))
        .await;
    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            success_receipt(&tx_hash, owner, token, 0),
        )
        .await;

    let signer = LocalMockSigner::default();
    signer.enqueue_success(&tx_hash).await;
    let (rpc_client, erc20, state) = setup_clients(state).await;
    let config = default_config(router, ApprovalPolicy::Exact);

    let err = ensure_allowance(
        rpc_client.as_ref(),
        &erc20,
        &signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("receipt status=0 must fail");

    assert!(err.to_string().contains("APPROVE_FAILED"));
    assert!(err.to_string().contains("status=0"));
    assert_eq!(state.method_count("eth_call").await, 1);
}

#[tokio::test]
async fn test_approve_nonce_or_gas_rpc_failure_bubbles_context() {
    let owner = address!("1111111111111111111111111111111111111111");
    let token = address!("2222222222222222222222222222222222222222");
    let router = address!("3333333333333333333333333333333333333333");
    let required = U256::from(500u64);

    // Nonce failure context.
    let nonce_state = MockRpcState::new();
    nonce_state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;
    nonce_state
        .enqueue_json(
            "eth_getTransactionCount",
            rpc_error(-32000, "nonce unavailable"),
        )
        .await;

    let nonce_signer = LocalMockSigner::default();
    let (nonce_rpc, nonce_erc20, _nonce_state) = setup_clients(nonce_state).await;
    let config = default_config(router, ApprovalPolicy::Exact);
    let nonce_err = ensure_allowance(
        nonce_rpc.as_ref(),
        &nonce_erc20,
        &nonce_signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("nonce failure should bubble with context");
    assert!(
        nonce_err
            .to_string()
            .contains("failed to fetch approve nonce")
    );
    assert_eq!(nonce_signer.requests().await.len(), 0);

    // Gas estimate failure context.
    let gas_state = MockRpcState::new();
    gas_state
        .enqueue_json("eth_call", rpc_result(json!(abi_u256_hex(U256::ZERO))))
        .await;
    gas_state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x1")))
        .await;
    gas_state
        .enqueue_json("eth_estimateGas", rpc_error(-32000, "gas estimate failed"))
        .await;

    let gas_signer = LocalMockSigner::default();
    let (gas_rpc, gas_erc20, _gas_state) = setup_clients(gas_state).await;
    let gas_err = ensure_allowance(
        gas_rpc.as_ref(),
        &gas_erc20,
        &gas_signer,
        owner,
        token,
        router,
        required,
        &config,
    )
    .await
    .expect_err("gas estimate failure should bubble with context");
    assert!(gas_err.to_string().contains("failed to estimate gas"));
    assert_eq!(gas_signer.requests().await.len(), 0);
}

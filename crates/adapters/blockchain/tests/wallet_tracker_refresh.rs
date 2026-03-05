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

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc, time::Duration};

use alloy::{
    primitives::{Address, Bytes, U256, address},
    sol_types::SolCall,
};
use axum::http::StatusCode;
use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig,
    contracts::{base::Multicall3, erc20::Erc20Contract},
    execution::{
        client::BlockchainExecutionClient,
        wallet::{WalletTracker, WalletTrackerConfig},
    },
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_common::{cache::Cache, clients::ExecutionClient, messages::execution::QueryAccount};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    defi::{Token, chain::chains},
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId, Venue},
    stubs::TestDefault,
};
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

fn encode_u256(value: U256) -> Bytes {
    let hex_value = format!("{value:064x}");
    Bytes::from(hex::decode(hex_value).expect("hex u256"))
}

fn encode_multicall_u256(values: &[U256]) -> String {
    let results: Vec<Multicall3::Result> = values
        .iter()
        .map(|value| Multicall3::Result {
            success: true,
            returnData: encode_u256(*value),
        })
        .collect();
    let encoded = Multicall3::tryAggregateCall::abi_encode_returns(&results);
    format!("0x{}", hex::encode(encoded))
}

fn decode_multicall_call_count(request: &Value) -> Option<usize> {
    let data_hex = request
        .get("params")
        .and_then(Value::as_array)?
        .first()?
        .get("data")?
        .as_str()?;
    let bytes = hex::decode(data_hex.trim_start_matches("0x")).ok()?;
    let decoded = Multicall3::tryAggregateCall::abi_decode(&bytes).ok()?;
    Some(decoded.calls.len())
}

fn make_token(address: Address, symbol: &str, decimals: u8) -> Token {
    Token::new(
        Arc::new(chains::BSC.clone()),
        address,
        symbol.to_string(),
        symbol.to_string(),
        decimals,
    )
}

fn make_tracker(
    wallet: Address,
    tokens: &[Address],
    spenders: Vec<Address>,
    max_tokens_per_refresh: usize,
    multicall_max_batch_size: usize,
    multicall_min_batch_size: usize,
) -> WalletTracker {
    let mut token_universe = HashSet::new();
    token_universe.extend(tokens.iter().copied());

    let config = WalletTrackerConfig {
        allowance_spenders: spenders,
        snapshot_ttl: Duration::from_secs(30),
        max_tokens_per_refresh,
        multicall_max_batch_size,
        multicall_min_batch_size,
    };
    WalletTracker::new(
        Arc::new(chains::BSC.clone()),
        wallet,
        token_universe,
        config,
    )
}

async fn setup_rpc() -> (MockRpcState, Arc<BlockchainHttpRpcClient>, Erc20Contract) {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(
        format!("http://{addr}/"),
        None,
    ));
    let erc20_contract = Erc20Contract::new(rpc_client.clone(), true);
    (state, rpc_client, erc20_contract)
}

#[tokio::test]
async fn test_wallet_tracker_batches_balance_of_via_multicall() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let token_a = address!("2222222222222222222222222222222222222222");
    let token_b = address!("3333333333333333333333333333333333333333");

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(5u64),
                U256::from(7u64)
            ]))),
        )
        .await;

    let mut tracker = make_tracker(wallet, &[token_a, token_b], Vec::new(), 16, 16, 1);
    tracker.seed_token_metadata(make_token(token_a, "TKA", 18));
    tracker.seed_token_metadata(make_token(token_b, "TKB", 18));

    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("wallet refresh should succeed");

    assert_eq!(state.method_count("eth_getBalance").await, 1);
    assert_eq!(state.method_count("eth_call").await, 1);

    let requests = state.request_log().await;
    let multicall_counts: Vec<usize> = requests
        .iter()
        .filter(|req| req.get("method").and_then(Value::as_str) == Some("eth_call"))
        .filter_map(decode_multicall_call_count)
        .collect();
    assert_eq!(multicall_counts, vec![2]);
}

#[tokio::test]
async fn test_wallet_tracker_batches_allowance_via_multicall() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let token_a = address!("2222222222222222222222222222222222222222");
    let token_b = address!("3333333333333333333333333333333333333333");
    let spender = address!("4444444444444444444444444444444444444444");

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(9u64),
                U256::from(11u64)
            ]))),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(15u64),
                U256::from(17u64)
            ]))),
        )
        .await;

    let mut tracker = make_tracker(wallet, &[token_a, token_b], vec![spender], 16, 16, 1);
    tracker.seed_token_metadata(make_token(token_a, "TKA", 18));
    tracker.seed_token_metadata(make_token(token_b, "TKB", 18));

    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("wallet refresh should succeed");

    assert_eq!(state.method_count("eth_call").await, 2);
    assert_eq!(
        tracker.allowances().get(&(token_a, spender)).copied(),
        Some(U256::from(15u64))
    );
    assert_eq!(
        tracker.allowances().get(&(token_b, spender)).copied(),
        Some(U256::from(17u64))
    );
}

#[tokio::test]
async fn test_refresh_replaces_snapshot_not_appends_duplicates() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let token_a = address!("2222222222222222222222222222222222222222");
    let token_b = address!("3333333333333333333333333333333333333333");

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(1u64),
                U256::from(2u64)
            ]))),
        )
        .await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x2")))
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(3u64),
                U256::from(4u64)
            ]))),
        )
        .await;

    let mut tracker = make_tracker(wallet, &[token_a, token_b], Vec::new(), 16, 16, 1);
    tracker.seed_token_metadata(make_token(token_a, "TKA", 18));
    tracker.seed_token_metadata(make_token(token_b, "TKB", 18));

    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("first refresh");
    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("second refresh");

    let snapshot = tracker.wallet_balance();
    assert_eq!(snapshot.token_balances.len(), 2);
    let amount_a = snapshot
        .token_balances
        .iter()
        .find(|balance| balance.token.address == token_a)
        .map(|balance| balance.amount)
        .expect("token_a should be present");
    let amount_b = snapshot
        .token_balances
        .iter()
        .find(|balance| balance.token.address == token_b)
        .map(|balance| balance.amount)
        .expect("token_b should be present");
    assert_eq!(amount_a, U256::from(3u64));
    assert_eq!(amount_b, U256::from(4u64));
}

#[tokio::test]
async fn test_wallet_tracker_allowance_refresh_supports_multiple_spenders() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let token_a = address!("2222222222222222222222222222222222222222");
    let token_b = address!("3333333333333333333333333333333333333333");
    let spender_a = address!("4444444444444444444444444444444444444444");
    let spender_b = address!("5555555555555555555555555555555555555555");

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(10u64),
                U256::from(20u64)
            ]))),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(30u64),
                U256::from(40u64)
            ]))),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(50u64),
                U256::from(60u64)
            ]))),
        )
        .await;

    let mut tracker = make_tracker(
        wallet,
        &[token_a, token_b],
        vec![spender_a, spender_b],
        16,
        16,
        1,
    );
    tracker.seed_token_metadata(make_token(token_a, "TKA", 18));
    tracker.seed_token_metadata(make_token(token_b, "TKB", 18));

    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("wallet refresh should succeed");

    assert_eq!(tracker.allowances().len(), 4);
    assert_eq!(
        tracker.allowances().get(&(token_a, spender_a)).copied(),
        Some(U256::from(30u64))
    );
    assert_eq!(
        tracker.allowances().get(&(token_b, spender_a)).copied(),
        Some(U256::from(40u64))
    );
    assert_eq!(
        tracker.allowances().get(&(token_a, spender_b)).copied(),
        Some(U256::from(50u64))
    );
    assert_eq!(
        tracker.allowances().get(&(token_b, spender_b)).copied(),
        Some(U256::from(60u64))
    );
}

#[tokio::test]
async fn test_wallet_tracker_adaptive_splits_on_provider_limit_error() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let tokens = [
        address!("2222222222222222222222222222222222222222"),
        address!("3333333333333333333333333333333333333333"),
        address!("4444444444444444444444444444444444444444"),
        address!("5555555555555555555555555555555555555555"),
    ];

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;
    state
        .enqueue_response(
            "eth_call",
            MockRpcResponse::json(rpc_error(-32005, "query returned more than 10000 results"))
                .with_status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(1u64),
                U256::from(2u64)
            ]))),
        )
        .await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_multicall_u256(&[
                U256::from(3u64),
                U256::from(4u64)
            ]))),
        )
        .await;

    let mut tracker = make_tracker(wallet, &tokens, Vec::new(), 32, 4, 1);
    tracker.seed_token_metadata(make_token(tokens[0], "A", 18));
    tracker.seed_token_metadata(make_token(tokens[1], "B", 18));
    tracker.seed_token_metadata(make_token(tokens[2], "C", 18));
    tracker.seed_token_metadata(make_token(tokens[3], "D", 18));

    tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect("adaptive split should recover");

    let requests = state.request_log().await;
    let multicall_counts: Vec<usize> = requests
        .iter()
        .filter(|req| req.get("method").and_then(Value::as_str) == Some("eth_call"))
        .filter_map(decode_multicall_call_count)
        .collect();

    // First attempt uses full batch, then split into two half-batches.
    assert!(multicall_counts.contains(&4));
    assert!(multicall_counts.iter().filter(|&&count| count == 2).count() >= 2);
}

#[tokio::test]
async fn test_wallet_refresh_respects_budget_caps_under_chainstack_profile() {
    let wallet = address!("1111111111111111111111111111111111111111");
    let token_a = address!("2222222222222222222222222222222222222222");
    let token_b = address!("3333333333333333333333333333333333333333");

    let (state, rpc_client, erc20_contract) = setup_rpc().await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;

    let mut tracker = make_tracker(wallet, &[token_a, token_b], Vec::new(), 1, 16, 1);
    tracker.seed_token_metadata(make_token(token_a, "TKA", 18));
    tracker.seed_token_metadata(make_token(token_b, "TKB", 18));

    let err = tracker
        .refresh(rpc_client.as_ref(), &erc20_contract)
        .await
        .expect_err("refresh should fail when token cap is exceeded");

    let err_text = err.to_string();
    assert!(err_text.contains("wallet_max_tokens_per_refresh"));
}

#[test]
fn test_query_account_triggers_refresh_and_does_not_panic() {
    let (state, rpc_url) = nautilus_common::live::get_runtime().block_on(async {
        let state = MockRpcState::new();
        let addr = start_mock_rpc_server(state.clone()).await;
        state
            .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
            .await;
        (state, format!("http://{addr}/"))
    });

    let trader_id = TraderId::test_default();
    let account_id = AccountId::test_default();
    let config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        Venue::new("Bsc:PancakeSwapV2"),
        chains::BSC.clone(),
        String::from("0x1111111111111111111111111111111111111111"),
        None,
        rpc_url,
        None,
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        config.venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );
    let client =
        BlockchainExecutionClient::new(core, config).expect("execution client should build");

    let cmd = QueryAccount::new(
        trader_id,
        Some(ClientId::new("BLOCKCHAIN")),
        account_id,
        UUID4::new(),
        UnixNanos::from(1),
    );

    client
        .query_account(&cmd)
        .expect("query_account should refresh without panic");
    let call_count =
        nautilus_common::live::get_runtime().block_on(state.method_count("eth_getBalance"));
    assert_eq!(call_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_query_account_from_runtime_thread_does_not_panic() {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    state
        .enqueue_json("eth_getBalance", rpc_result(json!("0x1")))
        .await;

    let trader_id = TraderId::test_default();
    let account_id = AccountId::test_default();
    let config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        Venue::new("Bsc:PancakeSwapV2"),
        chains::BSC.clone(),
        String::from("0x1111111111111111111111111111111111111111"),
        None,
        format!("http://{addr}/"),
        None,
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        config.venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );
    let client =
        BlockchainExecutionClient::new(core, config).expect("execution client should build");

    let cmd = QueryAccount::new(
        trader_id,
        Some(ClientId::new("BLOCKCHAIN")),
        account_id,
        UUID4::new(),
        UnixNanos::from(1),
    );

    client
        .query_account(&cmd)
        .expect("query_account should not panic on runtime thread");
    assert_eq!(state.method_count("eth_getBalance").await, 1);
}

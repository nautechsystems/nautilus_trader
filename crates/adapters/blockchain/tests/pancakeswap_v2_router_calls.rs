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

use std::sync::Arc;

use alloy::{
    primitives::{U256, address},
    sol_types::SolCall,
};
use nautilus_blockchain::{
    contracts::pancakeswap_v2_router::{
        PancakeSwapV2QuoteErrorCode, PancakeSwapV2Router, PancakeSwapV2RouterContract,
        PancakeSwapV2RouterError,
    },
    execution::amm::{AmmProtocolAdapter, pancakeswap_v2::PancakeSwapV2Adapter},
    rpc::http::BlockchainHttpRpcClient,
};
use serde_json::{Value, json};

mod common;

use common::{MockRpcState, start_mock_rpc_server};

fn rpc_result(result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": result,
    })
}

fn rpc_error(code: i64, message: &str, data: Option<Value>) -> Value {
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

fn encode_amounts_response(amounts: Vec<U256>) -> String {
    let encoded = PancakeSwapV2Router::getAmountsOutCall::abi_encode_returns(&amounts);
    format!("0x{}", hex::encode(encoded))
}

async fn setup_router() -> (MockRpcState, PancakeSwapV2RouterContract, Address) {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    let client = Arc::new(BlockchainHttpRpcClient::new(
        format!("http://{addr}/"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");

    (
        state,
        PancakeSwapV2RouterContract::new(client, router),
        router,
    )
}

type Address = alloy::primitives::Address;

#[tokio::test]
async fn test_quote_calls_eth_call_with_router_to_and_data() {
    let (state, router_contract, router_address) = setup_router().await;

    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_amounts_response(vec![
                U256::from(1_000u64),
                U256::from(2_500u64)
            ]))),
        )
        .await;

    let path = vec![
        address!("0x1111111111111111111111111111111111111111"),
        address!("0x2222222222222222222222222222222222222222"),
    ];
    let amounts = router_contract
        .quote_exact_in(U256::from(1_000u64), path.clone())
        .await
        .expect("quote should succeed");

    assert_eq!(amounts, vec![U256::from(1_000u64), U256::from(2_500u64)]);

    let expected_data =
        PancakeSwapV2RouterContract::encode_get_amounts_out_call(U256::from(1_000u64), path)
            .expect("encoding should succeed");

    let requests = state.request_log().await;
    let request = requests
        .iter()
        .find(|entry| entry.get("method").and_then(Value::as_str) == Some("eth_call"))
        .expect("eth_call request should be logged");

    let params = request
        .get("params")
        .and_then(Value::as_array)
        .expect("eth_call params should exist");
    let call_obj = params
        .first()
        .and_then(Value::as_object)
        .expect("call object should exist");

    let to = call_obj
        .get("to")
        .and_then(Value::as_str)
        .expect("to should be present");
    assert_eq!(
        to.to_ascii_lowercase(),
        router_address.to_string().to_ascii_lowercase()
    );

    let data = call_obj
        .get("data")
        .and_then(Value::as_str)
        .expect("data should be present");
    assert_eq!(data, format!("0x{}", hex::encode(expected_data)));
}

#[tokio::test]
async fn test_quote_rpc_revert_maps_to_quote_error() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_error(
                -32000,
                "execution reverted",
                Some(json!("0x08c379a0000000000000000000000000")),
            ),
        )
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("quote should fail");

    match err {
        PancakeSwapV2RouterError::Quote(quote_err) => {
            assert_eq!(quote_err.code, PancakeSwapV2QuoteErrorCode::RpcRevert);
            assert_eq!(quote_err.rpc_code, Some(-32000));
            assert!(quote_err.message.contains("execution reverted"));
            assert!(quote_err.data.is_some());
        }
        other => panic!("expected quote error mapping, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_non_revert_rpc_error_stays_rpc_error() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json("eth_call", rpc_error(-32000, "upstream timeout", None))
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("quote should fail");

    match err {
        PancakeSwapV2RouterError::Rpc(inner) => {
            assert!(
                inner
                    .to_string()
                    .to_ascii_lowercase()
                    .contains("upstream timeout")
            );
        }
        other => panic!("expected Rpc classification for non-revert error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_rejects_amounts_response_shorter_than_path() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_amounts_response(vec![U256::from(1_000u64)]))),
        )
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("invalid response length must fail closed");

    match err {
        PancakeSwapV2RouterError::InvalidQuoteResponse(message) => {
            assert!(message.contains("expected 2 amount points, got 1"));
        }
        other => panic!("expected InvalidQuoteResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_rejects_amounts_response_longer_than_path() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_amounts_response(vec![
                U256::from(1_000u64),
                U256::from(2_500u64),
                U256::from(2_600u64),
            ]))),
        )
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("invalid response length must fail closed");

    match err {
        PancakeSwapV2RouterError::InvalidQuoteResponse(message) => {
            assert!(message.contains("expected 2 amount points, got 3"));
        }
        other => panic!("expected InvalidQuoteResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_revert_insufficient_liquidity_maps_to_dex_error_code() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_error(
                -32000,
                "execution reverted: PancakeLibrary: INSUFFICIENT_LIQUIDITY",
                None,
            ),
        )
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("quote should fail");

    match err {
        PancakeSwapV2RouterError::Quote(quote_err) => {
            assert_eq!(
                quote_err.code,
                PancakeSwapV2QuoteErrorCode::InsufficientLiquidity
            );
        }
        other => panic!("expected quote error mapping, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_revert_insufficient_amount_maps_to_dex_error_code() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_error(
                -32000,
                "execution reverted: PancakeLibrary: INSUFFICIENT_AMOUNT",
                None,
            ),
        )
        .await;

    let err = router_contract
        .quote_exact_out(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("quote should fail");

    match err {
        PancakeSwapV2RouterError::Quote(quote_err) => {
            assert_eq!(
                quote_err.code,
                PancakeSwapV2QuoteErrorCode::InsufficientAmount
            );
        }
        other => panic!("expected quote error mapping, got {other:?}"),
    }
}

#[tokio::test]
async fn test_quote_revert_identical_addresses_maps_to_dex_error_code() {
    let (state, router_contract, _) = setup_router().await;
    state
        .enqueue_json(
            "eth_call",
            rpc_error(
                -32000,
                "execution reverted: PancakeLibrary: IDENTICAL_ADDRESSES",
                None,
            ),
        )
        .await;

    let err = router_contract
        .quote_exact_in(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .await
        .expect_err("quote should fail");

    match err {
        PancakeSwapV2RouterError::Quote(quote_err) => {
            assert_eq!(
                quote_err.code,
                PancakeSwapV2QuoteErrorCode::IdenticalAddresses
            );
        }
        other => panic!("expected quote error mapping, got {other:?}"),
    }
}

#[test]
fn test_swap_tx_build_uses_min_out_deadline_path_recipient() {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = PancakeSwapV2Adapter::new(client, router, wallet);

    let path = vec![
        address!("0x1111111111111111111111111111111111111111"),
        address!("0x2222222222222222222222222222222222222222"),
    ];
    let recipient = wallet;

    let tx_call = adapter
        .build_swap_exact_in_tx(
            U256::from(1_000u64),
            U256::from(900u64),
            path.clone(),
            recipient,
            U256::from(1_234_567u64),
        )
        .expect("swap tx build should succeed");

    assert_eq!(tx_call.to, router);
    assert_eq!(tx_call.value, U256::ZERO);

    let decoded =
        PancakeSwapV2Router::swapExactTokensForTokensCall::abi_decode(tx_call.data.as_slice())
            .expect("swap call should decode");

    assert_eq!(decoded.amountIn, U256::from(1_000u64));
    assert_eq!(decoded.amountOutMin, U256::from(900u64));
    assert_eq!(decoded.path, path);
    assert_eq!(decoded.to, recipient);
    assert_eq!(decoded.deadline, U256::from(1_234_567u64));
}

#[test]
fn test_swap_tx_build_rejects_recipient_override() {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = PancakeSwapV2Adapter::new(client, router, wallet);

    let err = adapter
        .build_swap_exact_in_tx(
            U256::from(1_000u64),
            U256::from(900u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
            address!("0x4444444444444444444444444444444444444444"),
            U256::from(1_234_567u64),
        )
        .expect_err("recipient override must fail closed");

    assert!(
        err.to_string()
            .contains("does not support recipient override")
    );
}

#[test]
fn test_swap_exact_out_tx_build_rejects_recipient_override() {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = PancakeSwapV2Adapter::new(client, router, wallet);

    let err = adapter
        .build_swap_exact_out_tx(
            U256::from(900u64),
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
            address!("0x4444444444444444444444444444444444444444"),
            U256::from(1_234_567u64),
        )
        .expect_err("recipient override must fail closed");

    assert!(
        err.to_string()
            .contains("does not support recipient override")
    );
}

#[test]
fn test_swap_tx_build_rejects_zero_deadline_for_exact_in_and_exact_out() {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = PancakeSwapV2Adapter::new(client, router, wallet);
    let path = vec![
        address!("0x1111111111111111111111111111111111111111"),
        address!("0x2222222222222222222222222222222222222222"),
    ];

    let err_exact_in = adapter
        .build_swap_exact_in_tx(
            U256::from(1_000u64),
            U256::from(900u64),
            path.clone(),
            wallet,
            U256::ZERO,
        )
        .expect_err("zero deadline must fail closed");
    assert!(err_exact_in.to_string().contains("deadline must be > 0"));

    let err_exact_out = adapter
        .build_swap_exact_out_tx(
            U256::from(900u64),
            U256::from(1_000u64),
            path,
            wallet,
            U256::ZERO,
        )
        .expect_err("zero deadline must fail closed");
    assert!(err_exact_out.to_string().contains("deadline must be > 0"));
}

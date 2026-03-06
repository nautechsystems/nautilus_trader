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
    primitives::{address, Address, U256},
    sol,
    sol_types::SolValue,
};
use nautilus_blockchain::{
    contracts::uniswap_v2_pair::SWAP_EVENT_TOPIC0_HEX,
    execution::amm::{pancakeswap_v2::PancakeSwapV2Adapter, AmmProtocolAdapter},
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_model::defi::{ReceiptLog, TransactionReceipt};

sol! {
    struct SwapEventDataFixture {
        uint256 amount0In;
        uint256 amount1In;
        uint256 amount0Out;
        uint256 amount1Out;
    }
}

fn topic_from_address(address: Address) -> String {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(address.as_slice());
    format!("0x{}", hex::encode(word))
}

fn encode_swap_data(
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
) -> String {
    let encoded = SwapEventDataFixture {
        amount0In: amount0_in,
        amount1In: amount1_in,
        amount0Out: amount0_out,
        amount1Out: amount1_out,
    }
    .abi_encode();
    format!("0x{}", hex::encode(encoded))
}

fn make_swap_log(
    pool: Address,
    sender: Address,
    to: Address,
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
    log_index: u64,
) -> ReceiptLog {
    ReceiptLog {
        address: pool,
        topics: vec![
            SWAP_EVENT_TOPIC0_HEX.to_string(),
            topic_from_address(sender),
            topic_from_address(to),
        ],
        data: encode_swap_data(amount0_in, amount1_in, amount0_out, amount1_out),
        log_index: Some(log_index),
        transaction_index: Some(1),
        transaction_hash: Some(
            "0x4f6a905f6ac0f749f2f7d4fe53cc994f94935271528ef2f7f5f51775a8d238f9".to_string(),
        ),
        block_hash: Some(
            "0x5f567bb0f2a4e22a5f8cc05610b18d5f8ef01cf15b8b7b8d0a8e1756e7f64f5e".to_string(),
        ),
        block_number: Some(42),
        removed: Some(false),
    }
}

fn make_receipt(status: u64, logs: Vec<ReceiptLog>) -> TransactionReceipt {
    TransactionReceipt {
        transaction_hash: "0x4f6a905f6ac0f749f2f7d4fe53cc994f94935271528ef2f7f5f51775a8d238f9"
            .to_string(),
        block_hash: "0x5f567bb0f2a4e22a5f8cc05610b18d5f8ef01cf15b8b7b8d0a8e1756e7f64f5e"
            .to_string(),
        block_number: 42,
        from: address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        to: Some(address!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
        cumulative_gas_used: 100_000,
        gas_used: 90_000,
        effective_gas_price: Some(U256::from(5_000_000_000u64)),
        status,
        logs,
    }
}

fn make_adapter(wallet: Address) -> PancakeSwapV2Adapter {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    PancakeSwapV2Adapter::new(client, router, wallet)
}

#[test]
fn test_receipt_decode_sets_tx_hash_and_sorts_by_log_index() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
    let token0 = address!("0x1111111111111111111111111111111111111111");
    let token1 = address!("0x2222222222222222222222222222222222222222");

    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::from(1_000u64),
            U256::ZERO,
            U256::ZERO,
            U256::from(2_500u64),
            7,
        )],
    );

    let fills = adapter
        .decode_fills_from_receipt(&receipt, pool, vec![token0, token1])
        .expect("receipt decode should succeed");

    assert_eq!(fills.len(), 1);
    let fill = &fills[0];
    assert_eq!(fill.tx_hash, receipt.transaction_hash);
    assert_eq!(fill.log_index, 7);
    assert_eq!(fill.token_in, token0);
    assert_eq!(fill.token_out, token1);
    assert_eq!(fill.amount_in, U256::from(1_000u64));
    assert_eq!(fill.amount_out, U256::from(2_500u64));
}

#[test]
fn test_receipt_decode_rejects_if_no_swap_log_for_expected_pool() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let expected_pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
    let other_pool = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1");

    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            other_pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::from(1_000u64),
            U256::ZERO,
            U256::ZERO,
            U256::from(2_500u64),
            4,
        )],
    );

    let err = adapter
        .decode_fills_from_receipt(
            &receipt,
            expected_pool,
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect_err("missing expected-pool swap log must fail closed");

    assert!(err
        .to_string()
        .contains("no swap log found for expected pool"));
}

#[test]
fn test_receipt_decode_rejects_if_multiple_swap_logs_for_expected_pool() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");

    let receipt = make_receipt(
        1,
        vec![
            make_swap_log(
                pool,
                address!("0x9999999999999999999999999999999999999999"),
                wallet,
                U256::from(1_000u64),
                U256::ZERO,
                U256::ZERO,
                U256::from(2_500u64),
                2,
            ),
            make_swap_log(
                pool,
                address!("0x9999999999999999999999999999999999999999"),
                wallet,
                U256::from(2_000u64),
                U256::ZERO,
                U256::ZERO,
                U256::from(4_000u64),
                3,
            ),
        ],
    );

    let err = adapter
        .decode_fills_from_receipt(
            &receipt,
            pool,
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect_err("multiple expected-pool swap logs must fail closed");

    assert!(err
        .to_string()
        .contains("expected exactly one swap log for pool"));
}

#[test]
fn test_receipt_decode_rejects_swap_log_from_other_pool_address() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let expected_pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
    let other_pool = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1");

    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            other_pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::from(1_000u64),
            U256::ZERO,
            U256::ZERO,
            U256::from(2_500u64),
            4,
        )],
    );

    let err = adapter
        .decode_fills_from_receipt(
            &receipt,
            expected_pool,
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect_err("swap from non-expected pool must fail closed");

    assert!(err
        .to_string()
        .contains("no swap log found for expected pool"));
}

#[test]
fn test_receipt_decode_rejects_path_len_not_two() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");

    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::from(1_000u64),
            U256::ZERO,
            U256::ZERO,
            U256::from(2_500u64),
            7,
        )],
    );

    let err = adapter
        .decode_fills_from_receipt(
            &receipt,
            pool,
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
                address!("0x3333333333333333333333333333333333333333"),
            ],
        )
        .expect_err("path length >2 must fail closed");

    assert!(err.to_string().contains("single-hop receipt decode"));
}

#[test]
fn test_receipt_decode_rejects_if_swap_to_is_not_wallet() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let other_recipient = address!("0x4444444444444444444444444444444444444444");
    let adapter = make_adapter(wallet);
    let pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");

    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            pool,
            address!("0x9999999999999999999999999999999999999999"),
            other_recipient,
            U256::from(1_000u64),
            U256::ZERO,
            U256::ZERO,
            U256::from(2_500u64),
            7,
        )],
    );

    let err = adapter
        .decode_fills_from_receipt(
            &receipt,
            pool,
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect_err("swap recipient mismatch must fail closed");

    assert!(err.to_string().contains("swap recipient mismatch"));
}

#[test]
fn test_receipt_decode_accepts_swap_when_pool_token_order_differs_from_path_order() {
    let wallet = address!("0x3333333333333333333333333333333333333333");
    let adapter = make_adapter(wallet);
    let pool = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
    let token_in = address!("0x1111111111111111111111111111111111111111");
    let token_out = address!("0x2222222222222222222222222222222222222222");

    // Represents token1 -> token0 pool direction, while expected path is token_in -> token_out.
    let receipt = make_receipt(
        1,
        vec![make_swap_log(
            pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::ZERO,
            U256::from(1_000u64),
            U256::from(2_500u64),
            U256::ZERO,
            9,
        )],
    );

    let fills = adapter
        .decode_fills_from_receipt(&receipt, pool, vec![token_in, token_out])
        .expect("receipt decode should infer fill amounts independent of pair token order");

    assert_eq!(fills.len(), 1);
    let fill = &fills[0];
    assert_eq!(fill.token_in, token_in);
    assert_eq!(fill.token_out, token_out);
    assert_eq!(fill.amount_in, U256::from(1_000u64));
    assert_eq!(fill.amount_out, U256::from(2_500u64));
}

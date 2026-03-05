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

use alloy::{
    primitives::{U256, address},
    sol,
    sol_types::SolValue,
};
use nautilus_blockchain::contracts::uniswap_v2_pair::{
    SWAP_EVENT_TOPIC0_HEX, UniswapV2PairError, decode_swap_log, map_swap_to_fill,
};
use nautilus_model::defi::ReceiptLog;

sol! {
    struct SwapEventDataFixture {
        uint256 amount0In;
        uint256 amount1In;
        uint256 amount0Out;
        uint256 amount1Out;
    }
}

fn topic_from_address(address: alloy::primitives::Address) -> String {
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
    sender: alloy::primitives::Address,
    to: alloy::primitives::Address,
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
) -> ReceiptLog {
    ReceiptLog {
        address: address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245"),
        topics: vec![
            SWAP_EVENT_TOPIC0_HEX.to_string(),
            topic_from_address(sender),
            topic_from_address(to),
        ],
        data: encode_swap_data(amount0_in, amount1_in, amount0_out, amount1_out),
        log_index: Some(7),
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

#[test]
fn test_decode_swap_log_amounts_in_out_and_addresses() {
    let sender = address!("0x1111111111111111111111111111111111111111");
    let to = address!("0x2222222222222222222222222222222222222222");
    let token0 = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let token1 = address!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

    let log = make_swap_log(
        sender,
        to,
        U256::from(1_000u64),
        U256::ZERO,
        U256::ZERO,
        U256::from(2_500u64),
    );

    let decoded = decode_swap_log(&log).expect("swap log decode should succeed");
    assert_eq!(decoded.sender, sender);
    assert_eq!(decoded.to, to);
    assert_eq!(decoded.amount0_in, U256::from(1_000u64));
    assert_eq!(decoded.amount1_out, U256::from(2_500u64));

    let fill =
        map_swap_to_fill(&decoded, token0, token1).expect("direction mapping should succeed");
    assert_eq!(fill.token_in, token0);
    assert_eq!(fill.token_out, token1);
    assert_eq!(fill.amount_in, U256::from(1_000u64));
    assert_eq!(fill.amount_out, U256::from(2_500u64));
}

#[test]
fn test_decode_swap_log_rejects_wrong_topic0() {
    let sender = address!("0x1111111111111111111111111111111111111111");
    let to = address!("0x2222222222222222222222222222222222222222");

    let mut log = make_swap_log(
        sender,
        to,
        U256::from(1_000u64),
        U256::ZERO,
        U256::ZERO,
        U256::from(2_500u64),
    );
    log.topics[0] = format!("0x{}", "11".repeat(32));

    let err = decode_swap_log(&log).expect_err("wrong topic0 must fail");
    match err {
        UniswapV2PairError::InvalidTopic0 { .. } => {}
        other => panic!("expected InvalidTopic0, got {other:?}"),
    }
}

#[test]
fn test_decode_swap_log_rejects_ambiguous_amounts() {
    let sender = address!("0x1111111111111111111111111111111111111111");
    let to = address!("0x2222222222222222222222222222222222222222");
    let token0 = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let token1 = address!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

    let log = make_swap_log(
        sender,
        to,
        U256::from(1_000u64),
        U256::from(10u64),
        U256::ZERO,
        U256::from(2_500u64),
    );

    let decoded = decode_swap_log(&log).expect("swap log decode should succeed");
    let err = map_swap_to_fill(&decoded, token0, token1)
        .expect_err("ambiguous amount shape must fail closed");
    assert_eq!(err, UniswapV2PairError::AmbiguousSwapAmounts);
}

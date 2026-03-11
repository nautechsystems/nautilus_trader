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
    dyn_abi::SolType,
    primitives::{Address, U256},
    sol,
};
use nautilus_model::defi::ReceiptLog;
use thiserror::Error;

pub const SWAP_EVENT_TOPIC0_HEX: &str =
    "0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822";
pub const SWAP_EVENT_TOPIC0: [u8; 32] = [
    0xd7, 0x8a, 0xd9, 0x5f, 0xa4, 0x6c, 0x99, 0x4b, 0x65, 0x51, 0xd0, 0xda, 0x85, 0xfc, 0x27, 0x5f,
    0xe6, 0x13, 0xce, 0x37, 0x65, 0x7f, 0xb8, 0xd5, 0xe3, 0xd1, 0x30, 0x84, 0x01, 0x59, 0xd8, 0x22,
];

sol! {
    struct SwapEventData {
        uint256 amount0In;
        uint256 amount1In;
        uint256 amount0Out;
        uint256 amount1Out;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniswapV2PairSwapLog {
    pub sender: Address,
    pub to: Address,
    pub amount0_in: U256,
    pub amount1_in: U256,
    pub amount0_out: U256,
    pub amount1_out: U256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniswapV2SwapFill {
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out: U256,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UniswapV2PairError {
    #[error("swap log missing topic0")]
    MissingTopic0,
    #[error("invalid swap topic0 expected={expected} actual={actual}")]
    InvalidTopic0 { expected: String, actual: String },
    #[error("topic index {index} is missing")]
    MissingTopic { index: usize },
    #[error("topic index {index} is not a 32-byte hex word: {topic}")]
    InvalidTopicHex { index: usize, topic: String },
    #[error("log data is not valid hex: {data}")]
    InvalidDataHex { data: String },
    #[error("swap log data must be exactly 128 bytes, got {len}")]
    InvalidDataLength { len: usize },
    #[error("failed to decode swap event data: {0}")]
    Decode(String),
    #[error("swap amounts are ambiguous for deterministic fill mapping")]
    AmbiguousSwapAmounts,
}

#[must_use]
pub fn topic0_is_swap(topic0: &str) -> bool {
    decode_word_hex(topic0).is_ok_and(|word| word == SWAP_EVENT_TOPIC0)
}

pub fn decode_swap_log(log: &ReceiptLog) -> Result<UniswapV2PairSwapLog, UniswapV2PairError> {
    let topic0 = log
        .topics
        .first()
        .ok_or(UniswapV2PairError::MissingTopic0)?;
    let topic0_word = decode_topic_word(topic0, 0)?;
    if topic0_word != SWAP_EVENT_TOPIC0 {
        return Err(UniswapV2PairError::InvalidTopic0 {
            expected: SWAP_EVENT_TOPIC0_HEX.to_string(),
            actual: topic0.clone(),
        });
    }

    let sender = decode_topic_address(
        log.topics
            .get(1)
            .ok_or(UniswapV2PairError::MissingTopic { index: 1 })?,
        1,
    )?;
    let to = decode_topic_address(
        log.topics
            .get(2)
            .ok_or(UniswapV2PairError::MissingTopic { index: 2 })?,
        2,
    )?;

    let data_bytes = decode_data_hex(log.data.as_str())?;
    if data_bytes.len() != 4 * 32 {
        return Err(UniswapV2PairError::InvalidDataLength {
            len: data_bytes.len(),
        });
    }

    let decoded = <SwapEventData as SolType>::abi_decode(data_bytes.as_slice())
        .map_err(|e| UniswapV2PairError::Decode(e.to_string()))?;

    Ok(UniswapV2PairSwapLog {
        sender,
        to,
        amount0_in: decoded.amount0In,
        amount1_in: decoded.amount1In,
        amount0_out: decoded.amount0Out,
        amount1_out: decoded.amount1Out,
    })
}

pub fn map_swap_to_fill(
    swap: &UniswapV2PairSwapLog,
    token0: Address,
    token1: Address,
) -> Result<UniswapV2SwapFill, UniswapV2PairError> {
    let left_to_right = !swap.amount0_in.is_zero()
        && swap.amount1_in.is_zero()
        && swap.amount0_out.is_zero()
        && !swap.amount1_out.is_zero();

    if left_to_right {
        return Ok(UniswapV2SwapFill {
            token_in: token0,
            token_out: token1,
            amount_in: swap.amount0_in,
            amount_out: swap.amount1_out,
        });
    }

    let right_to_left = swap.amount0_in.is_zero()
        && !swap.amount1_in.is_zero()
        && !swap.amount0_out.is_zero()
        && swap.amount1_out.is_zero();

    if right_to_left {
        return Ok(UniswapV2SwapFill {
            token_in: token1,
            token_out: token0,
            amount_in: swap.amount1_in,
            amount_out: swap.amount0_out,
        });
    }

    Err(UniswapV2PairError::AmbiguousSwapAmounts)
}

fn decode_topic_address(topic: &str, index: usize) -> Result<Address, UniswapV2PairError> {
    let word = decode_topic_word(topic, index)?;
    Ok(Address::from_slice(&word[12..]))
}

fn decode_topic_word(topic: &str, index: usize) -> Result<[u8; 32], UniswapV2PairError> {
    decode_word_hex(topic).map_err(|_| UniswapV2PairError::InvalidTopicHex {
        index,
        topic: topic.to_string(),
    })
}

fn decode_word_hex(input: &str) -> Result<[u8; 32], ()> {
    let bytes = decode_data_hex(input).map_err(|_| ())?;
    if bytes.len() != 32 {
        return Err(());
    }

    let mut word = [0u8; 32];
    word.copy_from_slice(bytes.as_slice());
    Ok(word)
}

fn decode_data_hex(data: &str) -> Result<Vec<u8>, UniswapV2PairError> {
    hex::decode(data.trim_start_matches("0x")).map_err(|_| UniswapV2PairError::InvalidDataHex {
        data: data.to_string(),
    })
}

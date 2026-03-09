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
    primitives::{Address, U256},
    sol,
    sol_types::SolCall,
};
use thiserror::Error;

use super::base::BaseContract;
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract PancakeSwapV2Router {
        function getAmountsOut(uint amountIn, address[] memory path) external view returns (uint[] memory amounts);
        function getAmountsIn(uint amountOut, address[] memory path) external view returns (uint[] memory amounts);
        function swapExactTokensForTokens(uint amountIn, uint amountOutMin, address[] calldata path, address to, uint deadline) external returns (uint[] memory amounts);
        function swapTokensForExactTokens(uint amountOut, uint amountInMax, address[] calldata path, address to, uint deadline) external returns (uint[] memory amounts);
    }
}

pub const SELECTOR_GET_AMOUNTS_OUT: [u8; 4] = [0xd0, 0x6c, 0xa6, 0x1f];
pub const SELECTOR_GET_AMOUNTS_IN: [u8; 4] = [0x1f, 0x00, 0xca, 0x74];
pub const SELECTOR_SWAP_EXACT_TOKENS_FOR_TOKENS: [u8; 4] = [0x38, 0xed, 0x17, 0x39];
pub const SELECTOR_SWAP_TOKENS_FOR_EXACT_TOKENS: [u8; 4] = [0x88, 0x03, 0xdb, 0xee];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PancakeSwapV2QuoteErrorCode {
    RpcRevert,
    InsufficientLiquidity,
    InsufficientAmount,
    IdenticalAddresses,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PancakeSwapV2QuoteError {
    pub code: PancakeSwapV2QuoteErrorCode,
    pub rpc_code: Option<i64>,
    pub message: String,
    pub data: Option<String>,
}

impl std::fmt::Display for PancakeSwapV2QuoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "quote error code={:?} rpc_code={:?} message={} data={:?}",
            self.code, self.rpc_code, self.message, self.data
        )
    }
}

impl std::error::Error for PancakeSwapV2QuoteError {}

#[derive(Debug, Error)]
pub enum PancakeSwapV2RouterError {
    #[error("RPC error: {0}")]
    Rpc(#[from] BlockchainRpcClientError),
    #[error("invalid path for V2 router: expected >=2 hops, got {0}")]
    InvalidPath(usize),
    #[error("invalid quote response: {0}")]
    InvalidQuoteResponse(String),
    #[error("ABI decode failure: {0}")]
    AbiDecode(String),
    #[error("{0}")]
    Quote(PancakeSwapV2QuoteError),
}

#[derive(Debug)]
pub struct PancakeSwapV2RouterContract {
    base: BaseContract,
    router_address: Address,
}

impl PancakeSwapV2RouterContract {
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>, router_address: Address) -> Self {
        Self {
            base: BaseContract::new(client),
            router_address,
        }
    }

    #[must_use]
    pub const fn router_address(&self) -> Address {
        self.router_address
    }

    pub fn encode_get_amounts_out_call(
        amount_in: U256,
        path: Vec<Address>,
    ) -> Result<Vec<u8>, PancakeSwapV2RouterError> {
        ensure_path(path.len())?;
        Ok(PancakeSwapV2Router::getAmountsOutCall {
            amountIn: amount_in,
            path,
        }
        .abi_encode())
    }

    pub fn encode_get_amounts_in_call(
        amount_out: U256,
        path: Vec<Address>,
    ) -> Result<Vec<u8>, PancakeSwapV2RouterError> {
        ensure_path(path.len())?;
        Ok(PancakeSwapV2Router::getAmountsInCall {
            amountOut: amount_out,
            path,
        }
        .abi_encode())
    }

    pub fn encode_swap_exact_tokens_for_tokens_call(
        amount_in: U256,
        amount_out_min: U256,
        path: Vec<Address>,
        to: Address,
        deadline: U256,
    ) -> Result<Vec<u8>, PancakeSwapV2RouterError> {
        ensure_path(path.len())?;
        Ok(PancakeSwapV2Router::swapExactTokensForTokensCall {
            amountIn: amount_in,
            amountOutMin: amount_out_min,
            path,
            to,
            deadline,
        }
        .abi_encode())
    }

    pub fn encode_swap_tokens_for_exact_tokens_call(
        amount_out: U256,
        amount_in_max: U256,
        path: Vec<Address>,
        to: Address,
        deadline: U256,
    ) -> Result<Vec<u8>, PancakeSwapV2RouterError> {
        ensure_path(path.len())?;
        Ok(PancakeSwapV2Router::swapTokensForExactTokensCall {
            amountOut: amount_out,
            amountInMax: amount_in_max,
            path,
            to,
            deadline,
        }
        .abi_encode())
    }

    pub fn decode_get_amounts_out_response(
        raw: &[u8],
        expected_path_len: usize,
    ) -> Result<Vec<U256>, PancakeSwapV2RouterError> {
        let amounts = PancakeSwapV2Router::getAmountsOutCall::abi_decode_returns(raw)
            .map_err(|e| PancakeSwapV2RouterError::AbiDecode(e.to_string()))?;
        ensure_amounts_response(&amounts, expected_path_len)?;
        Ok(amounts)
    }

    pub fn decode_get_amounts_in_response(
        raw: &[u8],
        expected_path_len: usize,
    ) -> Result<Vec<U256>, PancakeSwapV2RouterError> {
        let amounts = PancakeSwapV2Router::getAmountsInCall::abi_decode_returns(raw)
            .map_err(|e| PancakeSwapV2RouterError::AbiDecode(e.to_string()))?;
        ensure_amounts_response(&amounts, expected_path_len)?;
        Ok(amounts)
    }

    pub async fn quote_exact_in(
        &self,
        amount_in: U256,
        path: Vec<Address>,
    ) -> Result<Vec<U256>, PancakeSwapV2RouterError> {
        let expected_path_len = path.len();
        let call_data = Self::encode_get_amounts_out_call(amount_in, path)?;
        match self
            .base
            .execute_call(&self.router_address, call_data.as_slice(), None)
            .await
        {
            Ok(bytes) => Self::decode_get_amounts_out_response(bytes.as_slice(), expected_path_len),
            Err(e) => Err(map_quote_call_error(e)),
        }
    }

    pub async fn quote_exact_out(
        &self,
        amount_out: U256,
        path: Vec<Address>,
    ) -> Result<Vec<U256>, PancakeSwapV2RouterError> {
        let expected_path_len = path.len();
        let call_data = Self::encode_get_amounts_in_call(amount_out, path)?;
        match self
            .base
            .execute_call(&self.router_address, call_data.as_slice(), None)
            .await
        {
            Ok(bytes) => Self::decode_get_amounts_in_response(bytes.as_slice(), expected_path_len),
            Err(e) => Err(map_quote_call_error(e)),
        }
    }
}

fn ensure_path(path_len: usize) -> Result<(), PancakeSwapV2RouterError> {
    if path_len < 2 {
        return Err(PancakeSwapV2RouterError::InvalidPath(path_len));
    }
    Ok(())
}

fn ensure_amounts_response(
    amounts: &[U256],
    expected_path_len: usize,
) -> Result<(), PancakeSwapV2RouterError> {
    if amounts.len() != expected_path_len {
        return Err(PancakeSwapV2RouterError::InvalidQuoteResponse(format!(
            "expected {} amount points, got {}",
            expected_path_len,
            amounts.len(),
        )));
    }
    Ok(())
}

fn map_quote_call_error(error: BlockchainRpcClientError) -> PancakeSwapV2RouterError {
    let source = error.to_string();
    let lower = source.to_ascii_lowercase();
    if !is_revert_like(lower.as_str()) {
        return PancakeSwapV2RouterError::Rpc(error);
    }

    let code =
        if lower.contains("insufficient_liquidity") || lower.contains("insufficient liquidity") {
            PancakeSwapV2QuoteErrorCode::InsufficientLiquidity
        } else if lower.contains("insufficient_amount") || lower.contains("insufficient amount") {
            PancakeSwapV2QuoteErrorCode::InsufficientAmount
        } else if lower.contains("identical_addresses") || lower.contains("identical addresses") {
            PancakeSwapV2QuoteErrorCode::IdenticalAddresses
        } else {
            PancakeSwapV2QuoteErrorCode::RpcRevert
        };

    let quote_error = PancakeSwapV2QuoteError {
        code,
        rpc_code: extract_rpc_code(source.as_str()),
        message: extract_message(source.as_str()),
        data: extract_data(source.as_str()),
    };

    PancakeSwapV2RouterError::Quote(quote_error)
}

fn is_revert_like(lower_error: &str) -> bool {
    lower_error.contains("execution reverted")
        || lower_error.contains(" revert")
        || lower_error.contains("revert:")
        || lower_error.contains("insufficient_liquidity")
        || lower_error.contains("insufficient liquidity")
        || lower_error.contains("insufficient_amount")
        || lower_error.contains("insufficient amount")
        || lower_error.contains("identical_addresses")
        || lower_error.contains("identical addresses")
}

fn extract_rpc_code(source: &str) -> Option<i64> {
    let (_, after) = source.split_once("code=")?;
    let raw = after
        .split(|c: char| c.is_ascii_whitespace() || c == ',' || c == ')')
        .next()?;
    raw.parse::<i64>().ok()
}

fn extract_message(source: &str) -> String {
    if let Some((_, after_message)) = source.split_once("message=") {
        if let Some((message, _)) = after_message.split_once(" data=") {
            return message.trim_matches('"').trim().to_string();
        }
        return after_message.trim_matches('"').trim().to_string();
    }

    source.to_string()
}

fn extract_data(source: &str) -> Option<String> {
    let (_, data) = source.split_once(" data=")?;
    Some(data.trim_matches('"').trim().to_string())
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{U256, address};
    use alloy::sol_types::SolCall;

    use super::{
        PancakeSwapV2Router, PancakeSwapV2RouterContract, SELECTOR_GET_AMOUNTS_IN,
        SELECTOR_GET_AMOUNTS_OUT, SELECTOR_SWAP_EXACT_TOKENS_FOR_TOKENS,
        SELECTOR_SWAP_TOKENS_FOR_EXACT_TOKENS,
    };

    #[test]
    fn test_get_amounts_out_encoding_selector_d06ca61f() {
        let encoded = PancakeSwapV2RouterContract::encode_get_amounts_out_call(
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect("call encoding should succeed");

        assert_eq!(&encoded[..4], SELECTOR_GET_AMOUNTS_OUT.as_slice());
    }

    #[test]
    fn test_get_amounts_in_encoding_selector_1f00ca74() {
        let encoded = PancakeSwapV2RouterContract::encode_get_amounts_in_call(
            U256::from(500u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
        )
        .expect("call encoding should succeed");

        assert_eq!(&encoded[..4], SELECTOR_GET_AMOUNTS_IN.as_slice());
    }

    #[test]
    fn test_swap_exact_tokens_for_tokens_encoding_selector_38ed1739() {
        let encoded = PancakeSwapV2RouterContract::encode_swap_exact_tokens_for_tokens_call(
            U256::from(1_000u64),
            U256::from(900u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
            address!("0x3333333333333333333333333333333333333333"),
            U256::from(1_234_567u64),
        )
        .expect("call encoding should succeed");

        assert_eq!(
            &encoded[..4],
            SELECTOR_SWAP_EXACT_TOKENS_FOR_TOKENS.as_slice()
        );
    }

    #[test]
    fn test_swap_tokens_for_exact_tokens_encoding_selector_8803dbee() {
        let encoded = PancakeSwapV2RouterContract::encode_swap_tokens_for_exact_tokens_call(
            U256::from(900u64),
            U256::from(1_000u64),
            vec![
                address!("0x1111111111111111111111111111111111111111"),
                address!("0x2222222222222222222222222222222222222222"),
            ],
            address!("0x3333333333333333333333333333333333333333"),
            U256::from(1_234_567u64),
        )
        .expect("call encoding should succeed");

        assert_eq!(
            &encoded[..4],
            SELECTOR_SWAP_TOKENS_FOR_EXACT_TOKENS.as_slice()
        );
    }

    #[test]
    fn test_decode_get_amounts_out_response_u256_path() {
        let amounts = vec![U256::from(1_000u64), U256::from(2_500u64)];
        let encoded = PancakeSwapV2Router::getAmountsOutCall::abi_encode_returns(&amounts);

        let decoded =
            PancakeSwapV2RouterContract::decode_get_amounts_out_response(encoded.as_ref(), 2)
                .expect("decode should succeed");

        assert_eq!(decoded, amounts);
    }
}

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
    primitives::{
        Address, Bytes, U256,
        aliases::{U24, U160},
    },
    sol,
    sol_types::SolCall,
};

sol! {
    #[sol(rpc)]
    contract UniswapV3QuoterV2 {
        struct QuoteExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amountIn;
            uint24 fee;
            uint160 sqrtPriceLimitX96;
        }

        struct QuoteExactOutputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amount;
            uint24 fee;
            uint160 sqrtPriceLimitX96;
        }

        function quoteExactInputSingle(QuoteExactInputSingleParams memory params)
            external
            returns (
                uint256 amountOut,
                uint160 sqrtPriceX96After,
                uint32 initializedTicksCrossed,
                uint256 gasEstimate
            );

        function quoteExactOutputSingle(QuoteExactOutputSingleParams memory params)
            external
            returns (
                uint256 amountIn,
                uint160 sqrtPriceX96After,
                uint32 initializedTicksCrossed,
                uint256 gasEstimate
            );
    }
}

/// Exact normalized result returned by one QuoterV2 single-pool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UniswapV3Quote {
    pub amount: U256,
    pub sqrt_price_x96_after: U160,
    pub initialized_ticks_crossed: u32,
    pub gas_estimate: U256,
}

pub(crate) fn quote_exact_input_single_call(
    token_in: Address,
    token_out: Address,
    amount_in: U256,
    fee: U24,
) -> Bytes {
    Bytes::from(
        UniswapV3QuoterV2::quoteExactInputSingleCall {
            params: UniswapV3QuoterV2::QuoteExactInputSingleParams {
                tokenIn: token_in,
                tokenOut: token_out,
                amountIn: amount_in,
                fee,
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode(),
    )
}

pub(crate) fn quote_exact_output_single_call(
    token_in: Address,
    token_out: Address,
    amount_out: U256,
    fee: U24,
) -> Bytes {
    Bytes::from(
        UniswapV3QuoterV2::quoteExactOutputSingleCall {
            params: UniswapV3QuoterV2::QuoteExactOutputSingleParams {
                tokenIn: token_in,
                tokenOut: token_out,
                amount: amount_out,
                fee,
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode(),
    )
}

pub(crate) fn decode_quote_exact_input_single(result: &[u8]) -> anyhow::Result<UniswapV3Quote> {
    let result = UniswapV3QuoterV2::quoteExactInputSingleCall::abi_decode_returns(result)?;
    Ok(UniswapV3Quote {
        amount: result.amountOut,
        sqrt_price_x96_after: result.sqrtPriceX96After,
        initialized_ticks_crossed: result.initializedTicksCrossed,
        gas_estimate: result.gasEstimate,
    })
}

pub(crate) fn decode_quote_exact_output_single(result: &[u8]) -> anyhow::Result<UniswapV3Quote> {
    let result = UniswapV3QuoterV2::quoteExactOutputSingleCall::abi_decode_returns(result)?;
    Ok(UniswapV3Quote {
        amount: result.amountIn,
        sqrt_price_x96_after: result.sqrtPriceX96After,
        initialized_ticks_crossed: result.initializedTicksCrossed,
        gas_estimate: result.gasEstimate,
    })
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, hex};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn quote_exact_input_single_selector_matches_interface() {
        let call = quote_exact_input_single_call(
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
            U256::from(1_000_000u64),
            U24::try_from(500u32).unwrap(),
        );

        assert_eq!(hex::encode(&call[..4]), "c6a5026a");
    }

    #[rstest]
    fn quote_exact_output_single_selector_matches_interface() {
        let call = quote_exact_output_single_call(
            address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            U256::from(1_000_000u64),
            U24::try_from(500u32).unwrap(),
        );

        assert_eq!(hex::encode(&call[..4]), "bd21704a");
    }
}

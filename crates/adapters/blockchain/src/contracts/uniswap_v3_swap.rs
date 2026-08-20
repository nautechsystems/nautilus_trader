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

use alloy::sol;

// The original Uniswap V3 SwapRouter interface: `exactInputSingle` carries a `deadline`
// parameter, unlike the later SwapRouter02 whose struct drops it.
sol! {
    #[sol(rpc)]
    contract UniswapV3SwapRouter {
        struct ExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint24 fee;
            address recipient;
            uint256 deadline;
            uint256 amountIn;
            uint256 amountOutMinimum;
            uint160 sqrtPriceLimitX96;
        }

        function exactInputSingle(ExactInputSingleParams memory params) external payable returns (uint256 amountOut);
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        primitives::{
            U256, address,
            aliases::{U24, U160},
        },
        sol_types::SolCall,
    };
    use nautilus_core::hex;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn exact_input_single_selector_matches_canonical_signature() {
        let calldata = UniswapV3SwapRouter::exactInputSingleCall {
            params: UniswapV3SwapRouter::ExactInputSingleParams {
                tokenIn: address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                tokenOut: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
                fee: U24::try_from(500u32).unwrap(),
                recipient: address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                deadline: U256::from(1_761_889_100u64),
                amountIn: U256::from(1_000_000_000_000_000u64),
                amountOutMinimum: U256::from(1_995_000u64),
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode();

        // keccak256("exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))")
        assert_eq!(hex::encode(&calldata[..4]), "414bf389");
    }

    #[rstest]
    fn exact_input_single_encodes_fields_in_order() {
        let calldata = UniswapV3SwapRouter::exactInputSingleCall {
            params: UniswapV3SwapRouter::ExactInputSingleParams {
                tokenIn: address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                tokenOut: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
                fee: U24::try_from(500u32).unwrap(),
                recipient: address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                deadline: U256::from(1_761_889_100u64),
                amountIn: U256::from(1_000_000_000_000_000u64),
                amountOutMinimum: U256::from(1_995_000u64),
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode();

        let expected = concat!(
            "414bf389",
            "00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1",
            "000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831",
            "00000000000000000000000000000000000000000000000000000000000001f4",
            "000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "0000000000000000000000000000000000000000000000000000000069044b4c",
            "00000000000000000000000000000000000000000000000000038d7ea4c68000",
            "00000000000000000000000000000000000000000000000000000000001e70f8",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert_eq!(hex::encode(&calldata), expected);
    }
}

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
    primitives::{Address, aliases::U24},
    sol,
    sol_types::SolCall,
};

use super::base::BaseContract;
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

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

sol! {
    #[sol(rpc)]
    contract UniswapV3RouterState {
        function factory() external view returns (address);
        function WETH9() external view returns (address);
    }

    #[sol(rpc)]
    contract UniswapV3Factory {
        function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool);
    }
}

/// Reads immutable deployment relationships used to authorize Uniswap V3 execution.
#[derive(Debug)]
pub struct UniswapV3Deployment {
    base: BaseContract,
}

impl UniswapV3Deployment {
    /// Creates a deployment reader with an optional per-request timeout.
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>, rpc_timeout_secs: Option<u64>) -> Self {
        Self {
            base: BaseContract::new_with_multicall_limit_and_timeout(
                client,
                super::base::DEFAULT_MULTICALL_CALLS_PER_RPC_REQUEST,
                rpc_timeout_secs,
            ),
        }
    }

    /// Reads the factory configured by the router.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result cannot be decoded as an address.
    pub async fn router_factory(
        &self,
        router: &Address,
    ) -> Result<Address, BlockchainRpcClientError> {
        self.router_factory_with_block(router, None).await
    }

    async fn router_factory_with_block(
        &self,
        router: &Address,
        block: Option<u64>,
    ) -> Result<Address, BlockchainRpcClientError> {
        let result = self
            .base
            .execute_call(
                router,
                &UniswapV3RouterState::factoryCall {}.abi_encode(),
                block,
            )
            .await?;
        UniswapV3RouterState::factoryCall::abi_decode_returns(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
    }

    /// Reads the wrapped-native-token address configured by the router.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result cannot be decoded as an address.
    pub async fn router_weth9(
        &self,
        router: &Address,
    ) -> Result<Address, BlockchainRpcClientError> {
        self.router_weth9_with_block(router, None).await
    }

    async fn router_weth9_with_block(
        &self,
        router: &Address,
        block: Option<u64>,
    ) -> Result<Address, BlockchainRpcClientError> {
        let result = self
            .base
            .execute_call(
                router,
                &UniswapV3RouterState::WETH9Call {}.abi_encode(),
                block,
            )
            .await?;
        UniswapV3RouterState::WETH9Call::abi_decode_returns(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
    }

    /// Resolves the canonical pool registered by a factory for a token pair and fee tier.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or the result cannot be decoded as an address.
    pub async fn pool(
        &self,
        factory: &Address,
        token_a: Address,
        token_b: Address,
        fee: U24,
    ) -> Result<Address, BlockchainRpcClientError> {
        self.pool_with_block(factory, token_a, token_b, fee, None)
            .await
    }

    async fn pool_with_block(
        &self,
        factory: &Address,
        token_a: Address,
        token_b: Address,
        fee: U24,
        block: Option<u64>,
    ) -> Result<Address, BlockchainRpcClientError> {
        let call = UniswapV3Factory::getPoolCall {
            tokenA: token_a,
            tokenB: token_b,
            fee,
        };
        let result = self
            .base
            .execute_call(factory, &call.abi_encode(), block)
            .await?;
        UniswapV3Factory::getPoolCall::abi_decode_returns(&result)
            .map_err(|e| BlockchainRpcClientError::AbiDecodingError(e.to_string()))
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

    #[rstest]
    fn deployment_selectors_match_canonical_signatures() {
        assert_eq!(
            hex::encode(UniswapV3RouterState::factoryCall {}.abi_encode()),
            "c45a0155"
        );
        assert_eq!(
            hex::encode(UniswapV3RouterState::WETH9Call {}.abi_encode()),
            "4aa4a4fc"
        );
        let calldata = UniswapV3Factory::getPoolCall {
            tokenA: address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            tokenB: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
            fee: U24::try_from(500u32).unwrap(),
        }
        .abi_encode();
        assert_eq!(hex::encode(&calldata[..4]), "1698ee82");
    }
}

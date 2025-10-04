// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, U160, U256, keccak256},
    sol,
    sol_types::{SolCall, private::primitives::aliases::I24},
};
use nautilus_model::defi::{pool_analysis::position::PoolPosition, tick_map::tick::Tick};
use thiserror::Error;

use super::base::{BaseContract, ContractCall};
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    #[sol(rpc)]
    contract UniswapV3Pool {
        /// Packed struct containing core pool state
        struct Slot0Return {
            uint160 sqrtPriceX96;
            int24 tick;
            uint16 observationIndex;
            uint16 observationCardinality;
            uint16 observationCardinalityNext;
            uint8 feeProtocol;
            bool unlocked;
        }

        /// Tick information
        struct TickInfoReturn {
            uint128 liquidityGross;
            int128 liquidityNet;
            uint256 feeGrowthOutside0X128;
            uint256 feeGrowthOutside1X128;
            int56 tickCumulativeOutside;
            uint160 secondsPerLiquidityOutsideX128;
            uint32 secondsOutside;
            bool initialized;
        }

        /// Position information
        struct PositionInfoReturn {
            uint128 liquidity;
            uint256 feeGrowthInside0LastX128;
            uint256 feeGrowthInside1LastX128;
            uint128 tokensOwed0;
            uint128 tokensOwed1;
        }

        // Core state getters
        function slot0() external view returns (Slot0Return memory);
        function liquidity() external view returns (uint128);
        function feeGrowthGlobal0X128() external view returns (uint256);
        function feeGrowthGlobal1X128() external view returns (uint256);

        // Tick and position getters
        function ticks(int24 tick) external view returns (TickInfoReturn memory);
        function positions(bytes32 key) external view returns (PositionInfoReturn memory);
    }
}

/// Combined global state of a Uniswap V3 pool.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolGlobalState {
    /// Current sqrt price
    pub sqrt_price_x96: U160,
    /// Current tick
    pub tick: i32,
    /// Current liquidity
    pub liquidity: u128,
    /// Global fee growth for token0
    pub fee_growth_global_0_x128: U256,
    /// Global fee growth for token1
    pub fee_growth_global_1_x128: U256,
    /// Protocol fee setting
    pub fee_protocol: u8,
}

/// Represents errors that can occur when interacting with UniswapV3Pool contract.
#[derive(Debug, Error)]
pub enum UniswapV3PoolError {
    #[error("RPC error: {0}")]
    RpcError(#[from] BlockchainRpcClientError),
    #[error("Failed to decode {field} for pool {pool}: {reason} (raw data: {raw_data})")]
    DecodingError {
        field: String,
        pool: Address,
        reason: String,
        raw_data: String,
    },
    #[error("Call failed for {field} at pool {pool}: {reason}")]
    CallFailed {
        field: String,
        pool: Address,
        reason: String,
    },
    #[error("Tick {tick} is not initialized in pool {pool}")]
    TickNotInitialized { tick: i32, pool: Address },
}

/// Interface for interacting with UniswapV3Pool contracts on a blockchain.
///
/// This struct provides methods to query pool state including slot0, liquidity,
/// fee growth, tick data, and position data. Supports both single calls and
/// batch multicalls for efficiency.
#[derive(Debug)]
pub struct UniswapV3PoolContract {
    /// The base contract providing common RPC execution functionality.
    base: BaseContract,
}

impl UniswapV3PoolContract {
    /// Creates a new UniswapV3Pool contract interface with the specified RPC client.
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>) -> Self {
        Self {
            base: BaseContract::new(client),
        }
    }

    /// Gets all global state in a single multicall.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall fails or any decoding fails.
    pub async fn get_global_state(
        &self,
        pool_address: &Address,
    ) -> Result<PoolGlobalState, UniswapV3PoolError> {
        let calls = vec![
            ContractCall {
                target: *pool_address,
                allow_failure: false,
                call_data: UniswapV3Pool::slot0Call {}.abi_encode(),
            },
            ContractCall {
                target: *pool_address,
                allow_failure: false,
                call_data: UniswapV3Pool::liquidityCall {}.abi_encode(),
            },
            ContractCall {
                target: *pool_address,
                allow_failure: false,
                call_data: UniswapV3Pool::feeGrowthGlobal0X128Call {}.abi_encode(),
            },
            ContractCall {
                target: *pool_address,
                allow_failure: false,
                call_data: UniswapV3Pool::feeGrowthGlobal1X128Call {}.abi_encode(),
            },
        ];

        let results = self.base.execute_multicall(calls).await?;

        if results.len() != 4 {
            return Err(UniswapV3PoolError::CallFailed {
                field: "global_state_multicall".to_string(),
                pool: *pool_address,
                reason: format!("Expected 4 results, got {}", results.len()),
            });
        }

        // Decode slot0
        let slot0 =
            UniswapV3Pool::slot0Call::abi_decode_returns(&results[0].returnData).map_err(|e| {
                UniswapV3PoolError::DecodingError {
                    field: "slot0".to_string(),
                    pool: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&results[0].returnData),
                }
            })?;

        // Decode liquidity
        let liquidity = UniswapV3Pool::liquidityCall::abi_decode_returns(&results[1].returnData)
            .map_err(|e| UniswapV3PoolError::DecodingError {
                field: "liquidity".to_string(),
                pool: *pool_address,
                reason: e.to_string(),
                raw_data: hex::encode(&results[1].returnData),
            })?;

        // Decode feeGrowthGlobal0X128
        let fee_growth_0 =
            UniswapV3Pool::feeGrowthGlobal0X128Call::abi_decode_returns(&results[2].returnData)
                .map_err(|e| UniswapV3PoolError::DecodingError {
                    field: "feeGrowthGlobal0X128".to_string(),
                    pool: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&results[2].returnData),
                })?;

        // Decode feeGrowthGlobal1X128
        let fee_growth_1 =
            UniswapV3Pool::feeGrowthGlobal1X128Call::abi_decode_returns(&results[3].returnData)
                .map_err(|e| UniswapV3PoolError::DecodingError {
                    field: "feeGrowthGlobal1X128".to_string(),
                    pool: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&results[3].returnData),
                })?;

        Ok(PoolGlobalState {
            sqrt_price_x96: slot0.sqrtPriceX96,
            tick: slot0.tick.as_i32(),
            liquidity,
            fee_growth_global_0_x128: fee_growth_0,
            fee_growth_global_1_x128: fee_growth_1,
            fee_protocol: slot0.feeProtocol,
        })
    }

    /// Gets tick data for a specific tick.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC call fails or decoding fails.
    pub async fn get_tick(
        &self,
        pool_address: &Address,
        tick: i32,
    ) -> Result<Tick, UniswapV3PoolError> {
        let tick_i24 = I24::try_from(tick).map_err(|_| UniswapV3PoolError::CallFailed {
            field: "tick".to_string(),
            pool: *pool_address,
            reason: format!("Tick {} out of range for int24", tick),
        })?;

        let call_data = UniswapV3Pool::ticksCall { tick: tick_i24 }.abi_encode();
        let raw_response = self.base.execute_call(pool_address, &call_data).await?;

        let tick_info =
            UniswapV3Pool::ticksCall::abi_decode_returns(&raw_response).map_err(|e| {
                UniswapV3PoolError::DecodingError {
                    field: format!("ticks({})", tick),
                    pool: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&raw_response),
                }
            })?;

        Ok(Tick::new(
            tick,
            tick_info.liquidityGross,
            tick_info.liquidityNet,
            tick_info.feeGrowthOutside0X128,
            tick_info.feeGrowthOutside1X128,
            tick_info.initialized,
            0, // last_updated_block - not available from RPC
        ))
    }

    /// Gets tick data for multiple ticks in a single multicall.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall fails or if any tick decoding fails.
    /// Uninitialized ticks are silently skipped (not included in the result HashMap).
    pub async fn batch_get_ticks(
        &self,
        pool_address: &Address,
        ticks: &[i32],
    ) -> Result<HashMap<i32, Tick>, UniswapV3PoolError> {
        let calls: Vec<ContractCall> = ticks
            .iter()
            .filter_map(|&tick| {
                I24::try_from(tick).ok().map(|tick_i24| ContractCall {
                    target: *pool_address,
                    allow_failure: true,
                    call_data: UniswapV3Pool::ticksCall { tick: tick_i24 }.abi_encode(),
                })
            })
            .collect();

        let results = self.base.execute_multicall(calls).await?;

        let mut tick_infos = HashMap::with_capacity(ticks.len());
        for (i, &tick_value) in ticks.iter().enumerate() {
            if i >= results.len() {
                break;
            }

            let result = &results[i];
            if !result.success {
                // Skip uninitialized ticks
                continue;
            }

            let tick_info = UniswapV3Pool::ticksCall::abi_decode_returns(&result.returnData)
                .map_err(|e| UniswapV3PoolError::DecodingError {
                    field: format!("ticks({})", tick_value),
                    pool: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&result.returnData),
                })?;

            tick_infos.insert(
                tick_value,
                Tick::new(
                    tick_value,
                    tick_info.liquidityGross,
                    tick_info.liquidityNet,
                    tick_info.feeGrowthOutside0X128,
                    tick_info.feeGrowthOutside1X128,
                    tick_info.initialized,
                    0, // last_updated_block - not available from RPC
                ),
            );
        }

        Ok(tick_infos)
    }

    /// Computes the position key used by Uniswap V3.
    ///
    /// The key is: keccak256(abi.encodePacked(owner, tickLower, tickUpper))
    #[must_use]
    pub fn compute_position_key(owner: &Address, tick_lower: i32, tick_upper: i32) -> [u8; 32] {
        // Pack: address (20 bytes) + int24 (3 bytes) + int24 (3 bytes) = 26 bytes total
        let mut packed = Vec::with_capacity(26);

        // Add owner address (20 bytes)
        packed.extend_from_slice(owner.as_slice());

        // Add tick_lower as int24 (3 bytes, big-endian, sign-extended)
        let tick_lower_bytes = tick_lower.to_be_bytes();
        packed.extend_from_slice(&tick_lower_bytes[1..4]);

        // Add tick_upper as int24 (3 bytes, big-endian, sign-extended)
        let tick_upper_bytes = tick_upper.to_be_bytes();
        packed.extend_from_slice(&tick_upper_bytes[1..4]);

        keccak256(&packed).into()
    }

    /// Gets position data for multiple positions in a single multicall.
    ///
    /// # Errors
    ///
    /// Returns an error if the multicall fails. Individual position failures are
    /// captured in the Result values of the returned Vec.
    pub async fn batch_get_positions(
        &self,
        pool_address: &Address,
        positions: &[(Address, i32, i32)],
    ) -> Result<Vec<PoolPosition>, UniswapV3PoolError> {
        let calls: Vec<ContractCall> = positions
            .iter()
            .map(|(owner, tick_lower, tick_upper)| {
                let position_key = Self::compute_position_key(owner, *tick_lower, *tick_upper);
                ContractCall {
                    target: *pool_address,
                    allow_failure: true,
                    call_data: UniswapV3Pool::positionsCall {
                        key: position_key.into(),
                    }
                    .abi_encode(),
                }
            })
            .collect();

        let results = self.base.execute_multicall(calls).await?;

        let position_infos: Vec<PoolPosition> = positions
            .iter()
            .enumerate()
            .filter_map(|(i, (owner, tick_lower, tick_upper))| {
                if i >= results.len() {
                    return None;
                }

                let result = &results[i];
                if !result.success {
                    return None;
                }

                UniswapV3Pool::positionsCall::abi_decode_returns(&result.returnData)
                    .ok()
                    .map(|info| PoolPosition {
                        owner: *owner,
                        tick_lower: *tick_lower,
                        tick_upper: *tick_upper,
                        liquidity: info.liquidity,
                        fee_growth_inside_0_last: info.feeGrowthInside0LastX128,
                        fee_growth_inside_1_last: info.feeGrowthInside1LastX128,
                        tokens_owed_0: info.tokensOwed0,
                        tokens_owed_1: info.tokensOwed1,
                        total_amount0_deposited: U256::ZERO,
                        total_amount1_deposited: U256::ZERO,
                        total_amount0_collected: 0,
                        total_amount1_collected: 0,
                    })
            })
            .collect();

        Ok(position_infos)
    }
}

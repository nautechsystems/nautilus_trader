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

use std::sync::Arc;

use alloy::{primitives::Address, sol, sol_types::SolCall};
use nautilus_model::defi::{Blockchain, tick_map::TickMap, validation::validate_address};
use thiserror::Error;

use super::base::BaseContract;
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    struct SolidityTick {
        int24 index;
        uint128 liquidityGross;
        int128 liquidityNet;
    }

    contract UniswapV3Lens {
        function getAllTicks(address pool) external view returns (SolidityTick[] memory ticks);
    }
}

/// UniswapV3Lens contract address on Arbitrum
pub const ARBITRUM_UNISWAP_V3_LENS_ADDRESS: &str = "0xf632a03754090B44B605C0bA417Fffe369E26397";

/// Represents the tick data from a Uniswap V3 pool.
#[derive(Debug, Clone, PartialEq)]
pub struct UniswapV3Tick {
    /// The tick index.
    pub index: i32,
    /// The liquidity available at this tick.
    pub liquidity_gross: u128,
    /// The net liquidity change at this tick.
    pub liquidity_net: i128,
}

/// Represents errors that can occur when interacting with UniswapV3Lens contract.
#[derive(Debug, Error)]
pub enum UniswapV3LensError {
    #[error("RPC error: {0}")]
    RpcError(#[from] BlockchainRpcClientError),
    #[error("Failed to decode response for pool {address}: {reason} (raw data: {raw_data})")]
    DecodingError {
        address: Address,
        reason: String,
        raw_data: String,
    },
}

/// Interface for interacting with UniswapV3Lens contracts on a blockchain.
///
/// This struct provides methods to fetch all tick data from Uniswap V3 pools.
/// Currently configured for Arbitrum's UniswapV3Lens deployment.
#[derive(Debug)]
pub struct UniswapV3LensContract {
    /// The base contract providing common RPC execution functionality.
    base: BaseContract,
    /// The lens contract address (hardcoded for Arbitrum).
    lens_address: Address,
}

impl UniswapV3LensContract {
    /// Creates a new UniswapV3Lens contract interface with the specified RPC client and chain.
    ///
    /// # Panics
    ///
    /// Panics if the chain is not Arbitrum, as UniswapV3Lens is only deployed on Arbitrum.
    /// Also panics if the lens address is invalid.
    #[must_use]
    pub fn new(client: Arc<BlockchainHttpRpcClient>, chain: Blockchain) -> Self {
        if chain != Blockchain::Arbitrum {
            panic!(
                "UniswapV3Lens is only supported on Arbitrum, got: {:?}",
                chain
            );
        }

        let lens_address = validate_address(ARBITRUM_UNISWAP_V3_LENS_ADDRESS)
            .expect("Invalid UniswapV3Lens address");

        Self {
            base: BaseContract::new(client),
            lens_address,
        }
    }

    /// Gets all ticks from a Uniswap V3 pool.
    ///
    /// This method calls the `getAllTicks` function on the UniswapV3Lens contract
    /// to retrieve all active ticks in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails or if decoding fails.
    pub async fn get_all_ticks(
        &self,
        pool_address: &Address,
    ) -> Result<Vec<UniswapV3Tick>, UniswapV3LensError> {
        let call_data = UniswapV3Lens::getAllTicksCall {
            pool: *pool_address,
        }
        .abi_encode();

        let raw_response = self
            .base
            .execute_call(&self.lens_address, &call_data)
            .await?;

        let response =
            UniswapV3Lens::getAllTicksCall::abi_decode_returns(&raw_response).map_err(|e| {
                UniswapV3LensError::DecodingError {
                    address: *pool_address,
                    reason: e.to_string(),
                    raw_data: hex::encode(&raw_response),
                }
            })?;

        Ok(response
            .into_iter()
            .map(|tick| UniswapV3Tick {
                index: tick.index.as_i32(),
                liquidity_gross: tick.liquidityGross,
                liquidity_net: tick.liquidityNet,
            })
            .collect())
    }

    pub async fn compare_tick_maps(
        &self,
        pool_address: &Address,
        tick_map: &TickMap,
    ) -> Result<bool, UniswapV3LensError> {
        let current_ticks = self.get_all_ticks(pool_address).await?;
        let mut matching_ticks = 0;

        // Check that current ticks exist in the target tickmaps.
        for on_chain_tick in &current_ticks {
            let tick_index = on_chain_tick.index;
            if let Some(target_tick) = tick_map.get_tick(tick_index) {
                if target_tick.is_active() {
                    if target_tick.liquidity_gross == on_chain_tick.liquidity_gross
                        && target_tick.liquidity_net == on_chain_tick.liquidity_net
                    {
                        matching_ticks += 1;
                    } else {
                        tracing::error!(
                            "Tick {} doesnt have matching liquidity gross: {} vs {} or liquidity net: {} vs {}",
                            tick_index,
                            target_tick.liquidity_gross,
                            on_chain_tick.liquidity_gross,
                            target_tick.liquidity_net,
                            on_chain_tick.liquidity_net
                        );
                    }
                } else {
                    tracing::error!(
                        "Tick {} is not active on-chain but found in the target tick maps",
                        tick_index
                    );
                }
            } else {
                tracing::error!(
                    "Tick {} exists on-chain but not found in the target tick maps",
                    tick_index
                );
            }
        }

        // Check for the ticks that exist in the target tickmaps but not on-chain.as
        let mut our_extra_ticks = 0;
        for (tick_index, our_tick) in tick_map.get_all_ticks().iter() {
            if our_tick.is_active() {
                let found_on_chain = current_ticks.iter().any(|t| t.index == *tick_index);
                if !found_on_chain {
                    our_extra_ticks += 1;
                    log::error!(
                        "Tick {} is active in our maps but not found on-chain (liquidity: {})",
                        tick_index,
                        our_tick.liquidity_gross
                    );
                }
            }
        }

        Ok(matching_ticks == current_ticks.len() && our_extra_ticks == 0)
    }
}

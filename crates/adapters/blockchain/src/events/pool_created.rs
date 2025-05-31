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

use alloy::primitives::Address;

/// Represents a liquidity pool creation event from a decentralized exchange.
///
// This struct models the data structure of a pool creation event emitted by DEX factory contracts.
#[derive(Debug, Clone)]
pub struct PoolCreated {
    /// The block number when the pool was created.
    pub block_number: u64,
    /// The blockchain address of the first token in the pair.
    pub token0: Address,
    /// The blockchain address of the second token in the pair.
    pub token1: Address,
    /// The fee tier of the pool, specified in basis points (e.g., 500 = 0.05%, 3000 = 0.3%).
    pub fee: u32,
    /// The tick spacing parameter that controls the granularity of price ranges.
    pub tick_spacing: u32,
    /// The blockchain address of the created liquidity pool contract.
    pub pool_address: Address,
}

impl PoolCreated {
    /// Creates a new [`PoolCreated`] instance with the specified parameters.
    #[must_use]
    pub const fn new(
        block_number: u64,
        token0: Address,
        token1: Address,
        fee: u32,
        tick_spacing: u32,
        pool_address: Address,
    ) -> Self {
        Self {
            block_number,
            token0,
            token1,
            fee,
            tick_spacing,
            pool_address,
        }
    }
}

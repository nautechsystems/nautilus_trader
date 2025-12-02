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

use alloy::primitives::{Address, U160};
use nautilus_model::defi::PoolIdentifier;

/// Represents a liquidity pool creation event from a decentralized exchange.
///
// This struct models the data structure of a pool creation event emitted by DEX factory contracts.
#[derive(Debug, Clone)]
pub struct PoolCreatedEvent {
    /// The block number when the pool was created.
    pub block_number: u64,
    /// The blockchain address of the first token in the pair.
    pub token0: Address,
    /// The blockchain address of the second token in the pair.
    pub token1: Address,
    /// The blockchain address of the created liquidity pool contract.
    /// For V2/V3: the pool contract address
    /// For V4: the PoolManager contract address
    pub pool_address: Address,
    /// The unique identifier for this pool.
    pub pool_identifier: PoolIdentifier,
    /// The fee tier of the pool, specified in basis points (e.g., 500 = 0.05%, 3000 = 0.3%).
    pub fee: Option<u32>,
    /// The tick spacing parameter that controls the granularity of price ranges.
    pub tick_spacing: Option<u32>,
    /// The square root of the price ratio encoded as a fixed point number with 96 fractional bits.
    pub sqrt_price_x96: Option<U160>,
    /// The current tick of the pool.
    pub tick: Option<i32>,
    /// The hooks contract address for Uniswap V4 pools.
    pub hooks: Option<Address>,
}

impl PoolCreatedEvent {
    /// Creates a new [`PoolCreatedEvent`] instance with the specified parameters.
    #[must_use]
    pub fn new(
        block_number: u64,
        token0: Address,
        token1: Address,
        pool_address: Address,
        pool_identifier: PoolIdentifier,
        fee: Option<u32>,
        tick_spacing: Option<u32>,
    ) -> Self {
        Self {
            block_number,
            token0,
            token1,
            pool_address,
            pool_identifier,
            fee,
            tick_spacing,
            sqrt_price_x96: None,
            tick: None,
            hooks: None,
        }
    }

    /// Sets the initialization parameters for the pool after it has been initialized.
    pub fn set_initialize_params(&mut self, sqrt_price_x96: U160, tick: i32) {
        self.sqrt_price_x96 = Some(sqrt_price_x96);
        self.tick = Some(tick);
    }

    /// Sets the hooks contract address for this pool.
    ///
    /// This is typically called for Uniswap V4 pools that have hooks enabled.
    pub fn set_hooks(&mut self, hooks: Address) {
        self.hooks = Some(hooks);
    }
}

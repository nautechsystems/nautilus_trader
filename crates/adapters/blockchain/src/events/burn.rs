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

use alloy::primitives::{Address, U256};

/// Represents a burn event that occurs when liquidity is removed from a position in a liquidity pool.
#[derive(Debug, Clone)]
pub struct BurnEvent {
    /// The block number when the burn occurred.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
    /// The owner of the position.
    pub owner: Address,
    /// The lower tick boundary of the position.
    pub tick_lower: i32,
    /// The upper tick boundary of the position.
    pub tick_upper: i32,
    /// The amount of liquidity burned to the position range.
    pub amount: u128,
    /// The amount of token0 withdrawn.
    pub amount0: U256,
    /// The amount of token1 withdrawn.
    pub amount1: U256,
}

impl BurnEvent {
    /// Creates a new [`BurnEvent`] instance with the specified parameters.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        owner: Address,
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
        amount0: U256,
        amount1: U256,
    ) -> Self {
        Self {
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            owner,
            tick_lower,
            tick_upper,
            amount,
            amount0,
            amount1,
        }
    }
}

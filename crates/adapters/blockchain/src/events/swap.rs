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

use alloy::primitives::{Address, I256, U160};

/// Represents a token swap event from liquidity pools emitted from smart contract.
///
/// This struct captures the essential data from a swap transaction on decentralized
/// exchanges (DEXs) that use automated market maker (AMM) protocols.
#[derive(Debug, Clone)]
pub struct SwapEvent {
    /// The block number in which this swap transaction was included.
    pub block_number: u64,
    /// The address that initiated the swap transaction.
    pub sender: Address,
    /// The address that received the swapped tokens.
    pub receiver: Address,
    /// The amount of token0 involved in the swap.
    /// Negative values indicate tokens flowing out of the pool, positive values indicate tokens flowing in.
    pub amount0: I256,
    /// The amount of token1 involved in the swap.
    /// Negative values indicate tokens flowing out of the pool, positive values indicate tokens flowing in.
    pub amount1: I256,
    /// The square root of the price ratio encoded as a Q64.96 fixed-point number.
    /// This represents the price of token1 in terms of token0 after the swap.
    pub sqrt_price_x96: U160,
}

impl SwapEvent {
    /// Creates a new [`SwapEvent`] instance with the specified parameters.
    pub fn new(
        block_number: u64,
        sender: Address,
        receiver: Address,
        amount0: I256,
        amount1: I256,
        sqrt_price_x96: U160,
    ) -> Self {
        Self {
            block_number,
            sender,
            receiver,
            amount0,
            amount1,
            sqrt_price_x96,
        }
    }
}

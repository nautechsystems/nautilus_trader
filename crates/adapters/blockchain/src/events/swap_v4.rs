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

//! Uniswap v4 Swap event.

use alloy::primitives::{Address, FixedBytes, I128, U160};
use nautilus_model::defi::SharedDex;

/// Represents a token swap event from Uniswap v4 PoolManager.
///
/// This extends the base swap event with a fee field, which in v4
/// can be dynamic based on hooks.
#[derive(Debug, Clone)]
pub struct SwapV4Event {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The pool ID (keccak256 hash of PoolKey).
    pub pool_id: FixedBytes<32>,
    /// The block number in which this swap was included.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
    /// The address that initiated the swap.
    pub sender: Address,
    /// The delta of currency0 balance (negative = out of pool).
    pub amount0: I128,
    /// The delta of currency1 balance (negative = out of pool).
    pub amount1: I128,
    /// The sqrt price after the swap (Q64.96 format).
    pub sqrt_price_x96: U160,
    /// The pool liquidity after the swap.
    pub liquidity: u128,
    /// The current tick after the swap.
    pub tick: i32,
    /// The swap fee in hundredths of a bip.
    pub fee: u32,
}

impl SwapV4Event {
    /// Creates a new [`SwapV4Event`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        dex: SharedDex,
        pool_id: FixedBytes<32>,
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Address,
        amount0: I128,
        amount1: I128,
        sqrt_price_x96: U160,
        liquidity: u128,
        tick: i32,
        fee: u32,
    ) -> Self {
        Self {
            dex,
            pool_id,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
            fee,
        }
    }

    /// Returns the fee as a percentage (e.g., 0.3% = 3000 bips).
    #[must_use]
    pub const fn fee_bips(&self) -> u32 {
        self.fee
    }

    /// Returns the fee as a fraction (e.g., 0.003 for 0.3%).
    #[must_use]
    pub fn fee_fraction(&self) -> f64 {
        f64::from(self.fee) / 1_000_000.0
    }
}

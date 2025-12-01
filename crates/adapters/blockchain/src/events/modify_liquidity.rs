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

//! Uniswap v4 ModifyLiquidity event.

use alloy::primitives::{Address, FixedBytes, I256};
use nautilus_model::defi::SharedDex;

/// Event emitted when liquidity is modified in a Uniswap v4 pool.
///
/// This event replaces the separate Mint/Burn events from v3.
/// Positive liquidityDelta indicates adding liquidity, negative indicates removing.
#[derive(Debug, Clone)]
pub struct ModifyLiquidityEvent {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The pool ID (keccak256 hash of PoolKey).
    pub pool_id: FixedBytes<32>,
    /// The block number in which this event was included.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
    /// The address that modified the liquidity.
    pub sender: Address,
    /// The lower tick of the position.
    pub tick_lower: i32,
    /// The upper tick of the position.
    pub tick_upper: i32,
    /// The amount of liquidity added (positive) or removed (negative).
    pub liquidity_delta: I256,
    /// The salt used to make the position unique.
    pub salt: FixedBytes<32>,
}

impl ModifyLiquidityEvent {
    /// Creates a new [`ModifyLiquidityEvent`] instance.
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
        tick_lower: i32,
        tick_upper: i32,
        liquidity_delta: I256,
        salt: FixedBytes<32>,
    ) -> Self {
        Self {
            dex,
            pool_id,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            tick_lower,
            tick_upper,
            liquidity_delta,
            salt,
        }
    }

    /// Returns true if this event represents adding liquidity.
    #[must_use]
    pub fn is_add(&self) -> bool {
        self.liquidity_delta > I256::ZERO
    }

    /// Returns true if this event represents removing liquidity.
    #[must_use]
    pub fn is_remove(&self) -> bool {
        self.liquidity_delta < I256::ZERO
    }
}

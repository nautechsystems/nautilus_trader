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

//! Uniswap v4 Initialize event.

use alloy::primitives::{Address, FixedBytes, U160};
use nautilus_model::defi::SharedDex;

/// Event emitted when a new pool is initialized in the v4 PoolManager.
///
/// Unlike v3 where pools are separate contracts, v4 pools are identified
/// by their PoolKey hash (pool_id) within the singleton PoolManager.
#[derive(Debug, Clone)]
pub struct InitializeV4Event {
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
    /// The address of currency0 (lower by address sort).
    pub currency0: Address,
    /// The address of currency1 (higher by address sort).
    pub currency1: Address,
    /// The pool fee in hundredths of a bip.
    pub fee: u32,
    /// The tick spacing.
    pub tick_spacing: i32,
    /// The hooks contract address (or zero address if none).
    pub hooks: Address,
    /// The initial sqrt price (Q64.96 format).
    pub sqrt_price_x96: U160,
    /// The initial tick.
    pub tick: i32,
}

impl InitializeV4Event {
    /// Creates a new [`InitializeV4Event`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        dex: SharedDex,
        pool_id: FixedBytes<32>,
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
        sqrt_price_x96: U160,
        tick: i32,
    ) -> Self {
        Self {
            dex,
            pool_id,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks,
            sqrt_price_x96,
            tick,
        }
    }

    /// Returns true if this pool has hooks enabled.
    #[must_use]
    pub fn has_hooks(&self) -> bool {
        !self.hooks.is_zero()
    }
}

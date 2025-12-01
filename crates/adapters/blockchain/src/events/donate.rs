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

//! Uniswap v4 Donate event.

use alloy::primitives::{Address, FixedBytes, U256};
use nautilus_model::defi::SharedDex;

/// Event emitted when tokens are donated to in-range liquidity providers.
///
/// Donations go directly to LPs currently in range and can be used
/// for incentive programs or protocol distributions.
#[derive(Debug, Clone)]
pub struct DonateEvent {
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
    /// The address that donated the tokens.
    pub sender: Address,
    /// The amount of currency0 donated.
    pub amount0: U256,
    /// The amount of currency1 donated.
    pub amount1: U256,
}

impl DonateEvent {
    /// Creates a new [`DonateEvent`] instance.
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
        amount0: U256,
        amount1: U256,
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
        }
    }
}

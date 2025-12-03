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
use nautilus_core::UnixNanos;
use nautilus_model::{
    defi::{PoolIdentifier, SharedChain, SharedDex, data::PoolFlash},
    identifiers::InstrumentId,
};

/// Represents a flash loan event from liquidity pools emitted from smart contract.
///
/// This struct captures the essential data from a flash loan transaction on decentralized
/// exchanges (DEXs) that support flash loans.
#[derive(Debug, Clone)]
pub struct FlashEvent {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The unique identifier for the pool.
    pub pool_identifier: PoolIdentifier,
    /// The block number in which this flash loan transaction was included.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
    /// The address that initiated the flash loan transaction.
    pub sender: Address,
    /// The address that received the flash loan.
    pub recipient: Address,
    /// The amount of token0 borrowed.
    pub amount0: U256,
    /// The amount of token1 borrowed.
    pub amount1: U256,
    /// The amount of token0 paid back (including fees).
    pub paid0: U256,
    /// The amount of token1 paid back (including fees).
    pub paid1: U256,
}

impl FlashEvent {
    /// Creates a new [`FlashEvent`] instance with the specified parameters.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dex: SharedDex,
        pool_identifier: PoolIdentifier,
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Address,
        recipient: Address,
        amount0: U256,
        amount1: U256,
        paid0: U256,
        paid1: U256,
    ) -> Self {
        Self {
            dex,
            pool_identifier,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            recipient,
            amount0,
            amount1,
            paid0,
            paid1,
        }
    }

    /// Converts a flash event into a `PoolFlash`.
    #[must_use]
    pub fn to_pool_flash(
        &self,
        chain: SharedChain,
        instrument_id: InstrumentId,
        timestamp: Option<UnixNanos>,
    ) -> PoolFlash {
        PoolFlash::new(
            chain,
            self.dex.clone(),
            instrument_id,
            self.pool_identifier,
            self.block_number,
            self.transaction_hash.clone(),
            self.transaction_index,
            self.log_index,
            timestamp,
            self.sender,
            self.recipient,
            self.amount0,
            self.amount1,
            self.paid0,
            self.paid1,
        )
    }
}

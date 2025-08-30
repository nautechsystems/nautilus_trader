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
    defi::{PoolLiquidityUpdate, PoolLiquidityUpdateType, SharedChain, SharedDex},
    identifiers::InstrumentId,
    types::Quantity,
};

/// Represents a mint event that occurs when liquidity is added to a position in a liquidity pool.
#[derive(Debug, Clone)]
pub struct MintEvent {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The address of the smart contract which emitted the event.
    pub pool_address: Address,
    /// The block number when the mint occurred.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
    /// The address that sent the transaction.
    pub sender: Address,
    /// The owner of the position.
    pub owner: Address,
    /// The lower tick boundary of the position.
    pub tick_lower: i32,
    /// The upper tick boundary of the position.
    pub tick_upper: i32,
    /// The amount of liquidity minted.
    pub amount: u128,
    /// The amount of token0 deposited.
    pub amount0: U256,
    /// The amount of token1 deposited.
    pub amount1: U256,
}

impl MintEvent {
    /// Creates a new [`MintEvent`] instance with the specified parameters.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        dex: SharedDex,
        pool_address: Address,
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Address,
        owner: Address,
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
        amount0: U256,
        amount1: U256,
    ) -> Self {
        Self {
            dex,
            pool_address,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            owner,
            tick_lower,
            tick_upper,
            amount,
            amount0,
            amount1,
        }
    }

    /// Converts a mint event into a `PoolLiquidityUpdate`.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn to_pool_liquidity_update(
        &self,
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: Address,
        liquidity: Quantity,
        amount0: Quantity,
        amount1: Quantity,
        timestamp: Option<UnixNanos>,
    ) -> PoolLiquidityUpdate {
        PoolLiquidityUpdate::new(
            chain,
            dex,
            instrument_id,
            pool_address,
            PoolLiquidityUpdateType::Mint,
            self.block_number,
            self.transaction_hash.clone(),
            self.transaction_index,
            self.log_index,
            Some(self.sender),
            self.owner,
            liquidity,
            amount0,
            amount1,
            self.tick_lower,
            self.tick_upper,
            timestamp,
        )
    }
}

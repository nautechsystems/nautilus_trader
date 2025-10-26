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
use nautilus_core::UnixNanos;
use nautilus_model::{
    defi::{PoolSwap, SharedChain, SharedDex},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// Represents a token swap event from liquidity pools emitted from smart contract.
///
/// This struct captures the essential data from a swap transaction on decentralized
/// exchanges (DEXs) that use automated market maker (AMM) protocols.
#[derive(Debug, Clone)]
pub struct SwapEvent {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The address of the smart contract which emitted the event.
    pub pool_address: Address,
    /// The block number in which this swap transaction was included.
    pub block_number: u64,
    /// The unique hash identifier of the transaction containing this event.
    pub transaction_hash: String,
    /// The position of this transaction within the block.
    pub transaction_index: u32,
    /// The position of this event log within the transaction.
    pub log_index: u32,
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
    /// The liquidity of the pool after the swap occurred.
    pub liquidity: u128,
    /// The current tick of the pool after the swap occurred.
    pub tick: i32,
}

impl SwapEvent {
    /// Creates a new [`SwapEvent`] instance with the specified parameters.
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
        receiver: Address,
        amount0: I256,
        amount1: I256,
        sqrt_price_x96: U160,
        liquidity: u128,
        tick: i32,
    ) -> Self {
        Self {
            dex,
            pool_address,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            receiver,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
        }
    }

    /// Converts a swap event into a `PoolSwap`.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn to_pool_swap(
        &self,
        chain: SharedChain,
        instrument_id: InstrumentId,
        pool_address: Address,
        normalized_side: Option<OrderSide>,
        normalized_quantity: Option<Quantity>,
        normalized_price: Option<Price>,
        timestamp: Option<UnixNanos>,
    ) -> PoolSwap {
        PoolSwap::new(
            chain,
            self.dex.clone(),
            instrument_id,
            pool_address,
            self.block_number,
            self.transaction_hash.clone(),
            self.transaction_index,
            self.log_index,
            timestamp,
            self.sender,
            self.receiver,
            self.amount0,
            self.amount1,
            self.sqrt_price_x96,
            self.liquidity,
            self.tick,
            normalized_side,
            normalized_quantity,
            normalized_price,
        )
    }
}

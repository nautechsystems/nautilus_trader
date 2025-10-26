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

use std::fmt::Display;

use alloy_primitives::{Address, I256, U160};
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    defi::{SharedChain, SharedDex},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// Represents a token swap transaction on a decentralized exchange (DEX).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolSwap {
    /// The blockchain network where the swap occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the swap was executed.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The blockchain address of the pool smart contract.
    pub pool_address: Address,
    /// The blockchain block number at which the swap was executed.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the swap.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the swap event log within the transaction.
    pub log_index: u32,
    /// The blockchain address of the user or contract that initiated the swap.
    pub sender: Address,
    /// The blockchain address that received the swapped tokens.
    pub recipient: Address,
    /// The sqrt price after the swap (Q64.96 format).
    pub sqrt_price_x96: U160,
    /// The amount of token0 involved in the swap.
    pub amount0: I256,
    /// The amount of token1 involved in the swap.
    pub amount1: I256,
    /// The liquidity of the pool after the swap occurred.
    pub liquidity: u128,
    /// The current tick of the pool after the swap occurred.
    pub tick: i32,
    /// The direction of the swap from the perspective of the base token.
    pub side: Option<OrderSide>,
    /// The amount of tokens swapped.
    pub size: Option<Quantity>,
    /// The exchange rate at which the swap occurred.
    pub price: Option<Price>,
    /// UNIX timestamp (nanoseconds) when the swap occurred.
    pub timestamp: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: Option<UnixNanos>,
}

impl PoolSwap {
    /// Creates a new [`PoolSwap`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: Address,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        timestamp: Option<UnixNanos>,
        sender: Address,
        recipient: Address,
        amount0: I256,
        amount1: I256,
        sqrt_price_x96: U160,
        liquidity: u128,
        tick: i32,
        side: Option<OrderSide>,
        size: Option<Quantity>,
        price: Option<Price>,
    ) -> Self {
        Self {
            chain,
            dex,
            instrument_id,
            pool_address,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            timestamp,
            sender,
            recipient,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
            side,
            size,
            price,
            ts_init: timestamp, // TODO: Use swap timestamp as init timestamp for now
        }
    }
}

impl Display for PoolSwap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={})",
            stringify!(PoolSwap),
            self.instrument_id,
        )
    }
}

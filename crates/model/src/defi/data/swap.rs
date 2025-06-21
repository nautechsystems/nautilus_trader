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

use alloy_primitives::Address;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    data::HasTsInit,
    defi::{amm::SharedPool, chain::SharedChain, dex::SharedDex},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// Represents a token swap transaction on a decentralized exchange (DEX).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolSwap {
    /// The blockchain network where the swap occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the swap was executed.
    pub dex: SharedDex,
    /// The DEX liquidity pool.
    pub pool: SharedPool,
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
    /// The direction of the swap from the perspective of the base token.
    pub side: OrderSide,
    /// The amount of tokens swapped.
    pub size: Quantity,
    /// The exchange rate at which the swap occurred.
    pub price: Price,
    /// UNIX timestamp (nanoseconds) when the swap occurred.
    pub timestamp: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl PoolSwap {
    /// Creates a new [`PoolSwap`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        pool: SharedPool,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        timestamp: UnixNanos,
        sender: Address,
        side: OrderSide,
        size: Quantity,
        price: Price,
    ) -> Self {
        Self {
            chain,
            dex,
            pool,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            timestamp,
            sender,
            side,
            size,
            price,
            ts_init: timestamp, // TODO: Use swap timestamp as init timestamp for now
        }
    }

    /// Returns the instrument ID for this swap.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.pool.instrument_id()
    }
}

impl HasTsInit for PoolSwap {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Display for PoolSwap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(chain={}, dex={}, pool={}, side={}, quantity={}, price={})",
            stringify!(PoolSwap),
            self.chain.name,
            self.dex.name,
            self.pool.ticker(),
            self.side,
            self.size,
            self.price,
        )
    }
}

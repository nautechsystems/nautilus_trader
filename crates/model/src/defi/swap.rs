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

use crate::{
    defi::{amm::SharedPool, chain::SharedChain, dex::SharedDex},
    enums::OrderSide,
    types::{Price, Quantity},
};

/// Represents a token swap transaction on a decentralized exchange (DEX).
#[derive(Debug, Clone)]
pub struct Swap {
    /// The blockchain network where the swap occurred
    pub chain: SharedChain,
    /// The decentralized exchange where the swap was executed
    pub dex: SharedDex,
    /// The DEX liquidity pool
    pub pool: SharedPool,
    /// The blockchain block number at which the swap was executed.
    pub block: u64,
    /// The blockchain address of the user or contract that initiated the swap.
    pub sender: Address,
    /// The direction of the swap from the perspective of the base token.
    pub side: OrderSide,
    /// The amount of tokens being swapped
    pub quantity: Quantity,
    /// The exchange rate at which the swap occurred.
    pub price: Price,
    /// The timestamp of the swap in Unix nanoseconds.
    pub timestamp: UnixNanos,
}

impl Swap {
    /// Creates a new [`Swap`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        pool: SharedPool,
        block: u64,
        timestamp: UnixNanos,
        sender: Address,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
    ) -> Self {
        Self {
            chain,
            dex,
            pool,
            block,
            timestamp,
            sender,
            side,
            quantity,
            price,
        }
    }
}

impl Display for Swap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Swap(chain={}, dex={}, pool={}, side={}, quantity={}, price={})",
            self.chain.name,
            self.dex.name,
            self.pool.ticker(),
            self.side,
            self.quantity,
            self.price,
        )
    }
}

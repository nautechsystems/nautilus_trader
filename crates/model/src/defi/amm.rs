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

use crate::defi::{chain::SharedChain, dex::Dex, token::Token};

/// Represents a liquidity pool in a decentralized exchange.
#[derive(Debug, Clone)]
pub struct Pool {
    /// The blockchain network where this pool exists.
    pub chain: SharedChain,
    /// The decentralized exchange protocol that created and manages this pool.
    pub dex: Dex,
    /// The blockchain address of the pool smart contract.
    pub address: String,
    /// The block number when this pool was created on the blockchain.
    pub creation_block: u64,
    /// The first token in the trading pair.
    pub token0: Token,
    /// The second token in the trading pair.
    pub token1: Token,
    /// The trading fee charged by the pool, denominated in basis points.
    pub fee: u32,
    /// The minimum tick spacing for positions in concentrated liquidity AMMs.
    pub tick_spacing: u32,
}

impl Pool {
    /// Creates a new [`Pool`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: Dex,
        address: String,
        creation_block: u64,
        token0: Token,
        token1: Token,
        fee: u32,
        tick_spacing: u32,
    ) -> Self {
        Self {
            chain,
            dex,
            address,
            creation_block,
            token0,
            token1,
            fee,
            tick_spacing,
        }
    }
}

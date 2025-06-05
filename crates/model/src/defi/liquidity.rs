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

use alloy_primitives::Address;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::{
    defi::{amm::SharedPool, chain::SharedChain, dex::SharedDex},
    types::Quantity,
};

#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    PartialOrd,
    PartialEq,
    Ord,
    Eq,
    Display,
    EnumString,
    Serialize,
    Deserialize,
)]
/// Represents the type of liquidity update operation in a DEX pool.
pub enum PoolLiquidityUpdateType {
    /// Liquidity is being added to the pool
    Mint,
    /// Liquidity is being removed from the pool
    Burn,
}

/// Represents a liquidity update event in a decentralized exchange (DEX) pool.
#[derive(Debug, Clone)]
pub struct PoolLiquidityUpdate {
    /// The blockchain network where the liquidity update occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the liquidity update was executed.
    pub dex: SharedDex,
    /// The DEX liquidity pool
    pub pool: SharedPool,
    /// The type of the pool liquidity update.
    pub kind: PoolLiquidityUpdateType,
    /// The blockchain block number where the liquidity update occurred.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the liquidity update.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the liquidity update event log within the transaction.
    pub log_index: u32,
    /// The blockchain address that initiated the liquidity update transaction.
    pub sender: Option<Address>,
    /// The blockchain address that owns the liquidity position.
    pub owner: Address,
    /// The amount of liquidity tokens affected in the position.
    pub position_liquidity: Quantity,
    /// The amount of the first token in the pool pair.
    pub amount0: Quantity,
    /// The amount of the second token in the pool pair.
    pub amount1: Quantity,
    /// The lower price tick boundary of the liquidity position.
    pub tick_lower: i32,
    /// The upper price tick boundary of the liquidity position.
    pub tick_upper: i32,
    /// The timestamp of the liquidity update in Unix nanoseconds.
    pub timestamp: UnixNanos,
}

impl PoolLiquidityUpdate {
    /// Creates a new [`PoolLiquidityUpdate`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        chain: SharedChain,
        dex: SharedDex,
        pool: SharedPool,
        kind: PoolLiquidityUpdateType,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Option<Address>,
        owner: Address,
        position_liquidity: Quantity,
        amount0: Quantity,
        amount1: Quantity,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: UnixNanos,
    ) -> Self {
        Self {
            chain,
            dex,
            pool,
            kind,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            owner,
            position_liquidity,
            amount0,
            amount1,
            tick_lower,
            tick_upper,
            timestamp,
        }
    }
}

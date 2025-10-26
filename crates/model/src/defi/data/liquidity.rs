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

use alloy_primitives::{Address, U256};
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use crate::{
    defi::{SharedChain, SharedDex},
    identifiers::InstrumentId,
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
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
/// Represents the type of liquidity update operation in a DEX pool.
#[non_exhaustive]
pub enum PoolLiquidityUpdateType {
    /// Liquidity is being added to the pool
    Mint,
    /// Liquidity is being removed from the pool
    Burn,
}

/// Represents a liquidity update event in a decentralized exchange (DEX) pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolLiquidityUpdate {
    /// The blockchain network where the liquidity update occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the liquidity update was executed.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The blockchain address of the pool smart contract.
    pub pool_address: Address,
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
    pub position_liquidity: u128,
    /// The amount of the first token in the pool pair.
    pub amount0: U256,
    /// The amount of the second token in the pool pair.
    pub amount1: U256,
    /// The lower price tick boundary of the liquidity position.
    pub tick_lower: i32,
    /// The upper price tick boundary of the liquidity position.
    pub tick_upper: i32,
    /// The timestamp of the liquidity update in Unix nanoseconds.
    pub timestamp: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: Option<UnixNanos>,
}

impl PoolLiquidityUpdate {
    /// Creates a new [`PoolLiquidityUpdate`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: Address,
        kind: PoolLiquidityUpdateType,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Option<Address>,
        owner: Address,
        position_liquidity: u128,
        amount0: U256,
        amount1: U256,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: Option<UnixNanos>,
    ) -> Self {
        Self {
            chain,
            dex,
            instrument_id,
            pool_address,
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
            ts_init: timestamp,
        }
    }
}

impl Display for PoolLiquidityUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolLiquidityUpdate(instrument_id={}, kind={}, amount0={}, amount1={}, liquidity={})",
            self.instrument_id, self.kind, self.amount0, self.amount1, self.position_liquidity
        )
    }
}

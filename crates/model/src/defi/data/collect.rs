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
    defi::{SharedChain, SharedDex},
    identifiers::InstrumentId,
    types::Quantity,
};

/// Represents a fee collection event in a decentralized exchange (DEX) pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolFeeCollect {
    /// The blockchain network where the fee collection occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the fee collection was executed.
    pub dex: SharedDex,
    /// The DEX liquidity pool instrument ID.
    pub instrument_id: InstrumentId,
    /// The blockchain address of the pool smart contract.
    pub pool_address: Address,
    /// The blockchain block number where the fee collection occurred.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the fee collection.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the fee collection event log within the transaction.
    pub log_index: u32,
    /// The blockchain address that owns the liquidity position.
    pub owner: Address,
    /// The amount of the first token fees collected.
    pub fee0: Quantity,
    /// The amount of the second token fees collected.
    pub fee1: Quantity,
    /// The lower price tick boundary of the liquidity position.
    pub tick_lower: i32,
    /// The upper price tick boundary of the liquidity position.
    pub tick_upper: i32,
    /// The timestamp of the fee collection in Unix nanoseconds.
    pub timestamp: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: Option<UnixNanos>,
}

impl PoolFeeCollect {
    /// Creates a new [`PoolFeeCollect`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: Address,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        owner: Address,
        fee0: Quantity,
        fee1: Quantity,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: Option<UnixNanos>,
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
            owner,
            fee0,
            fee1,
            tick_lower,
            tick_upper,
            timestamp,
            ts_init: timestamp,
        }
    }
}

impl Display for PoolFeeCollect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolFeeCollect({}:{} {} fees collected: token0={}, token1={}, owner={}, tick_range=[{}, {}], tx={}:{}:{})",
            self.chain.name,
            self.dex.name,
            self.instrument_id,
            self.fee0,
            self.fee1,
            self.owner,
            self.tick_lower,
            self.tick_upper,
            self.block,
            self.transaction_index,
            self.log_index,
        )
    }
}

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

use crate::{
    defi::{SharedChain, SharedDex},
    identifiers::InstrumentId,
};

/// Represents a flash loan event from a Uniswap V3 pool.
///
/// Flash loans allow users to borrow tokens without collateral as long as they are returned
/// within the same transaction. Fees are paid on the borrowed amount, which are added to
/// the pool's fee growth accumulators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolFlash {
    /// The blockchain network where the flash loan occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the flash loan was executed.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The blockchain address of the pool smart contract.
    pub pool_address: Address,
    /// The blockchain block number at which the flash loan was executed.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the flash loan.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the flash loan event log within the transaction.
    pub log_index: u32,
    /// The UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: Option<UnixNanos>,
    /// The blockchain address of the user or contract that initiated the flash loan.
    pub sender: Address,
    /// The blockchain address that received the flash loan.
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

impl PoolFlash {
    /// Creates a new [`PoolFlash`] instance with the specified parameters.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: Address,
        block_number: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        ts_event: Option<UnixNanos>,
        sender: Address,
        recipient: Address,
        amount0: U256,
        amount1: U256,
        paid0: U256,
        paid1: U256,
    ) -> Self {
        Self {
            chain,
            dex,
            instrument_id,
            pool_address,
            block: block_number,
            transaction_hash,
            transaction_index,
            log_index,
            ts_event,
            sender,
            recipient,
            amount0,
            amount1,
            paid0,
            paid1,
        }
    }
}

impl Display for PoolFlash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolFlash(instrument={}, recipient={}, amount0={}, amount1={}, paid0={}, paid1={})",
            self.instrument_id, self.recipient, self.amount0, self.amount1, self.paid0, self.paid1,
        )
    }
}

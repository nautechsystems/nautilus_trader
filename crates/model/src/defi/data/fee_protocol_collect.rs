// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    defi::{PoolIdentifier, SharedChain, SharedDex},
    identifiers::InstrumentId,
};

/// Represents a protocol-fee withdrawal from a Uniswap V3-style pool.
///
/// Emitted by `CollectProtocol`, this carries the protocol-fee amounts withdrawn to the recipient.
/// The amounts decrement the pool's accrued protocol-fee balances, leaving the on-chain remainder
/// (Uniswap V3 keeps one wei in each slot to save gas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PoolFeeProtocolCollect {
    /// The blockchain network where the protocol-fee withdrawal occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the protocol-fee withdrawal occurred.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The unique identifier for this pool (could be an address or other protocol-specific hex string).
    pub pool_identifier: PoolIdentifier,
    /// The blockchain block number where the protocol-fee withdrawal occurred.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the protocol-fee withdrawal.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the protocol-fee withdrawal event log within the transaction.
    pub log_index: u32,
    /// The address that initiated the withdrawal (the factory owner).
    pub sender: Address,
    /// The address that received the withdrawn protocol fees.
    pub recipient: Address,
    /// The amount of token0 protocol fees withdrawn.
    pub amount0: u128,
    /// The amount of token1 protocol fees withdrawn.
    pub amount1: u128,
    /// UNIX timestamp (nanoseconds) when the protocol-fee withdrawal event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl PoolFeeProtocolCollect {
    /// Creates a new [`PoolFeeProtocolCollect`] instance with the specified properties.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub const fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Address,
        recipient: Address,
        amount0: u128,
        amount1: u128,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            chain,
            dex,
            instrument_id,
            pool_identifier,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            recipient,
            amount0,
            amount1,
            ts_event,
            ts_init,
        }
    }
}

impl Display for PoolFeeProtocolCollect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolFeeProtocolCollect({} protocol fees withdrawn: token0={}, token1={}, recipient={}, tx={}:{}:{})",
            self.instrument_id,
            self.amount0,
            self.amount1,
            self.recipient,
            self.block,
            self.transaction_index,
            self.log_index,
        )
    }
}

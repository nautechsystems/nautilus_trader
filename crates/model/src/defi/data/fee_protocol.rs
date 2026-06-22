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

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    defi::{PoolIdentifier, SharedChain, SharedDex},
    identifiers::InstrumentId,
};

/// Represents a protocol-fee configuration change in a Uniswap V3-style pool.
///
/// Emitted by `SetFeeProtocol`, this carries the new protocol-fee denominators for each token.
/// Only the new values are kept; the previous values in the event are not needed to rebuild state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PoolFeeProtocolUpdate {
    /// The blockchain network where the protocol-fee change occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the protocol-fee change occurred.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The unique identifier for this pool (could be an address or other protocol-specific hex string).
    pub pool_identifier: PoolIdentifier,
    /// The blockchain block number where the protocol-fee change occurred.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the protocol-fee change.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the protocol-fee change event log within the transaction.
    pub log_index: u32,
    /// The new protocol-fee denominator for token0 (lower nibble of the packed `fee_protocol`).
    pub fee_protocol0_new: u8,
    /// The new protocol-fee denominator for token1 (upper nibble of the packed `fee_protocol`).
    pub fee_protocol1_new: u8,
    /// UNIX timestamp (nanoseconds) when the protocol-fee change event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl PoolFeeProtocolUpdate {
    /// Creates a new [`PoolFeeProtocolUpdate`] instance with the specified properties.
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
        fee_protocol0_new: u8,
        fee_protocol1_new: u8,
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
            fee_protocol0_new,
            fee_protocol1_new,
            ts_event,
            ts_init,
        }
    }

    /// Returns the new protocol-fee setting packed into a single byte, matching `slot0.feeProtocol`.
    ///
    /// The token0 denominator occupies the lower four bits and token1 the upper four bits.
    #[must_use]
    pub const fn packed(&self) -> u8 {
        self.fee_protocol0_new | (self.fee_protocol1_new << 4)
    }
}

impl Display for PoolFeeProtocolUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolFeeProtocolUpdate({}, fee_protocol0_new={}, fee_protocol1_new={}, tx={}:{}:{})",
            self.instrument_id,
            self.fee_protocol0_new,
            self.fee_protocol1_new,
            self.block,
            self.transaction_index,
            self.log_index,
        )
    }
}

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

//! DeFi (Decentralized Finance) data models and types.
//!
//! This module provides core data structures for working with decentralized finance protocols,
//! including blockchain networks, tokens, liquidity pools, swaps, and other DeFi primitives.

use std::fmt::Display;

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{data::HasTsInit, defi::Pool, identifiers::InstrumentId};

pub mod block;
pub mod liquidity;
pub mod swap;
pub mod transaction;

// Re-exports
pub use block::Block;
pub use liquidity::{PoolLiquidityUpdate, PoolLiquidityUpdateType};
pub use swap::PoolSwap;
pub use transaction::Transaction;

/// Represents DeFi-specific data events in a decentralized exchange ecosystem.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DefiData {
    /// A block completion in a blockchain network.
    Block(Block),
    /// A DEX liquidity pool definition or update.
    Pool(Pool),
    /// A token swap transaction on a decentralized exchange.
    PoolSwap(PoolSwap),
    /// A liquidity update event (mint/burn) in a DEX pool.
    PoolLiquidityUpdate(PoolLiquidityUpdate),
}

impl DefiData {
    /// Returns the instrument ID associated with this DeFi data.
    ///
    /// # Panics
    ///
    /// Panics if the variant is a `Block` where instrument IDs are not applicable.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Block(_) => panic!("`InstrumentId` not applicable to `Block`"), // TBD?
            Self::PoolSwap(swap) => swap.instrument_id,
            Self::PoolLiquidityUpdate(update) => update.instrument_id,
            Self::Pool(pool) => pool.instrument_id,
        }
    }
}

impl Display for DefiData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block(b) => write!(f, "{b}"),
            Self::PoolSwap(s) => write!(f, "{s}"),
            Self::PoolLiquidityUpdate(u) => write!(f, "{u}"),
            Self::Pool(p) => write!(f, "{p}"),
        }
    }
}

impl HasTsInit for DefiData {
    fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Block(block) => block.timestamp, // TODO: TBD
            Self::PoolSwap(swap) => swap.ts_init,
            Self::PoolLiquidityUpdate(update) => update.ts_init,
            Self::Pool(pool) => pool.ts_init,
        }
    }
}

impl From<PoolSwap> for DefiData {
    fn from(value: PoolSwap) -> Self {
        Self::PoolSwap(value)
    }
}

impl From<PoolLiquidityUpdate> for DefiData {
    fn from(value: PoolLiquidityUpdate) -> Self {
        Self::PoolLiquidityUpdate(value)
    }
}

impl From<Pool> for DefiData {
    fn from(value: Pool) -> Self {
        Self::Pool(value)
    }
}

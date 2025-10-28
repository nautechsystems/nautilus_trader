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

use serde::{Deserialize, Serialize};

use crate::{
    defi::{Pool, pool_analysis::snapshot::PoolSnapshot},
    identifiers::InstrumentId,
};

pub mod block;
pub mod collect;
pub mod flash;
pub mod liquidity;
pub mod swap;
pub mod transaction;

// Re-exports
pub use block::Block;
pub use collect::PoolFeeCollect;
pub use flash::PoolFlash;
pub use liquidity::{PoolLiquidityUpdate, PoolLiquidityUpdateType};
pub use swap::PoolSwap;
pub use transaction::Transaction;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DexPoolData {
    Swap(PoolSwap),
    LiquidityUpdate(PoolLiquidityUpdate),
    FeeCollect(PoolFeeCollect),
    Flash(PoolFlash),
}

impl DexPoolData {
    /// Returns the block number associated with this pool event.
    #[must_use]
    pub fn block_number(&self) -> u64 {
        match self {
            Self::Swap(s) => s.block,
            Self::LiquidityUpdate(u) => u.block,
            Self::FeeCollect(c) => c.block,
            Self::Flash(f) => f.block,
        }
    }

    /// Returns the transaction index associated with this pool event.
    #[must_use]
    pub fn transaction_index(&self) -> u32 {
        match self {
            Self::Swap(s) => s.transaction_index,
            Self::LiquidityUpdate(u) => u.transaction_index,
            Self::FeeCollect(c) => c.transaction_index,
            Self::Flash(f) => f.transaction_index,
        }
    }

    /// Returns the log index associated with this pool event.
    #[must_use]
    pub fn log_index(&self) -> u32 {
        match self {
            Self::Swap(s) => s.log_index,
            Self::LiquidityUpdate(u) => u.log_index,
            Self::FeeCollect(c) => c.log_index,
            Self::Flash(f) => f.log_index,
        }
    }
}

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
    /// A complete snapshot of a pool's state at a specific point in time.
    PoolSnapshot(PoolSnapshot),
    /// A token swap transaction on a decentralized exchange.
    PoolSwap(PoolSwap),
    /// A liquidity update event (mint/burn) in a DEX pool.
    PoolLiquidityUpdate(PoolLiquidityUpdate),
    /// A fee collection event from a DEX pool position.
    PoolFeeCollect(PoolFeeCollect),
    /// A flash event
    PoolFlash(PoolFlash),
}

impl DefiData {
    /// Returns the instrument ID associated with this DeFi data.
    ///
    /// # Panics
    ///
    /// Panics if the variant is a `Block` or `PoolSnapshot` where instrument IDs are not applicable.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Block(_) => panic!("`InstrumentId` not applicable to `Block`"), // TBD?
            Self::PoolSnapshot(snapshot) => snapshot.instrument_id,
            Self::PoolSwap(swap) => swap.instrument_id,
            Self::PoolLiquidityUpdate(update) => update.instrument_id,
            Self::PoolFeeCollect(collect) => collect.instrument_id,
            Self::Pool(pool) => pool.instrument_id,
            Self::PoolFlash(flash) => flash.instrument_id,
        }
    }
}

impl Display for DefiData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block(b) => write!(f, "{b}"),
            Self::Pool(p) => write!(f, "{p}"),
            Self::PoolSnapshot(s) => write!(f, "PoolSnapshot(block={})", s.block_position.number),
            Self::PoolSwap(s) => write!(f, "{s}"),
            Self::PoolLiquidityUpdate(u) => write!(f, "{u}"),
            Self::PoolFeeCollect(c) => write!(f, "{c}"),
            Self::PoolFlash(p) => write!(f, "{p}"),
        }
    }
}

impl From<Pool> for DefiData {
    fn from(value: Pool) -> Self {
        Self::Pool(value)
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

impl From<PoolFeeCollect> for DefiData {
    fn from(value: PoolFeeCollect) -> Self {
        Self::PoolFeeCollect(value)
    }
}

impl From<PoolSnapshot> for DefiData {
    fn from(value: PoolSnapshot) -> Self {
        Self::PoolSnapshot(value)
    }
}

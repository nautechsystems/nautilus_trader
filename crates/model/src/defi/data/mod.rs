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

//! DeFi (Decentralized Finance) data models and types.
//!
//! This module provides core data structures for working with decentralized finance protocols,
//! including blockchain networks, tokens, liquidity pools, swaps, and other DeFi primitives.

use std::fmt::Display;

use nautilus_core::UnixNanos;

use crate::{
    data::HasTsInit,
    defi::{Pool, pool_analysis::PoolSnapshot},
    identifiers::InstrumentId,
};

pub mod block;
pub mod collect;
pub mod fee_protocol_collect;
pub mod fee_protocol_update;
pub mod flash;
pub mod liquidity;
pub mod swap;
pub mod swap_trade_info;
pub mod transaction;

// Re-exports
pub use block::Block;
pub use collect::PoolFeeCollect;
pub use fee_protocol_collect::PoolFeeProtocolCollect;
pub use fee_protocol_update::PoolFeeProtocolUpdate;
pub use flash::PoolFlash;
pub use liquidity::{PoolLiquidityUpdate, PoolLiquidityUpdateType};
pub use swap::PoolSwap;
pub use transaction::Transaction;

#[derive(Debug, Clone, PartialEq)]
pub enum DexPoolData {
    Swap(PoolSwap),
    LiquidityUpdate(PoolLiquidityUpdate),
    FeeCollect(PoolFeeCollect),
    FeeProtocolUpdate(PoolFeeProtocolUpdate),
    FeeProtocolCollect(PoolFeeProtocolCollect),
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
            Self::FeeProtocolUpdate(u) => u.block,
            Self::FeeProtocolCollect(c) => c.block,
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
            Self::FeeProtocolUpdate(u) => u.transaction_index,
            Self::FeeProtocolCollect(c) => c.transaction_index,
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
            Self::FeeProtocolUpdate(u) => u.log_index,
            Self::FeeProtocolCollect(c) => c.log_index,
            Self::Flash(f) => f.log_index,
        }
    }
}

/// Represents DeFi-specific data events in a decentralized exchange ecosystem.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
#[derive(Debug, Clone, PartialEq)]
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
    /// A protocol-fee configuration change in a DEX pool.
    PoolFeeProtocolUpdate(PoolFeeProtocolUpdate),
    /// A protocol-fee withdrawal from a DEX pool.
    PoolFeeProtocolCollect(PoolFeeProtocolCollect),
    /// A flash event
    PoolFlash(PoolFlash),
}

impl DefiData {
    /// Returns the block position associated with this DeFi data.
    #[must_use]
    pub fn block_position(&self) -> (u64, u32, u32) {
        match self {
            Self::Block(block) => (block.number, 0, 0),
            Self::Pool(pool) => (pool.creation_block, 0, 0),
            Self::PoolSnapshot(snapshot) => (
                snapshot.block_position.number,
                snapshot.block_position.transaction_index,
                snapshot.block_position.log_index,
            ),
            Self::PoolSwap(swap) => (swap.block, swap.transaction_index, swap.log_index),
            Self::PoolLiquidityUpdate(update) => {
                (update.block, update.transaction_index, update.log_index)
            }
            Self::PoolFeeCollect(collect) => {
                (collect.block, collect.transaction_index, collect.log_index)
            }
            Self::PoolFeeProtocolUpdate(update) => {
                (update.block, update.transaction_index, update.log_index)
            }
            Self::PoolFeeProtocolCollect(collect) => {
                (collect.block, collect.transaction_index, collect.log_index)
            }
            Self::PoolFlash(flash) => (flash.block, flash.transaction_index, flash.log_index),
        }
    }

    /// Returns the block number associated with this DeFi data.
    #[must_use]
    pub fn block_number(&self) -> u64 {
        self.block_position().0
    }

    /// Returns the transaction index associated with this DeFi data.
    #[must_use]
    pub fn transaction_index(&self) -> u32 {
        self.block_position().1
    }

    /// Returns the log index associated with this DeFi data.
    #[must_use]
    pub fn log_index(&self) -> u32 {
        self.block_position().2
    }

    /// Returns the event timestamp associated with this DeFi data.
    #[must_use]
    pub fn ts_event(&self) -> UnixNanos {
        match self {
            Self::Block(block) => block.timestamp,
            Self::Pool(pool) => pool.ts_event,
            Self::PoolSnapshot(snapshot) => snapshot.ts_event,
            Self::PoolSwap(swap) => swap.ts_event,
            Self::PoolLiquidityUpdate(update) => update.ts_event,
            Self::PoolFeeCollect(collect) => collect.ts_event,
            Self::PoolFeeProtocolUpdate(update) => update.ts_event,
            Self::PoolFeeProtocolCollect(collect) => collect.ts_event,
            Self::PoolFlash(flash) => flash.ts_event,
        }
    }

    /// Returns the event timestamp associated with this DeFi data.
    #[must_use]
    pub fn timestamp(&self) -> UnixNanos {
        self.ts_event()
    }

    /// Returns the initialization timestamp associated with this DeFi data.
    #[must_use]
    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Block(block) => block.timestamp,
            Self::Pool(pool) => pool.ts_init,
            Self::PoolSnapshot(snapshot) => snapshot.ts_init,
            Self::PoolSwap(swap) => swap.ts_init,
            Self::PoolLiquidityUpdate(update) => update.ts_init,
            Self::PoolFeeCollect(collect) => collect.ts_init,
            Self::PoolFeeProtocolUpdate(update) => update.ts_init,
            Self::PoolFeeProtocolCollect(collect) => collect.ts_init,
            Self::PoolFlash(flash) => flash.ts_init,
        }
    }

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
            Self::PoolFeeProtocolUpdate(update) => update.instrument_id,
            Self::PoolFeeProtocolCollect(collect) => collect.instrument_id,
            Self::Pool(pool) => pool.instrument_id,
            Self::PoolFlash(flash) => flash.instrument_id,
        }
    }
}

impl HasTsInit for DefiData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init()
    }
}

impl HasTsInit for Block {
    fn ts_init(&self) -> UnixNanos {
        self.timestamp
    }
}

impl HasTsInit for PoolSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolSwap {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolLiquidityUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolFeeCollect {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolFeeProtocolUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolFeeProtocolCollect {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PoolFlash {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
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
            Self::PoolFeeProtocolUpdate(u) => write!(f, "{u}"),
            Self::PoolFeeProtocolCollect(c) => write!(f, "{c}"),
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

impl From<PoolFeeProtocolUpdate> for DefiData {
    fn from(value: PoolFeeProtocolUpdate) -> Self {
        Self::PoolFeeProtocolUpdate(value)
    }
}

impl From<PoolFeeProtocolCollect> for DefiData {
    fn from(value: PoolFeeProtocolCollect) -> Self {
        Self::PoolFeeProtocolCollect(value)
    }
}

impl From<PoolSnapshot> for DefiData {
    fn from(value: PoolSnapshot) -> Self {
        Self::PoolSnapshot(value)
    }
}

impl From<PoolFlash> for DefiData {
    fn from(value: PoolFlash) -> Self {
        Self::PoolFlash(value)
    }
}

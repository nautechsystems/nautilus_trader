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

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{data::GetTsInit, identifiers::InstrumentId};

pub mod amm;
pub mod block;
pub mod liquidity;
pub mod swap;
pub mod transaction;

// Re-exports
pub use amm::{Pool, SharedPool};
pub use block::Block;
pub use liquidity::{PoolLiquidityUpdate, PoolLiquidityUpdateType};
pub use swap::Swap;
pub use transaction::Transaction;

/// Represents DeFi-specific data events in a decentralized exchange ecosystem.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DefiData {
    Block(Block),
    /// A token swap transaction on a decentralized exchange.
    Swap(Swap),
    /// A liquidity update event (mint/burn) in a DEX pool.
    PoolLiquidityUpdate(PoolLiquidityUpdate),
    /// A DEX liquidity pool definition or update.
    Pool(Pool),
}

impl DefiData {
    /// Returns the instrument ID associated with this DeFi data.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Block(_) => todo!("Not implemented yet"),
            Self::Swap(swap) => swap.instrument_id(),
            Self::PoolLiquidityUpdate(update) => update.instrument_id(),
            Self::Pool(pool) => pool.instrument_id(),
        }
    }
}

impl GetTsInit for DefiData {
    fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Block(block) => block.timestamp,
            Self::Swap(swap) => swap.ts_init,
            Self::PoolLiquidityUpdate(update) => update.ts_init,
            Self::Pool(pool) => pool.ts_init,
        }
    }
}

impl From<Swap> for DefiData {
    fn from(value: swap::Swap) -> Self {
        Self::Swap(value)
    }
}

impl From<PoolLiquidityUpdate> for DefiData {
    fn from(value: liquidity::PoolLiquidityUpdate) -> Self {
        Self::PoolLiquidityUpdate(value)
    }
}

impl From<Pool> for DefiData {
    fn from(value: amm::Pool) -> Self {
        Self::Pool(value)
    }
}

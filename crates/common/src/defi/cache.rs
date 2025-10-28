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

//! DeFi-specific cache functionality.

use ahash::AHashMap;
use nautilus_model::{
    defi::{Pool, PoolProfiler},
    identifiers::{InstrumentId, Venue},
};

use crate::cache::Cache;

/// DeFi-specific cache state.
#[derive(Clone, Debug, Default)]
pub(crate) struct DefiCache {
    pub(crate) pools: AHashMap<InstrumentId, Pool>,
    pub(crate) pool_profilers: AHashMap<InstrumentId, PoolProfiler>,
}

impl Cache {
    /// Adds a `Pool` to the cache.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but follows the same pattern as other add methods for consistency.
    pub fn add_pool(&mut self, pool: Pool) -> anyhow::Result<()> {
        log::debug!("Adding `Pool` {}", pool.instrument_id);

        self.defi.pools.insert(pool.instrument_id, pool);
        Ok(())
    }

    /// Adds a `PoolProfiler` to the cache.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but follows the same pattern as other add methods for consistency.
    pub fn add_pool_profiler(&mut self, pool_profiler: PoolProfiler) -> anyhow::Result<()> {
        let instrument_id = pool_profiler.pool.instrument_id;
        log::debug!("Adding `PoolProfiler` {instrument_id}");

        self.defi
            .pool_profilers
            .insert(instrument_id, pool_profiler);
        Ok(())
    }

    /// Gets a reference to the pool for the `instrument_id`.
    #[must_use]
    pub fn pool(&self, instrument_id: &InstrumentId) -> Option<&Pool> {
        self.defi.pools.get(instrument_id)
    }

    /// Gets a mutable reference to the pool for the `instrument_id`.
    #[must_use]
    pub fn pool_mut(&mut self, instrument_id: &InstrumentId) -> Option<&mut Pool> {
        self.defi.pools.get_mut(instrument_id)
    }

    /// Returns the instrument IDs of all pools in the cache, optionally filtered by `venue`.
    #[must_use]
    pub fn pool_ids(&self, venue: Option<&Venue>) -> Vec<InstrumentId> {
        match venue {
            Some(v) => self
                .defi
                .pools
                .keys()
                .filter(|id| &id.venue == v)
                .copied()
                .collect(),
            None => self.defi.pools.keys().copied().collect(),
        }
    }

    /// Returns references to all pools in the cache, optionally filtered by `venue`.
    #[must_use]
    pub fn pools(&self, venue: Option<&Venue>) -> Vec<&Pool> {
        match venue {
            Some(v) => self
                .defi
                .pools
                .values()
                .filter(|p| &p.instrument_id.venue == v)
                .collect(),
            None => self.defi.pools.values().collect(),
        }
    }

    /// Gets a reference to the pool profiler for the `instrument_id`.
    #[must_use]
    pub fn pool_profiler(&self, instrument_id: &InstrumentId) -> Option<&PoolProfiler> {
        self.defi.pool_profilers.get(instrument_id)
    }

    /// Gets a mutable reference to the pool profiler for the `instrument_id`.
    #[must_use]
    pub fn pool_profiler_mut(&mut self, instrument_id: &InstrumentId) -> Option<&mut PoolProfiler> {
        self.defi.pool_profilers.get_mut(instrument_id)
    }

    /// Returns the instrument IDs of all pool profilers in the cache, optionally filtered by `venue`.
    #[must_use]
    pub fn pool_profiler_ids(&self, venue: Option<&Venue>) -> Vec<InstrumentId> {
        match venue {
            Some(v) => self
                .defi
                .pool_profilers
                .keys()
                .filter(|id| &id.venue == v)
                .copied()
                .collect(),
            None => self.defi.pool_profilers.keys().copied().collect(),
        }
    }

    /// Returns references to all pool profilers in the cache, optionally filtered by `venue`.
    #[must_use]
    pub fn pool_profilers(&self, venue: Option<&Venue>) -> Vec<&PoolProfiler> {
        match venue {
            Some(v) => self
                .defi
                .pool_profilers
                .values()
                .filter(|p| &p.pool.instrument_id.venue == v)
                .collect(),
            None => self.defi.pool_profilers.values().collect(),
        }
    }
}

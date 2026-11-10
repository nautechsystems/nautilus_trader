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

//! DeFi-specific cache functionality.

use std::{cell::RefCell, fmt::Debug, sync::Arc};

use ahash::{AHashMap, AHashSet};
use nautilus_model::{
    defi::{Pool, PoolProfiler, currency},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::Currency,
};
use ustr::Ustr;

use crate::cache::Cache;

/// Identifies a DeFi currency conflict that prevents dependent state from being published.
#[derive(Debug, thiserror::Error)]
#[error("DeFi currency integrity: {0}")]
pub struct IntegrityError(pub String);

/// DeFi-specific cache state.
#[derive(Clone, Debug, Default)]
pub(crate) struct DefiCache {
    currencies: AHashSet<Ustr>,
    execution_checks: RefCell<AHashMap<ClientId, ExecutionCheck>>,
    pools: AHashMap<InstrumentId, Pool>,
    pool_profilers: AHashMap<InstrumentId, PoolProfiler>,
}

#[derive(Clone)]
struct ExecutionCheck {
    chain_id: u32,
    check: Arc<dyn Fn(InstrumentId) -> anyhow::Result<()> + Send + Sync>,
}

impl Debug for ExecutionCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionCheck))
            .field("chain_id", &self.chain_id)
            .finish_non_exhaustive()
    }
}

/// Checks the live state and authoritative intent stores before replacing a legacy instrument.
///
/// The cache creates this check for one instrument. Backends invoke it only for legacy replacement.
#[derive(Clone)]
pub struct InstrumentMigration {
    check: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
}

impl Debug for InstrumentMigration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InstrumentMigration")
    }
}

impl InstrumentMigration {
    /// Checks that replacement preserves live state and has no active swap intent.
    ///
    /// # Errors
    ///
    /// Returns an error if position history, nonterminal orders, or unavailable or active execution
    /// state blocks replacement.
    pub fn validate(&self) -> anyhow::Result<()> {
        (self.check)()
    }
}

impl Cache {
    /// Registers the authoritative execution-store check for a blockchain client.
    ///
    /// The check covers active swap intents for an instrument, including when the cache uses another store.
    pub fn register_defi_execution_check(
        &self,
        client_id: ClientId,
        chain_id: u32,
        check: impl Fn(InstrumentId) -> anyhow::Result<()> + Send + Sync + 'static,
    ) {
        self.defi.execution_checks.borrow_mut().insert(
            client_id,
            ExecutionCheck {
                chain_id,
                check: Arc::new(check),
            },
        );
    }

    pub(crate) fn check_instrument_migration(
        &self,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Option<InstrumentMigration>> {
        let id = instrument.id();
        let previous = self.instrument(&id);

        let changed = previous.is_some_and(|previous| {
            previous.base_currency() != instrument.base_currency()
                || previous.quote_currency() != instrument.quote_currency()
                || previous.settlement_currency() != instrument.settlement_currency()
        });

        if let Some(previous) = previous
            && changed
            && !currency::instrument_codes(previous)?.is_empty()
        {
            return Err(IntegrityError(
                "Cannot change a token instrument's currency identities".into(),
            )
            .into());
        }

        if currency::instrument_codes(instrument)?.is_empty() {
            return Ok(None);
        }

        let migration = self.instrument_migration(instrument)?;
        if changed
            || (previous.is_none()
                && (self.has_position_history(&id)? || self.has_nonterminal_orders(&id)))
        {
            migration.validate()?;
        }

        Ok(Some(migration))
    }

    pub(crate) fn instrument_migration(
        &self,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<InstrumentMigration> {
        let id = instrument.id();
        let history = self.has_position_history(&id)?;
        let orders = self.has_nonterminal_orders(&id);
        let mut chains = Vec::new();

        for code in currency::instrument_codes(instrument)? {
            let (_, metadata) = currency::lookup(Ustr::from(&code))
                .ok_or_else(|| anyhow::anyhow!("Missing token definition for {code}"))?;
            chains.push(metadata.chain_id);
        }

        chains.sort_unstable();
        chains.dedup();
        let checks = self
            .defi
            .execution_checks
            .borrow()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        Ok(InstrumentMigration {
            check: Arc::new(move || {
                anyhow::ensure!(
                    !history,
                    "Instrument {id} has position history; retain its legacy currencies"
                );
                anyhow::ensure!(!orders, "Instrument {id} has nonterminal orders");

                for chain in &chains {
                    let matching = checks
                        .iter()
                        .filter(|check| check.chain_id == *chain)
                        .collect::<Vec<_>>();
                    anyhow::ensure!(
                        !matching.is_empty(),
                        "No authoritative execution-store check for chain {chain}; cannot replace {id}"
                    );

                    for check in matching {
                        (check.check)(id)?;
                    }
                }

                Ok(())
            }),
        })
    }

    fn has_nonterminal_orders(&self, id: &InstrumentId) -> bool {
        self.orders_refs(None, Some(id), None, None, None)
            .iter()
            .any(|order| !order.is_closed())
    }

    /// Adds a `Pool` to the cache.
    ///
    /// This function does not return errors; the result type matches the other add methods.
    #[allow(
        clippy::missing_errors_doc,
        reason = "this method only inserts into the in-memory cache"
    )]
    pub fn add_pool(&mut self, pool: Pool) -> anyhow::Result<()> {
        log::debug!("Adding `Pool` {}", pool.instrument_id);

        self.defi.pools.insert(pool.instrument_id, pool);
        Ok(())
    }

    /// Adds a `PoolProfiler` to the cache.
    ///
    /// This function does not return errors; the result type matches the other add methods.
    #[allow(
        clippy::missing_errors_doc,
        reason = "this method only inserts into the in-memory cache"
    )]
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

impl DefiCache {
    pub(crate) fn restore_currencies(&mut self, currencies: &AHashMap<Ustr, Currency>) {
        self.currencies = currencies
            .values()
            .filter(|currency| currency::lookup(currency.code).is_some())
            .map(|currency| currency.code)
            .collect();
    }

    pub(crate) fn contains_currency(&self, code: &Ustr) -> bool {
        self.currencies.contains(code)
    }

    pub(crate) fn add_currency(&mut self, code: Ustr) {
        self.currencies.insert(code);
    }

    pub(crate) fn clear_currencies(&mut self) {
        self.currencies.clear();
    }

    pub(crate) fn remove_instrument(&mut self, instrument_id: &InstrumentId) {
        self.pools.remove(instrument_id);
        self.pool_profilers.remove(instrument_id);
    }

    pub(crate) fn clear_pools(&mut self) {
        self.pools.clear();
        self.pool_profilers.clear();
    }
}

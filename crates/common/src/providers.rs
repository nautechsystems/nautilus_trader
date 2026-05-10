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

//! Instrument provider trait and shared instrument storage.
//!
//! Defines the [`InstrumentProvider`] trait for loading instrument definitions
//! from venue APIs, and the [`InstrumentStore`] struct for caching them locally.

use std::collections::HashMap;

use ahash::AHashMap;
use async_trait::async_trait;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

/// Local instrument storage with initialization tracking.
///
/// Provides `add`/`find`/`get_all` operations for instrument caching.
/// Not thread-safe by itself; wrap in `Arc<RwLock<InstrumentStore>>` when
/// sharing across async tasks or WebSocket handlers.
#[derive(Debug, Default)]
pub struct InstrumentStore {
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    initialized: bool,
}

impl InstrumentStore {
    /// Creates a new empty instrument store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an instrument to the store, replacing any existing entry with the same ID.
    pub fn add(&mut self, instrument: InstrumentAny) {
        self.instruments.insert(instrument.id(), instrument);
    }

    /// Adds multiple instruments to the store.
    pub fn add_bulk(&mut self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.add(instrument);
        }
    }

    /// Returns the instrument for the given ID, if found.
    #[must_use]
    pub fn find(&self, instrument_id: &InstrumentId) -> Option<&InstrumentAny> {
        self.instruments.get(instrument_id)
    }

    /// Returns whether the store contains the given instrument ID.
    #[must_use]
    pub fn contains(&self, instrument_id: &InstrumentId) -> bool {
        self.instruments.contains_key(instrument_id)
    }

    /// Returns all instruments as a map keyed by instrument ID.
    #[must_use]
    pub fn get_all(&self) -> &AHashMap<InstrumentId, InstrumentAny> {
        &self.instruments
    }

    /// Returns all instruments as a vector.
    #[must_use]
    pub fn list_all(&self) -> Vec<&InstrumentAny> {
        self.instruments.values().collect()
    }

    /// Returns the number of instruments in the store.
    #[must_use]
    pub fn count(&self) -> usize {
        self.instruments.len()
    }

    /// Returns whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instruments.is_empty()
    }

    /// Returns whether the store has been marked as initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Marks the store as initialized.
    pub fn set_initialized(&mut self) {
        self.initialized = true;
    }

    /// Clears all instruments and resets initialization state.
    pub fn clear(&mut self) {
        self.instruments.clear();
        self.initialized = false;
    }
}

/// Provides instrument definitions from a venue.
///
/// Implementations define how instruments are fetched from a venue API.
/// The `store()` / `store_mut()` accessors expose the underlying
/// [`InstrumentStore`] so that callers can query cached instruments.
///
/// # Thread safety
///
/// Provider instances are not intended to be sent across threads. The `?Send`
/// bound allows implementations to hold non-Send state for Python interop.
#[async_trait(?Send)]
pub trait InstrumentProvider {
    /// Returns a reference to the provider's instrument store.
    fn store(&self) -> &InstrumentStore;

    /// Returns a mutable reference to the provider's instrument store.
    fn store_mut(&mut self) -> &mut InstrumentStore;

    /// Loads all available instruments from the venue.
    ///
    /// Implementations should populate the store via `store_mut().add()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the loading operation fails.
    async fn load_all(&mut self, filters: Option<&HashMap<String, String>>) -> anyhow::Result<()>;

    /// Loads specific instruments by their IDs.
    ///
    /// The default implementation calls [`load`](Self::load) for each ID
    /// sequentially. Adapters with batch APIs should override this.
    ///
    /// # Errors
    ///
    /// Returns an error if any instrument fails to load.
    async fn load_ids(
        &mut self,
        instrument_ids: &[InstrumentId],
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        for instrument_id in instrument_ids {
            self.load(instrument_id, filters).await?;
        }
        Ok(())
    }

    /// Loads a single instrument by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the loading operation fails.
    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_instrument_store_default_is_empty() {
        let store = InstrumentStore::new();
        assert!(store.is_empty());
        assert_eq!(store.count(), 0);
        assert!(!store.is_initialized());
    }

    #[rstest]
    fn test_instrument_store_add_and_find() {
        let mut store = InstrumentStore::new();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let id = instrument.id();

        store.add(instrument);

        assert_eq!(store.count(), 1);
        assert!(!store.is_empty());
        assert!(store.contains(&id));
        assert!(store.find(&id).is_some());
    }

    #[rstest]
    fn test_instrument_store_add_bulk() {
        let mut store = InstrumentStore::new();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let id = instrument.id();

        store.add_bulk(vec![instrument]);

        assert_eq!(store.count(), 1);
        assert!(store.contains(&id));
    }

    #[rstest]
    fn test_instrument_store_get_all() {
        let mut store = InstrumentStore::new();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

        store.add(instrument);

        let all = store.get_all();
        assert_eq!(all.len(), 1);
    }

    #[rstest]
    fn test_instrument_store_list_all() {
        let mut store = InstrumentStore::new();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

        store.add(instrument);

        let list = store.list_all();
        assert_eq!(list.len(), 1);
    }

    #[rstest]
    fn test_instrument_store_clear() {
        let mut store = InstrumentStore::new();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

        store.add(instrument);
        store.set_initialized();
        assert!(store.is_initialized());
        assert_eq!(store.count(), 1);

        store.clear();
        assert!(!store.is_initialized());
        assert!(store.is_empty());
    }

    #[rstest]
    fn test_instrument_store_find_missing_returns_none() {
        let store = InstrumentStore::new();
        let id = InstrumentId::from("UNKNOWN-UNKNOWN.VENUE");
        assert!(store.find(&id).is_none());
        assert!(!store.contains(&id));
    }

    #[rstest]
    fn test_instrument_store_add_replaces_existing() {
        let mut store = InstrumentStore::new();
        let instrument1 = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let instrument2 = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let id = instrument1.id();

        store.add(instrument1);
        store.add(instrument2);

        assert_eq!(store.count(), 1);
        assert!(store.contains(&id));
    }
}

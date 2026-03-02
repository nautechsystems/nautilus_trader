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

//! Instrument provider for the Polymarket adapter.

use std::collections::HashMap;

use ahash::AHashMap;
use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use crate::http::client::PolymarketHttpClient;

/// Provides Polymarket instruments via the Gamma API.
///
/// Wraps [`PolymarketHttpClient`] with an [`InstrumentStore`] and a
/// token_id index for resolving WebSocket asset IDs to instruments.
#[derive(Debug)]
pub struct PolymarketInstrumentProvider {
    store: InstrumentStore,
    http_client: PolymarketHttpClient,
    token_index: AHashMap<Ustr, InstrumentId>,
}

impl PolymarketInstrumentProvider {
    /// Creates a new [`PolymarketInstrumentProvider`] with an empty store.
    #[must_use]
    pub fn new(http_client: PolymarketHttpClient) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
        }
    }

    /// Returns the instrument for the given token ID, if found.
    #[must_use]
    pub fn get_by_token_id(&self, token_id: &Ustr) -> Option<&InstrumentAny> {
        let instrument_id = self.token_index.get(token_id)?;
        self.store.find(instrument_id)
    }

    /// Builds a frozen snapshot mapping token IDs to instruments.
    ///
    /// Used to provide the WS handler task with a read-only lookup
    /// table after instruments have been loaded.
    #[must_use]
    pub fn build_token_map(&self) -> AHashMap<Ustr, InstrumentAny> {
        self.token_index
            .iter()
            .filter_map(|(token_id, instrument_id)| {
                self.store
                    .find(instrument_id)
                    .map(|inst| (*token_id, inst.clone()))
            })
            .collect()
    }

    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub fn http_client(&self) -> &PolymarketHttpClient {
        &self.http_client
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for PolymarketInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, _filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        let instruments = self.http_client.request_instruments().await?;

        self.store.clear();
        self.token_index.clear();

        for instrument in &instruments {
            self.token_index.insert(
                Ustr::from(instrument.raw_symbol().as_str()),
                instrument.id(),
            );
        }

        self.store.add_bulk(instruments);
        self.store.set_initialized();

        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if self.store.contains(instrument_id) {
            return Ok(());
        }

        if !self.store.is_initialized() {
            self.load_all(filters).await?;
        }

        if self.store.contains(instrument_id) {
            Ok(())
        } else {
            anyhow::bail!("Instrument {instrument_id} not found on Polymarket")
        }
    }
}

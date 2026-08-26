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

//! Instrument provider for loading and caching Massive instruments.

use std::sync::Arc;

use nautilus_core::AtomicMap;
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};

use crate::http::{
    client::MassiveHttpClient, models::MassiveTickersResponse, parse::parse_instrument,
};

/// Loads and caches Massive instruments.
///
/// Wraps a [`MassiveHttpClient`] and provides methods for loading US equity
/// instruments from the reference tickers API or from pre-fetched JSON.
/// Parsed instruments are cached in the HTTP client's shared `AtomicMap`.
#[derive(Debug, Clone)]
pub struct MassiveInstrumentProvider {
    client: MassiveHttpClient,
    /// Tickers to load on `load_all`; empty loads the full active universe.
    symbols: Vec<String>,
}

impl MassiveInstrumentProvider {
    /// Creates a new [`MassiveInstrumentProvider`].
    ///
    /// When `symbols` is empty, `load_all` fetches every active US
    /// stocks-market ticker (several thousand instruments).
    #[must_use]
    pub fn new(client: MassiveHttpClient, symbols: Vec<String>) -> Self {
        Self { client, symbols }
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        self.client.instruments()
    }

    /// Returns the number of cached instruments.
    #[must_use]
    pub fn count(&self) -> usize {
        self.client.instruments().len()
    }

    /// Returns a cached instrument by ID, if present.
    #[must_use]
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.client.instruments().get_cloned(instrument_id)
    }

    /// Loads the configured instruments from the Massive REST API and caches
    /// them.
    ///
    /// # Errors
    ///
    /// Returns an error if a request fails or a response cannot be parsed.
    pub async fn load_all(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        self.client.request_instruments(&self.symbols).await
    }

    /// Loads a single instrument by ticker from the REST API and caches it.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn load(&self, ticker: &str) -> anyhow::Result<InstrumentAny> {
        self.client.request_instrument(ticker).await
    }

    /// Parses a `/v3/reference/tickers` response page and caches the
    /// instruments (for offline loading and tests).
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON cannot be deserialized.
    pub fn load_from_tickers_response(
        &self,
        json: &serde_json::Value,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let response: MassiveTickersResponse =
            serde_json::from_value(json.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;

        let infos = response.results.unwrap_or_default();
        let ts_init = self.client.ts_now();
        let mut instruments = Vec::with_capacity(infos.len());

        for info in &infos {
            match parse_instrument(info, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => log::debug!("Skipping ticker '{}' during parse: {e}", info.ticker),
            }
        }

        self.client.cache_instruments(&instruments);
        Ok(instruments)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn provider() -> MassiveInstrumentProvider {
        MassiveInstrumentProvider::new(MassiveHttpClient::default(), vec![])
    }

    #[rstest]
    fn test_provider_starts_empty() {
        let provider = provider();
        assert_eq!(provider.count(), 0);
    }

    #[rstest]
    fn test_load_from_tickers_response() {
        let provider = provider();
        let json: serde_json::Value =
            serde_json::from_str(&load_test_fixture("http_tickers.json")).unwrap();

        let instruments = provider.load_from_tickers_response(&json).unwrap();

        assert_eq!(instruments.len(), 2);
        assert_eq!(provider.count(), 2);
        assert_eq!(instruments[0].id().to_string(), "AAPL.MASSIVE");
        assert_eq!(instruments[1].id().to_string(), "BRK.A.MASSIVE");
        assert!(provider.get(&instruments[0].id()).is_some());
    }

    #[rstest]
    fn test_load_from_tickers_response_empty_results() {
        let provider = provider();
        let json = serde_json::json!({"status": "OK", "request_id": "abc", "count": 0});

        let instruments = provider.load_from_tickers_response(&json).unwrap();
        assert!(instruments.is_empty());
    }

    #[rstest]
    fn test_get_returns_none_for_missing_instrument() {
        let provider = provider();
        let missing = InstrumentId::from("MISSING.MASSIVE");
        assert!(provider.get(&missing).is_none());
    }
}

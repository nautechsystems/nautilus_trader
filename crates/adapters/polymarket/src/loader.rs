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

//! Standalone data loader for Polymarket instruments.

use nautilus_model::instruments::InstrumentAny;

use crate::http::{
    gamma::PolymarketGammaHttpClient,
    models::{GammaEvent, GammaMarket, GammaTag},
    query::{GetGammaEventsParams, GetGammaMarketsParams, GetSearchParams},
};

/// Standalone utility for loading Polymarket instruments and market data.
///
/// Unlike the instrument provider, this loader does not cache results — each
/// call returns fresh data from the Gamma API. Useful for offline analysis,
/// batch loading, and interactive exploration.
#[derive(Debug, Clone)]
pub struct PolymarketDataLoader {
    gamma_client: PolymarketGammaHttpClient,
}

impl PolymarketDataLoader {
    /// Creates a new [`PolymarketDataLoader`].
    #[must_use]
    pub fn new(gamma_client: PolymarketGammaHttpClient) -> Self {
        Self { gamma_client }
    }

    /// Loads instruments from a single market slug.
    pub async fn from_market_slug(&self, slug: &str) -> anyhow::Result<Vec<InstrumentAny>> {
        self.gamma_client
            .request_instruments_by_slugs(vec![slug.to_string()])
            .await
    }

    /// Loads instruments from an event slug (all markets in event).
    pub async fn from_event_slug(&self, slug: &str) -> anyhow::Result<Vec<InstrumentAny>> {
        self.gamma_client
            .request_instruments_by_event_slugs(vec![slug.to_string()])
            .await
    }

    /// Searches for instruments by text query.
    pub async fn search(&self, query: &str) -> anyhow::Result<Vec<InstrumentAny>> {
        let params = GetSearchParams {
            q: Some(query.to_string()),
            ..Default::default()
        };
        self.gamma_client
            .request_instruments_by_search(params)
            .await
    }

    /// Fetches available tags from the Gamma API.
    pub async fn list_tags(&self) -> anyhow::Result<Vec<GammaTag>> {
        self.gamma_client.request_tags().await
    }

    /// Queries raw market data by slug (for inspection).
    pub async fn query_market_by_slug(&self, slug: &str) -> anyhow::Result<Vec<GammaMarket>> {
        let params = GetGammaMarketsParams {
            slug: Some(slug.to_string()),
            ..Default::default()
        };
        Ok(self.gamma_client.inner().get_gamma_markets(params).await?)
    }

    /// Queries raw event data by slug (for inspection).
    pub async fn query_event_by_slug(&self, slug: &str) -> anyhow::Result<Vec<GammaEvent>> {
        Ok(self
            .gamma_client
            .inner()
            .get_gamma_events_by_slug(slug)
            .await?)
    }

    /// Queries events with full params.
    pub async fn query_events(
        &self,
        params: GetGammaEventsParams,
    ) -> anyhow::Result<Vec<GammaEvent>> {
        Ok(self.gamma_client.inner().get_gamma_events(params).await?)
    }
}

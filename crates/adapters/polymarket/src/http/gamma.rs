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

//! Provides the HTTP client for the Polymarket Gamma API.

use std::{collections::HashMap, result::Result as StdResult, sync::Arc};

use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::{HttpClient, HttpClientError, Method, USER_AGENT};
use serde::Serialize;
use serde_json::Value;

use crate::{
    common::urls::gamma_api_url,
    http::{
        error::{Error, Result},
        models::GammaMarket,
        parse::{create_instrument_from_def, parse_gamma_market},
        query::GetGammaMarketsParams,
        rate_limits::POLYMARKET_GAMMA_REST_QUOTA,
    },
};

/// Provides a raw HTTP client for the Polymarket Gamma API.
///
/// Handles HTTP transport for fetching market data from the public Gamma API.
/// No authentication is required.
#[derive(Debug, Clone)]
pub struct PolymarketGammaRawHttpClient {
    client: HttpClient,
    base_url: String,
}

impl PolymarketGammaRawHttpClient {
    /// Creates a new [`PolymarketGammaRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*POLYMARKET_GAMMA_REST_QUOTA),
                timeout_secs,
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn send_get<P: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        let url = self.url(path);
        let response = self
            .client
            .request_with_params(Method::GET, url, params, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    /// Fetches markets from the Gamma API.
    ///
    /// Handles both bare array and `{"data": [...]}` response schemas.
    pub async fn get_gamma_markets(
        &self,
        params: GetGammaMarketsParams,
    ) -> Result<Vec<GammaMarket>> {
        let value: Value = self.send_get("/markets", Some(&params)).await?;

        let array = match value {
            Value::Array(_) => value,
            Value::Object(ref map) if map.contains_key("data") => {
                map.get("data").cloned().unwrap_or(Value::Array(vec![]))
            }
            _ => {
                return Err(Error::decode(
                    "Unrecognized Gamma markets response schema".to_string(),
                ));
            }
        };

        serde_json::from_value(array).map_err(Error::Serde)
    }

    /// Fetches a single market by ID from the Gamma API.
    pub async fn get_gamma_market(&self, market_id: &str) -> Result<GammaMarket> {
        let path = format!("/markets/{market_id}");
        self.send_get::<(), _>(&path, None::<&()>).await
    }
}

/// Provides a domain HTTP client for Polymarket instrument fetching.
///
/// Wraps [`PolymarketGammaRawHttpClient`] with instrument parsing: fetch from
/// the Gamma API and parse into Nautilus types. Stateless with respect to
/// instrument storage; caching is handled by the instrument provider.
#[derive(Debug, Clone)]
pub struct PolymarketGammaHttpClient {
    inner: Arc<PolymarketGammaRawHttpClient>,
    clock: &'static AtomicTime,
}

impl PolymarketGammaHttpClient {
    /// Creates a new [`PolymarketGammaHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        gamma_base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            inner: Arc::new(PolymarketGammaRawHttpClient::new(
                gamma_base_url,
                timeout_secs,
            )?),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Fetches all active markets from the Gamma API, paginating automatically.
    async fn fetch_all_gamma_markets(&self) -> anyhow::Result<Vec<GammaMarket>> {
        const PAGE_LIMIT: u32 = 500;
        let mut all_markets = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let params = GetGammaMarketsParams {
                active: Some(true),
                closed: Some(false),
                limit: Some(PAGE_LIMIT),
                offset: Some(offset),
                ..Default::default()
            };

            let page = self.inner.get_gamma_markets(params).await?;
            let page_len = page.len() as u32;
            all_markets.extend(page);

            if page_len < PAGE_LIMIT {
                break;
            }
            offset += PAGE_LIMIT;
        }

        Ok(all_markets)
    }

    /// Fetches instruments from the Gamma API and returns Nautilus domain types.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or parsing fails.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let markets = self.fetch_all_gamma_markets().await?;
        let ts_init = self.clock.get_time_ns();

        let mut instruments = Vec::new();
        for market in &markets {
            match parse_gamma_market(market) {
                Ok(defs) => {
                    for def in defs {
                        match create_instrument_from_def(&def, ts_init) {
                            Ok(instrument) => instruments.push(instrument),
                            Err(e) => log::warn!("Failed to create instrument: {e}"),
                        }
                    }
                }
                Err(e) => log::warn!("Failed to parse gamma market: {e}"),
            }
        }

        log::info!("Parsed {} instruments from Gamma API", instruments.len());
        Ok(instruments)
    }

    /// Fetches instruments for the given slugs concurrently.
    ///
    /// Each slug is queried individually via the Gamma API. Missing or
    /// unparsable slugs are logged and skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if all slug requests fail. Individual slug failures
    /// are warned and skipped when at least one slug succeeds.
    pub async fn request_instruments_by_slugs(
        &self,
        slugs: Vec<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.clock.get_time_ns();

        let futures = slugs.into_iter().map(|slug| {
            let inner = Arc::clone(&self.inner);
            async move {
                let params = GetGammaMarketsParams {
                    slug: Some(slug.clone()),
                    ..Default::default()
                };
                match inner.get_gamma_markets(params).await {
                    Ok(markets) => Some((slug, markets)),
                    Err(e) => {
                        log::warn!("Failed to fetch slug '{slug}': {e}");
                        None
                    }
                }
            }
        });

        let results = futures_util::future::join_all(futures).await;

        let total_slugs = results.len();
        let succeeded = results.iter().filter(|r| r.is_some()).count();
        let mut instruments = Vec::new();

        for result in results.into_iter().flatten() {
            let (slug, markets) = result;
            if markets.is_empty() {
                log::warn!("No markets found for slug '{slug}'");
                continue;
            }
            for market in &markets {
                match parse_gamma_market(market) {
                    Ok(defs) => {
                        for def in defs {
                            match create_instrument_from_def(&def, ts_init) {
                                Ok(instrument) => instruments.push(instrument),
                                Err(e) => {
                                    log::warn!(
                                        "Failed to create instrument for slug '{slug}': {e}"
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => log::warn!("Failed to parse market for slug '{slug}': {e}"),
                }
            }
        }

        if succeeded == 0 && total_slugs > 0 {
            anyhow::bail!("All {total_slugs} slug requests failed");
        }

        log::info!("Parsed {} instruments from slug queries", instruments.len());
        Ok(instruments)
    }

    /// Returns a reference to the underlying raw HTTP client.
    #[must_use]
    pub fn inner(&self) -> &Arc<PolymarketGammaRawHttpClient> {
        &self.inner
    }
}

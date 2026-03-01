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

//! Provides the HTTP client for the Kalshi REST API.

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::http::{HttpClient, Method, USER_AGENT};
use nautilus_network::ratelimiter::quota::Quota;

use crate::{
    common::{
        credential::{KalshiCredential, HEADER_ACCESS_KEY, HEADER_SIGNATURE, HEADER_TIMESTAMP},
        enums::CandlestickInterval,
    },
    config::KalshiDataClientConfig,
    http::{
        error::{Error, Result},
        models::{
            KalshiCandlestick, KalshiCandlesticksResponse, KalshiMarket, KalshiMarketsResponse,
            KalshiOrderbookResponse, KalshiTrade, KalshiTradesResponse,
        },
        rate_limits::KALSHI_REST_QUOTA,
    },
};

const PATH_MARKETS: &str = "/markets";
const PATH_TRADES: &str = "/markets/trades";
const PATH_ORDERBOOK_SUFFIX: &str = "/orderbook";
const PATH_CANDLESTICKS_PREFIX: &str = "/historical/markets";
const PATH_CANDLESTICKS_SUFFIX: &str = "/candlesticks";

/// Provides a raw HTTP client for Kalshi REST API operations.
///
/// Handles HTTP transport, RSA-PSS authentication signing, pagination,
/// and raw API calls that closely match Kalshi endpoint specifications.
#[derive(Debug, Clone)]
pub struct KalshiHttpClient {
    inner: HttpClient,
    base_url: String,
    credential: Option<Arc<KalshiCredential>>,
}

impl KalshiHttpClient {
    /// Creates a new [`KalshiHttpClient`] from the given configuration.
    ///
    /// If the config contains valid API credentials they will be used for
    /// authenticated endpoints; otherwise the client operates in read-only
    /// (public) mode.
    ///
    /// # Panics
    ///
    /// Panics if the `HttpClient` cannot be constructed (e.g. invalid proxy URL).
    #[must_use]
    pub fn new(config: KalshiDataClientConfig) -> Self {
        let rate_limit_rps = config.rate_limit_rps;
        let timeout_secs = config.http_timeout_secs;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| crate::common::urls::rest_base_url().to_string())
            .trim_end_matches('/')
            .to_string();

        // Attempt to build credentials from config.
        let credential = config
            .resolved_api_key_id()
            .zip(config.resolved_private_key_pem())
            .and_then(|(key_id, pem)| {
                KalshiCredential::new(key_id, &pem)
                    .map_err(|e| {
                        log::warn!("Failed to build Kalshi credential: {e}");
                    })
                    .ok()
            })
            .map(Arc::new);

        let quota = Quota::per_second(NonZeroU32::new(rate_limit_rps).unwrap_or_else(|| {
            NonZeroU32::new(20).unwrap()
        }))
        .unwrap_or(*KALSHI_REST_QUOTA);

        let inner = HttpClient::new(
            Self::default_headers(),
            vec![],
            vec![],
            Some(quota),
            Some(timeout_secs),
            None,
        )
        .expect("Failed to build Kalshi HttpClient");

        Self {
            inner,
            base_url,
            credential,
        }
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ])
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Builds auth headers for an authenticated request by signing `method + path`.
    fn auth_headers(&self, method: &str, path: &str) -> Result<HashMap<String, String>> {
        let cred = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credential configured"))?;

        let (timestamp_ms, signature) = cred.sign(method, path);

        Ok(HashMap::from([
            (HEADER_ACCESS_KEY.to_string(), cred.api_key_id().to_string()),
            (HEADER_TIMESTAMP.to_string(), timestamp_ms),
            (HEADER_SIGNATURE.to_string(), signature),
        ]))
    }

    /// Sends a GET request with serializable query params, no auth.
    async fn get<P: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        let url = self.url(path);
        let response = self
            .inner
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

    /// Sends a GET request with auth headers.
    async fn get_authed<P: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        let headers = Some(self.auth_headers("GET", path)?);
        let url = self.url(path);
        let response = self
            .inner
            .request_with_params(Method::GET, url, params, headers, None, None, None)
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

    // -----------------------------------------------------------------------
    // Public endpoints
    // -----------------------------------------------------------------------

    /// Fetches all markets matching the given event and series tickers.
    ///
    /// Paginates internally until no more pages are available.
    /// Filters by `event_tickers` and `series_tickers` as query params.
    ///
    /// # Errors
    ///
    /// Returns an error if any HTTP request or JSON parse fails.
    pub async fn get_markets(
        &self,
        event_tickers: &[&str],
        series_tickers: &[&str],
    ) -> Result<Vec<KalshiMarket>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            // Build query params for this page.
            let mut params: Vec<(&str, String)> = Vec::new();
            for et in event_tickers {
                params.push(("event_ticker", (*et).to_string()));
            }
            for st in series_tickers {
                params.push(("series_ticker", (*st).to_string()));
            }
            if let Some(ref c) = cursor {
                params.push(("cursor", c.clone()));
            }

            let resp: KalshiMarketsResponse = self.get(PATH_MARKETS, Some(&params)).await?;
            all.extend(resp.markets);

            // An empty or absent cursor means no more pages.
            match resp.cursor {
                None => break,
                Some(ref c) if c.is_empty() => break,
                Some(c) => cursor = Some(c),
            }
        }

        Ok(all)
    }

    /// Fetches one page of trades for a market.
    ///
    /// Returns `(trades, next_cursor)`. The caller controls pagination.
    /// An empty-string cursor in the response is normalised to `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parse fails.
    pub async fn get_trades(
        &self,
        market_ticker: &str,
        min_ts: Option<u64>,
        max_ts: Option<u64>,
        cursor: Option<&str>,
    ) -> Result<(Vec<KalshiTrade>, Option<String>)> {
        let mut params: Vec<(&str, String)> = vec![("ticker", market_ticker.to_string())];
        if let Some(ts) = min_ts {
            params.push(("min_ts", ts.to_string()));
        }
        if let Some(ts) = max_ts {
            params.push(("max_ts", ts.to_string()));
        }
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }

        let resp: KalshiTradesResponse = self.get(PATH_TRADES, Some(&params)).await?;

        // Normalise empty-string cursor to None.
        let next_cursor = resp.cursor.filter(|c| !c.is_empty());
        Ok((resp.trades, next_cursor))
    }

    /// Fetches OHLCV candlesticks for a market over the given time range.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parse fails.
    pub async fn get_candlesticks(
        &self,
        market_ticker: &str,
        start_ts: u64,
        end_ts: u64,
        period_interval: CandlestickInterval,
    ) -> Result<Vec<KalshiCandlestick>> {
        let path = format!(
            "{PATH_CANDLESTICKS_PREFIX}/{market_ticker}{PATH_CANDLESTICKS_SUFFIX}"
        );
        let params: Vec<(&str, String)> = vec![
            ("start_ts", start_ts.to_string()),
            ("end_ts", end_ts.to_string()),
            ("period_interval", period_interval.as_minutes().to_string()),
        ];

        let resp: KalshiCandlesticksResponse = self.get(&path, Some(&params)).await?;
        Ok(resp.candlesticks)
    }

    // -----------------------------------------------------------------------
    // Authenticated endpoints
    // -----------------------------------------------------------------------

    /// Fetches the orderbook for a market (requires credentials).
    ///
    /// # Errors
    ///
    /// Returns an error if no credential is configured, or if the HTTP
    /// request or JSON parse fails.
    pub async fn get_orderbook(
        &self,
        market_ticker: &str,
        depth: Option<u32>,
    ) -> Result<KalshiOrderbookResponse> {
        let path = format!("{PATH_MARKETS}/{market_ticker}{PATH_ORDERBOOK_SUFFIX}");
        let params: Vec<(&str, String)> = depth
            .map(|d| vec![("depth", d.to_string())])
            .unwrap_or_default();

        self.get_authed(&path, Some(&params)).await
    }
}

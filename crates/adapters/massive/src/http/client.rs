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

//! Provides the HTTP client for the Massive REST API.
//!
//! Two-layer architecture:
//! - [`MassiveRawHttpClient`]: low-level requests, bearer auth, rate limiting,
//!   retries, and `next_url` cursor pagination.
//! - [`MassiveHttpClient`]: domain wrapper converting wire models into
//!   Nautilus instruments, bars, trades, and quotes.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use arc_swap::ArcSwap;
use nautilus_core::{
    AtomicMap, UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick, bar::BarType},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;

use crate::{
    common::{
        consts::{
            AGGS_PAGE_LIMIT, REST_URL, TICKERS_ANY_OF_CHUNK, TICKERS_PAGE_LIMIT, TICKS_PAGE_LIMIT,
        },
        credential::MassiveCredential,
        parse::instrument_id_from_ticker,
    },
    http::{
        error::{Error, Result},
        models::{
            MassiveAggBar, MassiveQuote, MassiveResponse, MassiveTickerDetailsResponse,
            MassiveTickerInfo, MassiveTrade,
        },
        parse::{
            bar_spec_to_aggs_params, parse_agg_bar, parse_http_quote, parse_http_trade,
            parse_instrument,
        },
    },
};

/// Default Massive REST rate limit.
///
/// Paid plans are not rate limited; this client-side quota keeps request
/// bursts (e.g. instrument pagination) shaped. Free-tier keys (5 requests
/// per minute) surface as retryable 429s.
pub static MASSIVE_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(50).expect("non-zero")).expect("valid constant")
});

/// Returns the default retry configuration for the Massive HTTP client.
#[must_use]
pub fn default_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 3,
        initial_delay_ms: 100,
        max_delay_ms: 5_000,
        backoff_factor: 2.0,
        jitter_ms: 250,
        operation_timeout_ms: Some(60_000),
        immediate_first: false,
        max_elapsed_ms: Some(180_000),
    }
}

// Builds a query string from `(key, value)` pairs, percent-encoding both halves.
fn encode_query(params: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        serializer.append_pair(k, v);
    }
    serializer.finish()
}

/// Provides a raw HTTP client for low-level Massive REST API operations.
///
/// Authenticates every request with an `Authorization: Bearer` header.
#[derive(Debug)]
pub struct MassiveRawHttpClient {
    client: HttpClient,
    credential: Option<MassiveCredential>,
    base_url: ArcSwap<String>,
    retry_manager: RetryManager<Error>,
    cancellation_token: CancellationToken,
}

impl MassiveRawHttpClient {
    /// Creates a new [`MassiveRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        credential: Option<MassiveCredential>,
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*MASSIVE_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )?,
            credential,
            base_url: ArcSwap::from_pointee(base_url.unwrap_or_else(|| REST_URL.to_string())),
            retry_manager: RetryManager::new(retry_config.unwrap_or_else(default_retry_config)),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Returns the cancellation token shared by in-flight requests.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Overrides the base REST URL (for testing with mock servers).
    pub fn set_base_url(&self, url: String) {
        self.base_url.store(Arc::new(url));
    }

    /// Returns true if this client has an API key configured.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.credential.is_some()
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ])
    }

    fn build_url(&self, path_and_query: &str) -> String {
        format!("{}{path_and_query}", self.base_url.load())
    }

    fn auth_headers(&self) -> Result<HashMap<String, String>> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No API key configured (set `MASSIVE_API_KEY`)"))?;
        Ok(HashMap::from([(
            "Authorization".to_string(),
            credential.bearer_header(),
        )]))
    }

    fn parse_response(response: &HttpResponse) -> Result<serde_json::Value> {
        if !response.status.is_success() {
            return Err(Error::from_http_status(
                response.status.as_u16(),
                &response.body,
            ));
        }
        serde_json::from_slice(&response.body).map_err(Error::Serde)
    }

    /// Sends an authenticated GET request to an absolute URL with retries.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failure, auth failure, or non-success
    /// HTTP status.
    pub async fn get_url(&self, url: String) -> Result<serde_json::Value> {
        let operation_name = url.clone();

        let operation = || {
            let url = url.clone();
            async move {
                let headers = self.auth_headers()?;
                let response = self
                    .client
                    .request(Method::GET, url, None, Some(headers), None, None, None)
                    .await
                    .map_err(Error::from_http_client)?;
                Self::parse_response(&response)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                &operation_name,
                operation,
                |err: &Error| err.is_retryable(),
                |e| Error::transport(e.to_string()),
                &self.cancellation_token,
            )
            .await
    }

    /// Sends an authenticated GET request for a path (with optional query)
    /// relative to the base URL.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failure, auth failure, or non-success
    /// HTTP status.
    pub async fn get(&self, path_and_query: &str) -> Result<serde_json::Value> {
        self.get_url(self.build_url(path_and_query)).await
    }

    /// Fetches every result page for a paginated list endpoint, following
    /// `next_url` cursors until exhaustion or until `max_items` is reached.
    ///
    /// # Errors
    ///
    /// Returns an error on any request failure or when the venue reports an
    /// `ERROR` status envelope.
    pub async fn get_paginated<T: DeserializeOwned>(
        &self,
        path_and_query: &str,
        max_items: Option<usize>,
    ) -> Result<Vec<T>> {
        let mut collected: Vec<T> = Vec::new();
        let mut url = self.build_url(path_and_query);

        loop {
            let json = self.get_url(url).await?;
            let response: MassiveResponse<Vec<T>> =
                serde_json::from_value(json).map_err(Error::Serde)?;

            if response.status.as_deref() == Some("ERROR") {
                let message = response
                    .message
                    .unwrap_or_else(|| "unknown venue error".to_string());
                return Err(Error::venue(message));
            }

            if let Some(results) = response.results {
                collected.extend(results);
            }

            if let Some(max) = max_items
                && collected.len() >= max
            {
                collected.truncate(max);
                break;
            }

            match response.next_url {
                Some(next) => url = next,
                None => break,
            }
        }

        Ok(collected)
    }
}

/// Provides a domain-level HTTP client for the Massive REST API.
///
/// Wraps [`MassiveRawHttpClient`] in an `Arc` and adds instrument caching
/// and Nautilus type conversions.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.massive", from_py_object)
)]
pub struct MassiveHttpClient {
    pub(crate) inner: Arc<MassiveRawHttpClient>,
    clock: &'static AtomicTime,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
}

impl MassiveHttpClient {
    /// Creates a new [`MassiveHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        credential: Option<MassiveCredential>,
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw =
            MassiveRawHttpClient::new(credential, base_url, timeout_secs, proxy_url, retry_config)?;
        Ok(Self::from_raw(raw))
    }

    /// Creates a client authenticated from the `MASSIVE_API_KEY` environment
    /// variable.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if no API key is available.
    pub fn from_env() -> Result<Self> {
        let credential = MassiveCredential::resolve(None)
            .ok_or_else(|| Error::auth("MASSIVE_API_KEY environment variable not set"))?;
        Self::new(Some(credential), None, 60, None, None)
            .map_err(|e| Error::transport(format!("Failed to create HTTP client: {e}")))
    }

    fn from_raw(raw: MassiveRawHttpClient) -> Self {
        Self {
            inner: Arc::new(raw),
            clock: get_atomic_clock_realtime(),
            instruments: Arc::new(AtomicMap::new()),
        }
    }

    /// Returns the cancellation token shared by in-flight requests.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    /// Overrides the base REST URL (for testing with mock servers).
    pub fn set_base_url(&self, url: String) {
        self.inner.set_base_url(url);
    }

    /// Returns true if this client has an API key configured.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated()
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        &self.instruments
    }

    /// Returns the current timestamp from the atomic clock.
    #[must_use]
    pub fn ts_now(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Caches a batch of instruments in the shared instrument map.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments.rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
    }

    /// Requests instrument definitions for the given tickers, or every
    /// active US stocks-market ticker when `symbols` is empty.
    ///
    /// Parsed instruments are cached in the shared instrument map.
    ///
    /// # Errors
    ///
    /// Returns an error when a request fails or a response cannot be
    /// deserialized.
    pub async fn request_instruments(
        &self,
        symbols: &[String],
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut infos: Vec<MassiveTickerInfo> = Vec::new();

        if symbols.is_empty() {
            let query = encode_query(&[
                ("market", "stocks"),
                ("active", "true"),
                ("order", "asc"),
                ("sort", "ticker"),
                ("limit", &TICKERS_PAGE_LIMIT.to_string()),
            ]);
            infos = self
                .inner
                .get_paginated(&format!("/v3/reference/tickers?{query}"), None)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch tickers: {e}"))?;
        } else {
            // `ticker.any_of` accepts a comma-separated list; chunk to keep
            // URLs bounded.
            for chunk in symbols.chunks(TICKERS_ANY_OF_CHUNK) {
                let any_of = chunk.join(",");
                let query = encode_query(&[
                    ("market", "stocks"),
                    ("ticker.any_of", &any_of),
                    ("limit", &TICKERS_PAGE_LIMIT.to_string()),
                ]);
                let page: Vec<MassiveTickerInfo> = self
                    .inner
                    .get_paginated(&format!("/v3/reference/tickers?{query}"), None)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to fetch tickers: {e}"))?;
                infos.extend(page);
            }
        }

        let ts_init = self.ts_now();
        let mut instruments = Vec::with_capacity(infos.len());
        for info in &infos {
            match parse_instrument(info, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => log::debug!("Skipping ticker '{}' during parse: {e}", info.ticker),
            }
        }

        self.cache_instruments(&instruments);
        Ok(instruments)
    }

    /// Requests a single instrument definition by ticker.
    ///
    /// Caches the result on success.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or the ticker cannot be
    /// parsed into an instrument.
    pub async fn request_instrument(&self, ticker: &str) -> anyhow::Result<InstrumentAny> {
        let json = self
            .inner
            .get(&format!("/v3/reference/tickers/{ticker}"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch ticker '{ticker}': {e}"))?;
        let response: MassiveTickerDetailsResponse =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;
        let info = response
            .results
            .ok_or_else(|| anyhow::anyhow!("No results for ticker '{ticker}'"))?;

        let instrument = parse_instrument(&info, self.ts_now())?;
        self.cache_instruments(std::slice::from_ref(&instrument));
        Ok(instrument)
    }

    /// Requests historical aggregate bars over `[start_ms, end_ms]` (both
    /// inclusive, Unix milliseconds).
    ///
    /// # Errors
    ///
    /// Returns an error when the bar specification is unsupported, a request
    /// fails, or a bar cannot be parsed.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start_ms: i64,
        end_ms: i64,
        limit: Option<usize>,
        adjusted: bool,
        timestamp_on_close: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        let spec = bar_type.spec();
        let (multiplier, timespan) = bar_spec_to_aggs_params(&spec)?;
        let ticker = bar_type.instrument_id().symbol.to_string();

        let query = encode_query(&[
            ("adjusted", if adjusted { "true" } else { "false" }),
            ("sort", "asc"),
            ("limit", &AGGS_PAGE_LIMIT.to_string()),
        ]);
        let path = format!(
            "/v2/aggs/ticker/{ticker}/range/{multiplier}/{timespan}/{start_ms}/{end_ms}?{query}"
        );

        let aggs: Vec<MassiveAggBar> = self
            .inner
            .get_paginated(&path, limit)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch bars for '{ticker}': {e}"))?;

        let ts_init = self.ts_now();
        let mut bars = Vec::with_capacity(aggs.len());
        for agg in &aggs {
            bars.push(parse_agg_bar(bar_type, agg, timestamp_on_close, ts_init)?);
        }
        Ok(bars)
    }

    /// Requests historical trade ticks over `[start_ns, end_ns)` (Unix
    /// nanoseconds).
    ///
    /// # Errors
    ///
    /// Returns an error when a request fails or a trade cannot be parsed.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start_ns: Option<u64>,
        end_ns: Option<u64>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let ticker = instrument_id.symbol.to_string();
        let path = build_ticks_path("trades", &ticker, start_ns, end_ns);

        let records: Vec<MassiveTrade> = self
            .inner
            .get_paginated(&path, limit)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch trades for '{ticker}': {e}"))?;

        let ts_init = self.ts_now();
        let mut trades = Vec::with_capacity(records.len());
        for record in &records {
            if let Some(tick) = parse_http_trade(&ticker, record, ts_init)? {
                trades.push(tick);
            }
        }
        Ok(trades)
    }

    /// Requests historical NBBO quote ticks over `[start_ns, end_ns)` (Unix
    /// nanoseconds).
    ///
    /// # Errors
    ///
    /// Returns an error when a request fails or a quote cannot be parsed.
    pub async fn request_quotes(
        &self,
        instrument_id: InstrumentId,
        start_ns: Option<u64>,
        end_ns: Option<u64>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        let ticker = instrument_id.symbol.to_string();
        let path = build_ticks_path("quotes", &ticker, start_ns, end_ns);

        let records: Vec<MassiveQuote> = self
            .inner
            .get_paginated(&path, limit)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch quotes for '{ticker}': {e}"))?;

        let ts_init = self.ts_now();
        let mut quotes = Vec::with_capacity(records.len());
        for record in &records {
            if let Some(tick) = parse_http_quote(&ticker, record, ts_init)? {
                quotes.push(tick);
            }
        }
        Ok(quotes)
    }

    /// Returns the cached instrument for an instrument ID, fetching it on miss.
    ///
    /// # Errors
    ///
    /// Returns an error when the instrument cannot be fetched or parsed.
    pub async fn get_or_fetch_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<InstrumentAny> {
        debug_assert_eq!(
            instrument_id_from_ticker(instrument_id.symbol.as_str()),
            *instrument_id,
        );

        if let Some(instrument) = self.instruments.get_cloned(instrument_id) {
            return Ok(instrument);
        }
        self.request_instrument(instrument_id.symbol.as_str()).await
    }
}

impl Default for MassiveHttpClient {
    fn default() -> Self {
        Self::new(None, None, 60, None, None).expect("Failed to create default Massive HTTP client")
    }
}

// Builds a `/v3/{trades|quotes}/{ticker}` path with nanosecond window filters.
fn build_ticks_path(
    endpoint: &str,
    ticker: &str,
    start_ns: Option<u64>,
    end_ns: Option<u64>,
) -> String {
    let limit = TICKS_PAGE_LIMIT.to_string();
    let mut pairs: Vec<(&str, String)> = vec![
        ("order", "asc".to_string()),
        ("sort", "timestamp".to_string()),
        ("limit", limit),
    ];

    if let Some(start) = start_ns {
        pairs.push(("timestamp.gte", start.to_string()));
    }

    if let Some(end) = end_ns {
        pairs.push(("timestamp.lt", end.to_string()));
    }
    let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format!("/v3/{endpoint}/{ticker}?{}", encode_query(&borrowed))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_raw_client_construction() {
        let client = MassiveRawHttpClient::new(None, None, 60, None, None).unwrap();
        assert!(!client.is_authenticated());
        assert_eq!(
            client.build_url("/v3/reference/tickers"),
            "https://api.massive.com/v3/reference/tickers"
        );
    }

    #[rstest]
    fn test_raw_client_authenticated() {
        let credential = MassiveCredential::new("test-key".to_string());
        let client = MassiveRawHttpClient::new(Some(credential), None, 60, None, None).unwrap();
        assert!(client.is_authenticated());
        let headers = client.auth_headers().unwrap();
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer test-key");
    }

    #[rstest]
    fn test_raw_client_auth_headers_without_credential() {
        let client = MassiveRawHttpClient::new(None, None, 60, None, None).unwrap();
        let result = client.auth_headers();
        assert!(result.unwrap_err().is_auth_error());
    }

    #[rstest]
    fn test_raw_client_set_base_url() {
        let client = MassiveRawHttpClient::new(None, None, 60, None, None).unwrap();
        client.set_base_url("http://localhost:8080".to_string());
        assert_eq!(client.build_url("/x"), "http://localhost:8080/x");
    }

    #[rstest]
    fn test_domain_client_construction() {
        let client = MassiveHttpClient::default();
        assert!(!client.is_authenticated());
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_build_ticks_path_full_window() {
        let path = build_ticks_path("trades", "AAPL", Some(1_000), Some(2_000));
        assert_eq!(
            path,
            "/v3/trades/AAPL?order=asc&sort=timestamp&limit=50000&timestamp.gte=1000&timestamp.lt=2000"
        );
    }

    #[rstest]
    fn test_build_ticks_path_no_window() {
        let path = build_ticks_path("quotes", "MSFT", None, None);
        assert_eq!(path, "/v3/quotes/MSFT?order=asc&sort=timestamp&limit=50000");
    }

    #[rstest]
    fn test_encode_query_escapes_reserved() {
        let query = encode_query(&[("ticker.any_of", "AAPL,BRK.A")]);
        assert_eq!(query, "ticker.any_of=AAPL%2CBRK.A");
    }
}

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

//! Authenticated GET transport with bounded local admission.

use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use nautilus_core::string::secret::SecretString;
use nautilus_network::{
    ConnectionUrl,
    backoff::ExponentialBackoff,
    dst::time,
    http::{HttpClient, HttpRedirectPolicy, Method},
    retry::{RetryConfig, RetryManager},
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, Error as SerdeError},
};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;
use url::Url;
use ustr::Ustr;

use super::{
    error::Error,
    models::{
        ActivityBatch, ActivityCurrent, Book, BookEvent, Exchange, Market, MarketsPage, Ohlcv,
        OhlcvPeriod, Quote, Trade,
    },
    query::{
        ActiveQuery, ActivityQuery, FlatFileOhlcvQuery, HistoryQuery, ListingsQuery, OhlcvQuery,
        Pagination,
    },
};
use crate::common::credential::resolve_api_key;

const DURATION_MAX_MS: u64 = 86_400_000;

/// Versioned prediction-market endpoint.
pub const REST_URL: &str = "https://api.prediction-markets.finfeedapi.com/v1/";

/// Transport and local request limits for one client shared by its callers.
///
/// # Local Attempt Allowance
///
/// The allowance counts managed `HttpClient` invocations, including retries, with no refunds.
/// The native transport can resend twice after remote HTTP/2 `REFUSED_STREAM` or
/// `GOAWAY(NO_ERROR)` errors without consuming allowance or pacing. These limits do not
/// establish billing, account-wide quota, or continuity guarantees.
///
/// # Timeouts and Retries
///
/// Request timeouts, pacing, retry deadlines, and maximum backoff are positive and limited
/// locally to one day. Both retry deadlines are required.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(on(SecretString, into))]
#[serde(default, deny_unknown_fields)]
pub struct FinFeedHttpConfig {
    /// HTTPS endpoint, or a plaintext loopback test peer, including the version prefix.
    /// Debug output redacts the complete endpoint.
    #[builder(default = FinFeedHttpConfig::default().base_url)]
    pub base_url: SecretString,
    /// Explicit HTTP(S) proxy, with ambient proxies disabled.
    pub proxy_url: Option<SecretString>,
    /// Transport timeout for one request, in seconds.
    #[builder(default = FinFeedHttpConfig::default().timeout_secs)]
    pub timeout_secs: u64,
    /// Maximum managed requests in flight on this instance.
    #[builder(default = FinFeedHttpConfig::default().max_concurrent_requests)]
    pub max_concurrent_requests: usize,
    /// Lifetime managed attempt allowance, including failed and canceled attempts after admission.
    #[builder(default = FinFeedHttpConfig::default().attempt_budget)]
    pub attempt_budget: u64,
    /// Minimum spacing between managed attempt starts, in milliseconds.
    #[builder(default = FinFeedHttpConfig::default().min_request_interval_ms)]
    pub min_request_interval_ms: u64,
    /// Retry policy with required per-operation and total elapsed deadlines, in milliseconds.
    /// Both include local admission waits; the total deadline also covers retries and backoff.
    /// Deadlines must be positive and at most one day. Python `None` retains defaults;
    /// a Rust `None` deadline fails client validation.
    /// Unspecified retry fields retain FinFeed defaults during deserialization.
    #[serde(deserialize_with = "deserialize_retry")]
    #[builder(default = FinFeedHttpConfig::default().retry)]
    pub retry: RetryConfig,
}

impl Default for FinFeedHttpConfig {
    fn default() -> Self {
        Self {
            base_url: REST_URL.into(),
            proxy_url: None,
            timeout_secs: 30,
            max_concurrent_requests: 1,
            attempt_budget: 100,
            min_request_interval_ms: 10_000,
            retry: RetryConfig {
                jitter_ms: 0,
                max_elapsed_ms: Some(120_000),
                ..RetryConfig::default()
            },
        }
    }
}

/// Authenticated read-only client returning provider models with exact decimal values.
///
/// Construction resolves credentials without sending requests. Callers share pacing,
/// concurrency, retries, and a finite attempt allowance. Requests preserve provider response
/// order and values without native instrument mapping or automatic historical continuation.
#[derive(Debug)]
pub struct FinFeedHttpClient {
    client: HttpClient,
    base_url: ConnectionUrl,
    api_key: SecretString,
    concurrency: tokio::sync::Semaphore,
    next_request: tokio::sync::Mutex<Option<time::Instant>>,
    interval: Duration,
    attempts_remaining: AtomicU64,
    retry: RetryManager<Error>,
    cancellation: CancellationToken,
}

impl FinFeedHttpClient {
    /// Constructs transport and resolves credentials once.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials, endpoint, proxy, or bounded controls are invalid,
    /// or transport construction fails.
    pub fn new(api_key: Option<SecretString>, config: FinFeedHttpConfig) -> Result<Self, Error> {
        let api_key = resolve_api_key(api_key)?;
        let mut base_url = Url::parse(config.base_url.expose_secret())
            .map_err(|_| Error::Validation("invalid base URL".into()))?;

        let loopback = base_url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });

        if !(base_url.scheme() == "https" || (base_url.scheme() == "http" && loopback))
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || config
                .base_url
                .expose_secret()
                .contains(api_key.expose_secret())
        {
            return Err(Error::Validation(
                "base URL must be HTTPS or loopback HTTP without credentials, query, or fragment"
                    .into(),
            ));
        }

        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        if let Some(proxy) = &config.proxy_url {
            let proxy = Url::parse(proxy.expose_secret())
                .map_err(|_| Error::Validation("invalid proxy URL".into()))?;

            if !matches!(proxy.scheme(), "http" | "https")
                || proxy.host_str().is_none()
                || proxy.query().is_some()
                || proxy.fragment().is_some()
                || proxy.path() != "/"
            {
                return Err(Error::Validation(
                    "proxy must be HTTP(S) without path, query, or fragment".into(),
                ));
            }
        }

        validate_controls(&config)?;
        let client = HttpClient::builder()
            .timeout_secs(config.timeout_secs)
            .redact_url(true)
            .redirect_policy(HttpRedirectPolicy::Reject)
            .use_system_proxy(false)
            .maybe_proxy_url(
                config
                    .proxy_url
                    .as_ref()
                    .map(|url| url.expose_secret().to_owned()),
            )
            .header_keys(vec!["retry-after".into()])
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.as_str().into(),
            api_key,
            concurrency: tokio::sync::Semaphore::new(config.max_concurrent_requests),
            next_request: tokio::sync::Mutex::new(None),
            interval: Duration::from_millis(config.min_request_interval_ms),
            attempts_remaining: AtomicU64::new(config.attempt_budget),
            retry: RetryManager::new(config.retry),
            cancellation: CancellationToken::new(),
        })
    }

    /// Permanently cancels pending and future requests, including admission and retry waits.
    ///
    /// Construct a new client to make further requests.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Returns the remaining local managed attempt allowance.
    #[must_use]
    pub fn attempts_remaining(&self) -> u64 {
        self.attempts_remaining.load(Ordering::Relaxed)
    }

    /// Retrieves supported exchanges.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Admission, transport, HTTP status, or response decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_exchanges(&self) -> Result<Vec<Exchange>, Error> {
        self.get(&["exchanges"], &()).await
    }

    /// Retrieves one exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_exchange(&self, exchange_id: &str) -> Result<Exchange, Error> {
        self.get(&["exchanges", exchange_id], &()).await
    }

    /// Retrieves one cursor listing page.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_listings(
        &self,
        exchange_id: &str,
        query: &ListingsQuery,
    ) -> Result<MarketsPage, Error> {
        query.validate()?;
        let page: MarketsPage = self
            .get(&["markets", exchange_id, "history"], query)
            .await?;
        check_page(page.data.as_ref().map_or(0, Vec::len), query.limit)?;
        Ok(page)
    }

    /// Retrieves one page of active IDs; absence alone does not establish resolution.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_active(
        &self,
        exchange_id: &str,
        query: &ActiveQuery,
    ) -> Result<Vec<Ustr>, Error> {
        query.validate()?;
        let ids: Vec<Ustr> = self.get(&["markets", exchange_id, "active"], query).await?;
        check_page(ids.len(), query.limit)?;
        Ok(ids)
    }

    /// Retrieves an outcome definition without native instrument mapping.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_market(&self, exchange_id: &str, market_id: &str) -> Result<Market, Error> {
        self.get(&["markets", exchange_id, market_id], &()).await
    }

    /// Retrieves the latest trade and quote independently.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_current(
        &self,
        exchange_id: &str,
        market_id: &str,
    ) -> Result<ActivityCurrent, Error> {
        self.get(&["activity", exchange_id, market_id, "current"], &())
            .await
    }

    /// Retrieves bounded activity with no continuation or continuity guarantee.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_latest(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &ActivityQuery,
    ) -> Result<ActivityBatch, Error> {
        query.validate()?;
        let batch: ActivityBatch = self
            .get(&["activity", exchange_id, market_id, "latest"], query)
            .await?;
        check_page(batch.trades.as_ref().map_or(0, Vec::len), query.limit)?;
        check_page(batch.quotes.as_ref().map_or(0, Vec::len), query.limit)?;
        Ok(batch)
    }

    /// Retrieves one raw trade-history response without completeness or short-window guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_trades_history(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Trade>, Error> {
        query.validate_hourly()?;
        let records: Vec<Trade> = self
            .get(
                &["activity", exchange_id, market_id, "history", "trades"],
                query,
            )
            .await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Retrieves one raw quote-history response without completeness or short-window guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_quotes_history(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Quote>, Error> {
        query.validate_hourly()?;
        let records: Vec<Quote> = self
            .get(
                &["activity", exchange_id, market_id, "history", "quotes"],
                query,
            )
            .await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Retrieves a current book snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_book(&self, exchange_id: &str, market_id: &str) -> Result<Book, Error> {
        self.get(&["orderbook", exchange_id, market_id, "current"], &())
            .await
    }

    /// Retrieves one bounded array of raw historical book events in response order.
    ///
    /// Date/time selectors use provider processing time, which may be absent from the events.
    /// Numeric actions remain uninterpreted. A saturated response may be truncated; a short or
    /// empty response does not establish coverage. No snapshot or continuation is inferred.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_book_history(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<BookEvent>, Error> {
        query.validate_hourly()?;
        let records: Vec<BookEvent> = self
            .get(&["orderbook", exchange_id, market_id, "history"], query)
            .await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Retrieves supported OHLCV periods.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    pub async fn get_periods(&self) -> Result<Vec<OhlcvPeriod>, Error> {
        self.get(&["ohlcv", "periods"], &()).await
    }

    /// Retrieves one historical market OHLCV response without completeness or short-window
    /// guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_ohlcv_history(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &OhlcvQuery,
    ) -> Result<Vec<Ohlcv>, Error> {
        query.validate(false)?;
        let records: Vec<Ohlcv> = self
            .get(&["ohlcv", exchange_id, market_id, "history"], query)
            .await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Retrieves OHLCV from the provider's flat-file activity source.
    ///
    /// The provider describes this source as trade-generated. Returns one response without filtering,
    /// pagination, or fallback to DB-backed OHLCV. A saturated response may be truncated; no
    /// continuation or bound inclusion is inferred. Short or empty responses do not establish
    /// source coverage or completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_ohlcv_flat_file_history(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &FlatFileOhlcvQuery,
    ) -> Result<Vec<Ohlcv>, Error> {
        query.validate()?;
        let records: Vec<Ohlcv> = self
            .get(
                &["activity", exchange_id, market_id, "history", "ohlcv"],
                query,
            )
            .await?;
        check_page(records.len(), query.history.limit)?;
        Ok(records)
    }

    /// Retrieves one exchange-wide OHLCV response with market identifiers retained.
    ///
    /// Both time bounds are required, at most 24 hours apart, with a limit in `[1, 10000]`.
    /// Returns every response row in provider order, without time filtering or native mapping.
    /// Callers partition longer windows explicitly; saturation may truncate markets at the
    /// same timestamp, so advancing by the last timestamp cannot establish completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested row limit.
    pub async fn get_ohlcv_exchange_history(
        &self,
        exchange_id: &str,
        query: &OhlcvQuery,
    ) -> Result<Vec<Ohlcv>, Error> {
        query.validate_exchange()?;
        let records: Vec<Ohlcv> = self.get(&["ohlcv", exchange_id, "history"], query).await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Retrieves the newest market OHLCV bars without time bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    pub async fn get_ohlcv_latest(
        &self,
        exchange_id: &str,
        market_id: &str,
        query: &OhlcvQuery,
    ) -> Result<Vec<Ohlcv>, Error> {
        query.validate(true)?;
        let records: Vec<Ohlcv> = self
            .get(&["ohlcv", exchange_id, market_id, "latest"], query)
            .await?;
        check_page(records.len(), query.limit)?;
        Ok(records)
    }

    /// Collects listings until a null or omitted cursor, rejecting cycles and exceeded traversal
    /// bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if a page fails, a cursor is empty or repeats, or a pagination bound
    /// is exceeded.
    pub async fn get_listings_all(
        &self,
        exchange_id: &str,
        query: &ListingsQuery,
        bounds: Pagination,
    ) -> Result<Vec<Market>, Error> {
        query.validate()?;
        bounds.validate()?;
        let mut query = query.clone();
        let mut cursors = HashSet::new();
        if let Some(cursor) = &query.cursor {
            cursors.insert(cursor.clone());
        }

        let mut records = Vec::new();

        for _ in 0..bounds.max_pages {
            let page = self.get_listings(exchange_id, &query).await?;
            let data = page.data.unwrap_or_default();
            if data.len() > bounds.max_records.saturating_sub(records.len()) {
                return Err(Error::Response("listing record bound exceeded"));
            }

            records.extend(data);
            match page.next_cursor {
                None => return Ok(records),
                Some(cursor) if cursor.is_empty() || !cursors.insert(cursor.clone()) => {
                    return Err(Error::Response("listing cursor is empty or repeated"));
                }
                Some(cursor) => query.cursor = Some(cursor),
            }
        }

        Err(Error::Response("listing page bound exceeded"))
    }

    /// Collects active IDs until an empty page, rejecting duplicate IDs and exceeded bounds.
    ///
    /// A short page alone does not establish exhaustion. Results are observations of a changing
    /// catalog, not a consistent snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if a page fails, an ID is empty or repeats, or a pagination bound
    /// is exceeded.
    pub async fn get_active_all(
        &self,
        exchange_id: &str,
        query: &ActiveQuery,
        bounds: Pagination,
    ) -> Result<Vec<Ustr>, Error> {
        query.validate()?;
        bounds.validate()?;
        let mut query = query.clone();
        let mut seen = HashSet::new();
        let mut ids = Vec::new();

        for _ in 0..bounds.max_pages {
            let page = self.get_active(exchange_id, &query).await?;
            if page.is_empty() {
                return Ok(ids);
            }

            if page.len() > bounds.max_records.saturating_sub(ids.len()) {
                return Err(Error::Response("active ID record bound exceeded"));
            }

            for id in page {
                if id.is_empty() || !seen.insert(id) {
                    return Err(Error::Response("active ID is empty or repeated"));
                }

                ids.push(id);
            }

            query.page = query
                .page
                .checked_add(1)
                .filter(|page| i32::try_from(*page).is_ok())
                .ok_or(Error::Response("active page overflow"))?;
        }

        Err(Error::Response("active page bound exceeded"))
    }

    async fn get<T: DeserializeOwned, Q: Serialize>(
        &self,
        path: &[&str],
        query: &Q,
    ) -> Result<T, Error> {
        if path
            .iter()
            .any(|part| part.trim().is_empty() || matches!(*part, "." | ".."))
        {
            return Err(Error::Validation(
                "path identifiers must not be empty or dot segments".into(),
            ));
        }

        let mut url = Url::parse(self.base_url.expose())
            .map_err(|_| Error::Validation("invalid base URL".into()))?;
        url.path_segments_mut()
            .map_err(|()| Error::Validation("invalid base URL path".into()))?
            .pop_if_empty()
            .extend(path);
        self.retry
            .invocation(
                "FinFeed GET",
                || async {
                    let _permit = self
                        .concurrency
                        .acquire()
                        .await
                        .map_err(|_| Error::Response("admission closed"))?;
                    let mut next = self.next_request.lock().await;
                    if let Some(deadline) = *next {
                        time::sleep_until(deadline).await;
                    }

                    self.attempts_remaining
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                            remaining.checked_sub(1)
                        })
                        .map_err(|_| Error::AttemptBudget)?;

                    *next = Some(time::Instant::now() + self.interval);
                    drop(next);
                    let headers = HashMap::from([
                        (
                            "Authorization".into(),
                            self.api_key.expose_secret().to_owned(),
                        ),
                        ("X-Referrer".into(), "nautilus".into()),
                        ("Accept".into(), "application/json".into()),
                    ]);
                    let response = self
                        .client
                        .request_with_params(
                            Method::GET,
                            url.to_string(),
                            Some(query),
                            Some(headers),
                            None,
                            None,
                            None,
                        )
                        .await?;

                    if !response.status.is_success() {
                        return Err(Error::Http {
                            status: response.status.as_u16(),
                            retry_after_secs: response
                                .headers
                                .get("retry-after")
                                .and_then(|value| value.parse().ok()),
                        });
                    }

                    serde_json::from_slice(&response.body).map_err(Error::from)
                },
                Error::is_retryable,
                Error::Retry,
            )
            .retry_delay(&Error::retry_delay)
            .cancellation_token(&self.cancellation)
            .execute()
            .await
    }
}

fn deserialize_retry<'de, D>(deserializer: D) -> Result<RetryConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let overrides = Map::<String, Value>::deserialize(deserializer)?;
    let value =
        serde_json::to_value(FinFeedHttpConfig::default().retry).map_err(SerdeError::custom)?;
    let mut defaults = Map::deserialize(value).map_err(SerdeError::custom)?;
    defaults.extend(overrides);
    RetryConfig::deserialize(Value::Object(defaults)).map_err(SerdeError::custom)
}

fn validate_controls(config: &FinFeedHttpConfig) -> Result<(), Error> {
    let invalid_control = [
        (
            "http_timeout_secs (http.timeout_secs)",
            config.timeout_secs == 0 || config.timeout_secs > DURATION_MAX_MS / 1_000,
        ),
        (
            "max_concurrent_requests",
            config.max_concurrent_requests == 0
                || config.max_concurrent_requests > tokio::sync::Semaphore::MAX_PERMITS,
        ),
        ("attempt_budget", config.attempt_budget == 0),
        (
            "min_request_interval_ms",
            config.min_request_interval_ms == 0 || config.min_request_interval_ms > DURATION_MAX_MS,
        ),
        (
            "retry_max_elapsed_ms (http.retry.max_elapsed_ms)",
            config
                .retry
                .max_elapsed_ms
                .is_none_or(|ms| ms == 0 || ms > DURATION_MAX_MS),
        ),
        (
            "retry_operation_timeout_ms (http.retry.operation_timeout_ms)",
            config
                .retry
                .operation_timeout_ms
                .is_none_or(|ms| ms == 0 || ms > DURATION_MAX_MS),
        ),
        ("max_retries", config.retry.max_retries == u32::MAX),
        (
            "retry_delay_max_ms (http.retry.max_delay_ms)",
            config.retry.max_delay_ms > DURATION_MAX_MS,
        ),
    ]
    .into_iter()
    .find_map(|(field, invalid)| invalid.then_some(field));

    if let Some(field) = invalid_control {
        return Err(Error::Validation(format!(
            "request controls must be positive and bounded within one day: {field}"
        )));
    }

    ExponentialBackoff::new(
        Duration::from_millis(config.retry.initial_delay_ms),
        Duration::from_millis(config.retry.max_delay_ms),
        config.retry.backoff_factor,
        config.retry.jitter_ms,
        config.retry.immediate_first,
    )
    .map_err(|e| Error::Validation(format!("invalid retry backoff: {e}")))?;
    Ok(())
}

fn check_page(records: usize, limit: u32) -> Result<(), Error> {
    if records > limit as usize {
        return Err(Error::Response("response exceeds requested record limit"));
    }

    Ok(())
}

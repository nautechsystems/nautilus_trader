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

//! CoinAPI REST and Flat Files transport with bounded retries and local admission limits.

use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use jiff::Timestamp;
use nautilus_core::{UUID4, UnixNanos, string::secret::SecretString, time::wall_clock_now};
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::{
    backoff::ExponentialBackoff,
    dst::time,
    http::{
        HttpClient, HttpRedirectPolicy, HttpResponse, RETRY_AFTER, StatusCode, parse_retry_after,
    },
    retry::{RetryConfig, RetryError, RetryManager},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    error::Error,
    models::{Book, Exchange, Ohlcv, Quote, Symbol, Trade},
    parse::{CoinApiInstrument, parse_instruments_with_precisions, validate_price_precisions},
    query::{
        BookQuery, ExchangesQuery, HISTORY_LIMIT_MAX, HistoryQuery, HistoryRange, OhlcvQuery,
        SymbolsQuery, TradesQuery, provider_time,
    },
};
use crate::common::{
    consts::COINAPI_NAUTILUS_REFERRER, credential::is_valid_api_key, enums::OhlcvPeriod,
};

/// Primary CoinAPI Market Data REST endpoint, including its version prefix.
pub const REST_URL: &str = "https://rest.coinapi.io/v1/";

static FLAT_FILES_URL: LazyLock<Url> = LazyLock::new(|| {
    Url::parse("https://s3.flatfiles.coinapi.io/").expect("valid Flat Files endpoint")
});

/// Transport configuration for the raw HTTP client.
///
/// # Credit Allowance
///
/// `credit_budget` is shared by REST requests on this instance:
///
/// - Each attempt reserves ceil(requested limit / 100) credits, or one credit
///   when the request has no record limit.
/// - A successful historical page keeps ceil(returned records / 100) credits,
///   with a minimum of one, and refunds the rest of its reservation.
/// - Failed or canceled attempts keep their full reservation.
///
/// This is a local safety limit, not the provider's billing ledger, and it does
/// not coordinate with other clients. Flat Files downloads use separate provider billing and do
/// not consume this REST allowance. Both services share the concurrency and retry controls.
#[derive(Debug, Clone)]
pub struct CoinApiHttpConfig {
    /// Base URL including the version prefix; accepts local simulated peers.
    pub base_url: String,
    /// Optional explicit proxy URL; ambient system proxies remain disabled.
    pub proxy_url: Option<SecretString>,
    /// Maximum simultaneous requests for this client instance.
    pub max_concurrent_requests: usize,
    /// Minimum spacing in milliseconds between REST attempts on this client. `None` disables pacing.
    pub min_request_interval_ms: Option<u64>,
    /// Lifetime credit allowance for this instance; see the struct docs for accounting.
    pub credit_budget: u64,
    /// Retry policy; the elapsed budget and any configured attempt timeout must be positive.
    pub retry: RetryConfig,
}

impl Default for CoinApiHttpConfig {
    fn default() -> Self {
        Self {
            base_url: REST_URL.into(),
            proxy_url: None,
            max_concurrent_requests: 1,
            min_request_interval_ms: None,
            credit_budget: 100,
            retry: RetryConfig {
                jitter_ms: 0,
                max_elapsed_ms: Some(120_000),
                ..RetryConfig::default()
            },
        }
    }
}

/// Records gathered from one or more historical pages, in ascending time order.
///
/// `is_truncated` reports that the credit budget stopped paging before the window or limit was
/// exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPages<T> {
    pub records: Vec<T>,
    pub is_truncated: bool,
}

/// Authenticated, data-only CoinAPI HTTP client.
///
/// REST requests return raw protocol models. The single-page historical methods fetch exactly one
/// page; callers must not infer completeness from a full page. The range methods page through a
/// whole window.
/// Range requests run serially so each sizes its pages after earlier ranges settle their credits.
/// Flat Files downloads stream raw bytes to disk without requiring instrument definitions.
#[derive(Debug)]
pub struct CoinApiHttpClient {
    api_key: SecretString,
    base_url: Url,
    client: HttpClient,
    flat_files_client: HttpClient,
    concurrency: tokio::sync::Semaphore,
    rest_not_before: Mutex<Option<time::Instant>>,
    min_request_interval: Option<Duration>,
    ranges: tokio::sync::Mutex<()>,
    flat_files_retry: RetryManager<Error>,
    rest_retry: RetryManager<Error>,
    rest_timeout_ms: Option<u64>,
    cancellation: CancellationToken,
    credits_remaining: AtomicU64,
}

impl CoinApiHttpClient {
    /// Creates a client using an explicit secret and endpoint configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the credentials, endpoint, limits, or transport configuration are
    /// invalid.
    pub fn new(api_key: SecretString, config: CoinApiHttpConfig) -> Result<Self, Error> {
        if !is_valid_api_key(api_key.expose_secret()) {
            return Err(Error::Validation(
                "API key must contain visible ASCII characters".into(),
            ));
        }

        let mut base_url = Url::parse(&config.base_url)
            .map_err(|_| Error::Validation("invalid base URL".into()))?;

        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::Validation(
                "base URL must be HTTP(S) without credentials, query, or fragment".into(),
            ));
        }

        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        if config.max_concurrent_requests == 0
            || config.max_concurrent_requests > tokio::sync::Semaphore::MAX_PERMITS
            || config.credit_budget == 0
            || config.retry.max_elapsed_ms.is_none_or(|ms| ms == 0)
            || config.retry.operation_timeout_ms == Some(0)
        {
            return Err(Error::Validation(
                "concurrency, credits, elapsed budget, and configured timeout must be positive and bounded"
                    .into(),
            ));
        }

        let min_request_interval = config.min_request_interval_ms.map(Duration::from_millis);
        if min_request_interval.is_some_and(|interval| {
            interval.is_zero() || time::Instant::now().checked_add(interval).is_none()
        }) {
            return Err(Error::Validation(
                "min_request_interval_ms must be positive and fit the monotonic clock".into(),
            ));
        }

        ExponentialBackoff::new(
            Duration::from_millis(config.retry.initial_delay_ms),
            Duration::from_millis(config.retry.max_delay_ms),
            config.retry.backoff_factor,
            config.retry.jitter_ms,
            config.retry.immediate_first,
        )
        .map_err(|e| Error::Validation(e.to_string()))?;

        let client = build_client(&api_key, &config, "application/json")?;
        let flat_files_client = build_client(&api_key, &config, "application/xml")?;
        let mut rest_retry = config.retry.clone();
        let rest_timeout_ms = rest_retry.operation_timeout_ms.take();
        Ok(Self {
            api_key,
            base_url,
            client,
            flat_files_client,
            concurrency: tokio::sync::Semaphore::new(config.max_concurrent_requests),
            rest_not_before: Mutex::default(),
            min_request_interval,
            ranges: tokio::sync::Mutex::new(()),
            flat_files_retry: RetryManager::new(config.retry),
            rest_retry: RetryManager::new(rest_retry),
            rest_timeout_ms,
            cancellation: CancellationToken::new(),
            credits_remaining: AtomicU64::new(config.credit_budget),
        })
    }

    /// Cancels pending and future requests on this instance.
    pub fn cancel(&self) {
        // Serialize cancellation with REST credit reservation
        let _not_before = self.rest_not_before.lock();
        self.cancellation.cancel();
    }

    /// Returns the remaining conservative local credit allowance.
    #[must_use]
    pub fn credits_remaining(&self) -> u64 {
        self.credits_remaining.load(Ordering::Relaxed)
    }

    /// Downloads one Flat Files object, replacing the destination only after the stream succeeds.
    ///
    /// Authentication is automatic. The destination's parent directories are created as needed.
    /// Downloads share this client's retry, cancellation, proxy, and concurrency controls, and
    /// reject redirects. Flat Files usage does not consume the local REST credit allowance.
    ///
    /// # Errors
    ///
    /// Returns an error if the bucket or object key contains empty or dot segments, the request
    /// fails, retry controls stop the download, or a file operation fails.
    pub async fn download_flat_file(
        &self,
        bucket: &str,
        object_key: &str,
        filepath: &Path,
    ) -> Result<(), Error> {
        let path: Vec<_> = std::iter::once(bucket)
            .chain(object_key.split('/'))
            .collect();
        let url = request_url(&FLAT_FILES_URL, &path)?;
        self.download_file(url, filepath).await
    }

    /// Retrieves exchange metadata with optional filtering.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_exchanges(&self, query: &ExchangesQuery) -> Result<Vec<Exchange>, Error> {
        self.get(&["exchanges"], query, 1, |_| 1).await
    }

    /// Retrieves active symbols for one exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_symbols(
        &self,
        exchange_id: &str,
        query: &SymbolsQuery,
    ) -> Result<Vec<Symbol>, Error> {
        self.get(&["symbols", exchange_id, "active"], query, 1, |_| 1)
            .await
    }

    /// Loads native instruments from the active symbols of one exchange.
    ///
    /// Only spot, perpetual, and futures symbols are parsed. Symbols of those types that cannot
    /// be mapped to a native instrument are logged and skipped. An instrument listed in
    /// `price_precisions` takes that price precision instead of the CoinAPI price resolution.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails. Validation rejects a price precision beyond the Nautilus
    /// maximum.
    pub async fn request_instruments(
        &self,
        exchange_id: &str,
        query: &SymbolsQuery,
        price_precisions: &HashMap<InstrumentId, u8>,
        ts_init: UnixNanos,
    ) -> Result<Vec<CoinApiInstrument>, Error> {
        validate_price_precisions(price_precisions)?;

        let symbols = self.get_symbols(exchange_id, query).await?;
        let instruments = parse_instruments_with_precisions(&symbols, price_precisions, ts_init);

        Ok(instruments)
    }

    /// Retrieves a current L2 market-by-price book for one symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_book(&self, symbol_id: &str, query: &BookQuery) -> Result<Book, Error> {
        self.get_book_at("orderbooks", symbol_id, query).await
    }

    /// Retrieves a current L3 market-by-order book for one symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_book_l3(&self, symbol_id: &str, query: &BookQuery) -> Result<Book, Error> {
        self.get_book_at("orderbooks3", symbol_id, query).await
    }

    async fn get_book_at(
        &self,
        family: &str,
        symbol_id: &str,
        query: &BookQuery,
    ) -> Result<Book, Error> {
        if query
            .limit_levels
            .is_some_and(|levels| levels == 0 || levels > i32::MAX as u32)
        {
            return Err(Error::Validation(
                "limit_levels must be a positive int32".into(),
            ));
        }

        self.get(&[family, symbol_id, "current"], query, 1, |_| 1)
            .await
    }

    /// Retrieves one historical quote page with an explicit window and limit.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_quotes(
        &self,
        symbol_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Quote>, Error> {
        query.validate()?;
        let cost = credits(query.limit);
        self.get(&["quotes", symbol_id, "history"], query, cost, billed)
            .await
    }

    /// Retrieves historical quotes across a window, ordered and paged by `time_exchange`.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, decoding fails, or response ordering fails.
    pub async fn get_quotes_range(
        &self,
        symbol_id: &str,
        range: &HistoryRange,
    ) -> Result<HistoryPages<Quote>, Error> {
        self.get_range(
            range,
            1,
            |quote: &Quote| quote.time_exchange,
            |page| async move { self.get_quotes(symbol_id, &page).await },
        )
        .await
    }

    /// Retrieves one historical trade page with an explicit window and limit.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_trades(
        &self,
        symbol_id: &str,
        query: &TradesQuery,
    ) -> Result<Vec<Trade>, Error> {
        query.range.validate()?;
        let cost = credits(query.range.limit);
        self.get(&["trades", symbol_id, "history"], query, cost, billed)
            .await
    }

    /// Retrieves historical trades across a window, ordered and paged by `time_exchange`.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, decoding fails, or response ordering fails.
    pub async fn get_trades_range(
        &self,
        symbol_id: &str,
        range: &HistoryRange,
    ) -> Result<HistoryPages<Trade>, Error> {
        self.get_range(
            range,
            1,
            |trade: &Trade| trade.time_exchange,
            |page| async move {
                let query = TradesQuery {
                    range: page,
                    include_id: None,
                };

                self.get_trades(symbol_id, &query).await
            },
        )
        .await
    }

    /// Retrieves one historical OHLCV page for the requested period.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, or decoding fails.
    pub async fn get_ohlcv(
        &self,
        symbol_id: &str,
        query: &OhlcvQuery,
    ) -> Result<Vec<Ohlcv>, Error> {
        query.range.validate()?;
        if query.period_id.as_ref().trim().is_empty() {
            return Err(Error::Validation("period_id must not be empty".into()));
        }

        let cost = credits(query.range.limit);
        self.get(&["ohlcv", symbol_id, "history"], query, cost, billed)
            .await
    }

    /// Retrieves OHLCV periods starting within a window, ordered and paged by
    /// `time_period_start`.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, the credit budget is exhausted, the transport or HTTP
    /// request fails, decoding fails, or response ordering fails.
    pub async fn get_ohlcv_range(
        &self,
        symbol_id: &str,
        period_id: OhlcvPeriod,
        range: &HistoryRange,
    ) -> Result<HistoryPages<Ohlcv>, Error> {
        self.get_range(
            range,
            2,
            |ohlcv: &Ohlcv| ohlcv.time_period_start,
            |page| async move {
                let query = OhlcvQuery {
                    period_id,
                    range: page,
                };

                self.get_ohlcv(symbol_id, &query).await
            },
        )
        .await
    }

    // Refetch the trailing timestamp group of a full page to avoid splitting equal-time records
    async fn get_range<T, F, Fut>(
        &self,
        range: &HistoryRange,
        min_page_limit: usize,
        key: fn(&T) -> Timestamp,
        mut fetch: F,
    ) -> Result<HistoryPages<T>, Error>
    where
        F: FnMut(HistoryQuery) -> Fut,
        Fut: Future<Output = Result<Vec<T>, Error>>,
    {
        // A window between two 100 ns grid points holds no CoinAPI records
        if range.time_start < range.time_end
            && provider_time(range.time_start)? >= provider_time(range.time_end)?
        {
            return Ok(HistoryPages {
                records: Vec::new(),
                is_truncated: false,
            });
        }

        let _range = self.ranges.lock().await;
        let mut records = Vec::new();
        let mut cursor = range.time_start;
        loop {
            let remaining = range.limit.map(|limit| limit.get() - records.len());
            let affordable =
                usize::try_from(self.credits_remaining().saturating_mul(100)).unwrap_or(usize::MAX);
            let page_limit = remaining
                .unwrap_or(usize::MAX)
                .max(min_page_limit)
                .min(HISTORY_LIMIT_MAX as usize)
                .min(affordable);

            if page_limit == 0 {
                if records.is_empty() {
                    return Err(Error::CreditBudget);
                }

                return Ok(HistoryPages {
                    records,
                    is_truncated: true,
                });
            }

            let page = fetch(HistoryQuery {
                time_start: cursor,
                time_end: range.time_end,
                limit: page_limit as u32,
            })
            .await;

            // Another request or a failed attempt spent credits after this page was sized
            let mut page = match page {
                Ok(page) => page,
                Err(Error::CreditBudget) => continue,
                Err(e) => return Err(e),
            };

            if page.windows(2).any(|pair| key(&pair[1]) < key(&pair[0])) {
                return Err(Error::Response(
                    "history page is not in ascending time order".into(),
                ));
            }

            let is_full = page.len() >= page_limit;
            let reaches_end = page
                .last()
                .is_some_and(|record| key(record) >= range.time_end);

            // CoinAPI can substitute nearby OHLCV periods for an empty window
            page.retain(|record| (cursor..range.time_end).contains(&key(record)));
            if let Some(remaining) = remaining {
                page.truncate(remaining);
            }

            // A full page emptied by filtering held only fallback periods outside the window
            if !is_full || reaches_end || page.is_empty() || remaining == Some(page.len()) {
                records.extend(page);
                return Ok(HistoryPages {
                    records,
                    is_truncated: false,
                });
            }

            let last = key(&page[page.len() - 1]);
            let boundary = page.partition_point(|record| key(record) < last);

            if boundary == 0 {
                // Filtering a leading period can leave one timestamp ahead of the cursor
                if page.len() < page_limit && last > cursor {
                    cursor = last;
                    continue;
                }

                return Err(Error::Response(format!(
                    "{page_limit} records share timestamp {last}, so paging cannot advance"
                )));
            }

            page.truncate(boundary);
            records.extend(page);
            cursor = last;
        }
    }

    // Returns the unbilled part of the reservation before the permit is released, so the next
    // admitted request sees the settled budget
    async fn get<T: DeserializeOwned, Q: Serialize>(
        &self,
        path: &[&str],
        query: &Q,
        cost: u64,
        billed: fn(&T) -> u64,
    ) -> Result<T, Error> {
        let url = request_url(&self.base_url, path)?;
        let attempt_retry_after = Mutex::new(None);

        self.rest_retry
            .invocation(
                "CoinAPI GET",
                || async {
                    *attempt_retry_after.lock() = None;
                    let queued = log::log_enabled!(log::Level::Debug).then(time::Instant::now);
                    let (_permit, remaining) = self.admit_rest(cost).await?;
                    let started = queued.map(|_| time::Instant::now());
                    let mut status = None;
                    let mut retry_after = None;

                    let request = self.client.get_with_params_on_headers(url.as_str(), query, |code, headers| {
                        status = Some(code.as_u16());
                        retry_after = headers
                            .get(RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(parse_rest_retry_after);
                        *attempt_retry_after.lock() = self.observe_rest_retry_after(code, retry_after);
                    });

                    // Box the transport future once to keep simulated REST futures small
                    let response = self
                        .within_rest_timeout(remaining, Box::pin(request))
                        .await
                        .and_then(|result| result.map_err(Error::from));

                    let elapsed = started.map(|started| started.elapsed());

                    let result = response.and_then(|response| {
                        let value = self.decode(&response, retry_after)?;
                        self.credits_remaining
                            .fetch_add(cost.saturating_sub(billed(&value)), Ordering::Relaxed);
                        Ok(value)
                    });

                    if let (Some(queued), Some(started), Some(elapsed)) = (queued, started, elapsed) {
                        log::debug!(
                            "CoinAPI REST GET endpoint={} admission_wait_ms={} http_elapsed_ms={} status={status:?} credits_remaining={}",
                            path.first().copied().unwrap_or("unknown"),
                            started.duration_since(queued).as_millis(),
                            elapsed.as_millis(),
                            self.credits_remaining(),
                        );
                    }

                    result
                },
                Error::is_retryable,
                Error::Retry,
            )
            .retry_delay(&|_| {
                attempt_retry_after
                    .lock()
                    .map(|(observed, delay)| delay.saturating_sub(observed.elapsed()))
            })
            .cancellation_token(&self.cancellation)
            .execute()
            .await
    }

    fn observe_rest_retry_after(
        &self,
        status: StatusCode,
        retry_after: Option<Duration>,
    ) -> Option<(time::Instant, Duration)> {
        if !Error::is_retryable_status(status.as_u16()) {
            return None;
        }

        let observed = time::Instant::now();

        if let Some(deadline) = retry_after.and_then(|delay| observed.checked_add(delay)) {
            self.record_rest_cooldown(deadline);
        }

        if status == StatusCode::TOO_MANY_REQUESTS || retry_after.is_some() {
            let retry_after_secs = retry_after.map(retry_after_seconds);
            log::warn!(
                "CoinAPI REST received HTTP {}, retry_after_secs={retry_after_secs:?}",
                status.as_u16()
            );
        }

        retry_after.map(|delay| (observed, delay))
    }

    async fn admit_rest(
        &self,
        cost: u64,
    ) -> Result<(tokio::sync::SemaphorePermit<'_>, Option<Duration>), Error> {
        let mut remaining = self.rest_timeout_ms.map(Duration::from_millis);

        loop {
            let deadline = *self.rest_not_before.lock();

            let (wait, permit) = if let Some(deadline) =
                deadline.filter(|deadline| *deadline > time::Instant::now())
            {
                (deadline, None)
            } else {
                let started = remaining.map(|_| time::Instant::now());
                let permit = self
                    .within_rest_timeout(remaining, self.concurrency.acquire())
                    .await?
                    .map_err(|_| Error::Validation("request admission closed".into()))?;
                remaining = remaining
                    .zip(started)
                    .map(|(budget, started)| budget.saturating_sub(started.elapsed()));

                if remaining.is_some_and(|duration| duration.is_zero()) {
                    return Err(self.rest_operation_timeout());
                }

                let mut not_before = self.rest_not_before.lock();
                let now = time::Instant::now();

                if let Some(deadline) = not_before.filter(|deadline| *deadline > now) {
                    (deadline, Some(permit))
                } else {
                    *not_before = self.reserve_rest_attempt(cost)?;
                    return Ok((permit, remaining));
                }
            };

            self.wait_rest_admission(wait, permit, cost).await?;
        }
    }

    fn reserve_rest_attempt(&self, cost: u64) -> Result<Option<time::Instant>, Error> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Retry(RetryError::Canceled));
        }

        self.credits_remaining
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(cost)
            })
            .map_err(|_| Error::CreditBudget)?;

        let Some(interval) = self.min_request_interval else {
            return Ok(None);
        };

        let Some(next) = time::Instant::now().checked_add(interval) else {
            self.credits_remaining.fetch_add(cost, Ordering::Relaxed);
            return Err(Error::Validation(
                "REST pacing deadline exceeds the clock range".into(),
            ));
        };

        Ok(Some(next))
    }

    async fn wait_rest_admission(
        &self,
        deadline: time::Instant,
        permit: Option<tokio::sync::SemaphorePermit<'_>>,
        cost: u64,
    ) -> Result<(), Error> {
        // Keep long provider deadlines out of the timer's registration arithmetic
        let now = time::Instant::now();
        let wake = deadline.min(now + Duration::from_secs(60));

        let permit = if permit.is_none() && self.credits_remaining() < cost {
            match time::timeout(
                wake.saturating_duration_since(now),
                self.concurrency.acquire(),
            )
            .await
            {
                Ok(result) => {
                    Some(result.map_err(|_| Error::Validation("request admission closed".into()))?)
                }
                Err(_) => return Ok(()),
            }
        } else {
            permit
        };

        if permit.is_some() && self.credits_remaining() < cost {
            return Err(Error::CreditBudget);
        }

        drop(permit);

        time::sleep_until(wake).await;
        Ok(())
    }

    async fn within_rest_timeout<F: Future>(
        &self,
        remaining: Option<Duration>,
        future: F,
    ) -> Result<F::Output, Error> {
        match remaining {
            Some(duration) => time::timeout(duration, future)
                .await
                .map_err(|_| self.rest_operation_timeout()),
            None => Ok(future.await),
        }
    }

    fn rest_operation_timeout(&self) -> Error {
        Error::Retry(RetryError::OperationTimeout {
            timeout_ms: self.rest_timeout_ms.unwrap_or_default(),
        })
    }

    async fn download_file(&self, url: Url, filepath: &Path) -> Result<(), Error> {
        self.flat_files_retry
            .invocation(
                "CoinAPI Flat Files GET",
                || async {
                    let _permit = self
                        .concurrency
                        .acquire()
                        .await
                        .map_err(|_| Error::Validation("request admission closed".into()))?;
                    let (partial, mut file) = PartialDownload::create(filepath).await?;
                    let mut response = self.flat_files_client.get_stream(url.to_string()).await?;
                    if !response.status().is_success() {
                        return Err(Error::Http {
                            status: response.status().as_u16(),
                            retry_after_secs: response
                                .headers()
                                .get("retry-after")
                                .and_then(|value| value.to_str().ok())
                                .and_then(|value| value.parse().ok()),
                        });
                    }

                    while let Some(chunk) = response.chunk().await? {
                        file.write_all(&chunk).await?;
                    }

                    file.flush().await?;
                    file.sync_all().await?;
                    drop(file);
                    tokio::fs::rename(&partial.path, filepath).await?;
                    Ok(())
                },
                Error::is_retryable,
                Error::Retry,
            )
            .retry_delay(&Error::retry_delay)
            .cancellation_token(&self.cancellation)
            .execute()
            .await
    }

    fn decode<T: DeserializeOwned>(
        &self,
        response: &HttpResponse,
        retry_after: Option<Duration>,
    ) -> Result<T, Error> {
        for (key, value) in &response.headers {
            if key.starts_with("x-credits-") || key == "x-ratelimit-request-cost" {
                log::debug!("CoinAPI usage {key}={value}");
            }
        }

        if !response.status.is_success() {
            let status = response.status.as_u16();
            let retry_after_secs = retry_after.map(retry_after_seconds);

            return Err(match serde_json::from_slice::<VenueError>(&response.body) {
                Ok(error) => Error::Venue {
                    status,
                    message: error
                        .error
                        .replace(self.api_key.expose_secret(), "<redacted>"),
                    retry_after_secs,
                },
                Err(_) => Error::Http {
                    status,
                    retry_after_secs,
                },
            });
        }

        serde_json::from_slice(&response.body).map_err(Error::Parse)
    }

    fn record_rest_cooldown(&self, deadline: time::Instant) {
        let mut not_before = self.rest_not_before.lock();
        *not_before = (*not_before).max(Some(deadline));
    }
}

fn parse_rest_retry_after(value: &str) -> Option<Duration> {
    let now = Timestamp::try_from(wall_clock_now()).ok()?;
    parse_retry_after(value, now)
}

fn retry_after_seconds(delay: Duration) -> u64 {
    delay
        .as_secs()
        .saturating_add(u64::from(delay.subsec_nanos() != 0))
}

#[derive(Debug)]
struct PartialDownload {
    path: PathBuf,
}

impl PartialDownload {
    async fn create(filepath: &Path) -> Result<(Self, tokio::fs::File), Error> {
        let mut name = filepath
            .file_name()
            .ok_or_else(|| Error::Validation("download destination must name a file".into()))?
            .to_os_string();
        name.push(format!(".{}.part", UUID4::new()));
        let path = filepath.with_file_name(name);
        if let Some(parent) = filepath
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok((Self { path }, tokio::fs::File::from_std(file)))
    }
}

impl Drop for PartialDownload {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn build_client(
    api_key: &SecretString,
    config: &CoinApiHttpConfig,
    accept: &str,
) -> Result<HttpClient, Error> {
    HttpClient::builder()
        .headers(HashMap::from([
            ("X-CoinAPI-Key".into(), api_key.expose_secret().to_owned()),
            ("X-Referrer".into(), COINAPI_NAUTILUS_REFERRER.into()),
            ("Accept".into(), accept.to_owned()),
        ]))
        .redirect_policy(HttpRedirectPolicy::Reject)
        .use_system_proxy(false)
        .maybe_proxy_url(
            config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_owned()),
        )
        .header_keys(vec![
            "retry-after".into(),
            "x-credits-used".into(),
            "x-credits-remaining".into(),
            "x-credits-limit".into(),
            "x-ratelimit-request-cost".into(),
        ])
        .build()
        .map_err(Error::from)
}

fn request_url(base: &Url, path: &[&str]) -> Result<Url, Error> {
    if path
        .iter()
        .any(|part| part.trim().is_empty() || matches!(*part, "." | ".."))
    {
        return Err(Error::Validation(
            "path identifiers must not be empty or dot segments".into(),
        ));
    }

    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| Error::Validation("base URL cannot contain path segments".into()))?
        .pop_if_empty()
        .extend(path);
    Ok(url)
}

#[derive(Deserialize)]
struct VenueError {
    error: String,
}

fn credits(points: u32) -> u64 {
    u64::from(points.div_ceil(100))
}

// CoinAPI bills each 100 returned records, and at least one credit per request
#[expect(
    clippy::ptr_arg,
    reason = "`get` passes the decoded response type by reference"
)]
fn billed<T>(records: &Vec<T>) -> u64 {
    credits(u32::try_from(records.len()).unwrap_or(u32::MAX)).max(1)
}

#[cfg(test)]
#[cfg(not(all(feature = "simulation", madsim)))]
mod tests {
    use std::{
        future::poll_fn,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::Poll,
    };

    use axum::{
        Router,
        http::{StatusCode, header::LOCATION},
        routing::get,
        serve,
    };
    use nautilus_common::testing::wait_until_async;
    use parking_lot::Mutex;
    use rstest::rstest;
    use tempfile::TempDir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Notify,
    };

    use super::*;

    #[rstest]
    #[case::nested(vec!["coinapi", "T-TRADES", "D-20261001", "name+symbol.csv.gz"], "/coinapi/T-TRADES/D-20261001/name+symbol.csv.gz")]
    #[case::reserved(vec!["coinapi", "data ?#.csv.gz"], "/coinapi/data%20%3F%23.csv.gz")]
    fn file_url_preserves_object_segments(#[case] path: Vec<&str>, #[case] expected_path: &str) {
        let url = request_url(&FLAT_FILES_URL, &path).unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("s3.flatfiles.coinapi.io"));
        assert_eq!(url.path(), expected_path);
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[rstest]
    #[case(vec!["", "data.csv.gz"])]
    #[case(vec!["coinapi", ""])]
    #[case(vec!["coinapi", "a", "..", "data.csv.gz"])]
    #[case(vec!["coinapi", ".", "data.csv.gz"])]
    fn file_url_rejects_empty_and_dot_segments(#[case] path: Vec<&str>) {
        let error = request_url(&FLAT_FILES_URL, &path).unwrap_err();

        assert!(
            matches!(error, Error::Validation(message) if message == "path identifiers must not be empty or dot segments")
        );
    }

    #[rstest]
    #[case::status(false)]
    #[case::truncated_body(true)]
    #[tokio::test]
    async fn download_retries_with_headers_and_preserves_wire_bytes(#[case] truncated: bool) {
        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("nested").join("export.csv.gz");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body: &[u8] = b"\x1f\x8b\x08\x00opaque-compressed-data\x00";

        let peer = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                captured.lock().push(request);

                if attempt == 0 {
                    let response: &[u8] = if truncated {
                        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort"
                    } else {
                        b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    };

                    socket.write_all(response).await.unwrap();
                } else {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(body).await.unwrap();
                }

                socket.shutdown().await.unwrap();
            }
        });

        let client = CoinApiHttpClient::new("test-api-key".into(), config()).unwrap();
        client
            .download_file(
                Url::parse(&format!("http://{addr}/archive.csv.gz")).unwrap(),
                &filepath,
            )
            .await
            .unwrap();
        peer.await.unwrap();

        let requests = requests.lock();
        assert_eq!(requests.len(), 2);

        for request in requests.iter() {
            assert_eq!(request.lines().next(), Some("GET /archive.csv.gz HTTP/1.1"));

            for (name, expected) in [
                ("x-coinapi-key", "test-api-key"),
                ("x-referrer", "nautilus"),
                ("accept", "application/xml"),
            ] {
                let values: Vec<_> = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.trim())
                    .collect();
                assert_eq!(values, vec![expected]);
            }
        }

        assert_eq!(std::fs::read(&filepath).unwrap(), body);
        assert_eq!(client.credits_remaining(), 7);
        assert_eq!(
            std::fs::read_dir(filepath.parent().unwrap())
                .unwrap()
                .count(),
            1
        );
        assert!(!format!("{client:?}").contains("test-api-key"));
    }

    #[rstest]
    #[case::redirect(StatusCode::FOUND)]
    #[case::denied(StatusCode::FORBIDDEN)]
    #[tokio::test]
    async fn failed_download_preserves_destination_and_rejects_redirects(
        #[case] status: StatusCode,
    ) {
        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("export.csv.gz");
        std::fs::write(&filepath, b"existing export").unwrap();
        let target_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&target_calls);
        let target = Router::new().route(
            "/target",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async { "must not receive credentials" }
            }),
        );

        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let target_server = tokio::spawn(async move {
            serve(target_listener, target).await.unwrap();
        });

        let source_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&source_calls);
        let source = Router::new().route(
            "/archive",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { (status, [(LOCATION, format!("http://{target_addr}/target"))]) }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let source_server = tokio::spawn(async move { serve(listener, source).await.unwrap() });
        let client = CoinApiHttpClient::new("test-api-key".into(), config()).unwrap();
        let error = client
            .download_file(
                Url::parse(&format!("http://{addr}/archive")).unwrap(),
                &filepath,
            )
            .await
            .unwrap_err();
        source_server.abort();
        target_server.abort();
        let _ = source_server.await;
        let _ = target_server.await;

        let Error::Http {
            status: actual_status,
            retry_after_secs,
        } = error
        else {
            panic!("expected HTTP status error, was {error:?}")
        };

        assert_eq!((actual_status, retry_after_secs), (status.as_u16(), None));
        assert_eq!(source_calls.load(Ordering::SeqCst), 1);
        assert_eq!(target_calls.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(&filepath).unwrap(), b"existing export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(client.credits_remaining(), 7);
    }

    #[cfg(unix)]
    #[rstest]
    fn final_write_failure_preserves_destination() {
        const CHILD: &str = "NAUTILUS_COINAPI_WRITE_LIMIT_CHILD";

        if let Some(filepath) = std::env::var_os(CHILD) {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let addr = listener.local_addr().unwrap();
                    let peer = tokio::spawn(async move {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        read_request(&mut socket).await;
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx")
                            .await
                            .unwrap();
                        socket.shutdown().await.unwrap();
                    });

                    let client = CoinApiHttpClient::new("test-api-key".into(), config()).unwrap();
                    let result = client
                        .download_file(
                            Url::parse(&format!("http://{addr}/archive")).unwrap(),
                            Path::new(&filepath),
                        )
                        .await;
                    peer.await.unwrap();

                    assert!(
                        matches!(result, Err(Error::Io(ref error)) if error.kind() == std::io::ErrorKind::FileTooLarge),
                        "expected final write failure, was {result:?}"
                    );
                });

            return;
        }

        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("export.csv.gz");
        std::fs::write(&filepath, b"existing export").unwrap();

        // Isolate the zero-byte file limit and ignored SIGXFSZ from parallel tests
        let output = std::process::Command::new("bash")
            .args([
                "-c",
                "trap '' XFSZ; ulimit -f 0; exec \"$@\"",
                "coinapi-write-limit",
            ])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "http::client::tests::final_write_failure_preserves_destination",
                "--nocapture",
            ])
            .env(CHILD, &filepath)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        assert_eq!(std::fs::read(&filepath).unwrap(), b"existing export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[rstest]
    #[case::timeout(false)]
    #[case::canceled(true)]
    #[tokio::test]
    async fn interrupted_download_removes_partial_file(#[case] cancel: bool) {
        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("export.csv.gz");
        let destination = filepath.clone();
        let started = Arc::new(Notify::new());
        let signal = Arc::clone(&started);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nx")
                .await
                .unwrap();
            signal.notify_one();
            std::future::pending::<()>().await;
        });

        let mut config = config();
        config.retry.max_retries = 0;
        let timeout_ms = config.retry.operation_timeout_ms.unwrap();
        let client = Arc::new(CoinApiHttpClient::new("test-api-key".into(), config).unwrap());
        let transfer_client = Arc::clone(&client);
        let transfer = tokio::spawn(async move {
            transfer_client
                .download_file(
                    Url::parse(&format!("http://{addr}/archive")).unwrap(),
                    &destination,
                )
                .await
        });

        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        wait_until_async(
            || {
                std::future::ready(
                    std::fs::read_dir(directory.path())
                        .unwrap()
                        .any(|entry| entry.unwrap().metadata().unwrap().len() == 1),
                )
            },
            Duration::from_secs(2),
        )
        .await;

        if cancel {
            client.cancel();
        } else {
            tokio::time::pause();
            tokio::time::advance(Duration::from_millis(timeout_ms)).await;
        }

        let error = transfer.await.unwrap().unwrap_err();

        if !cancel {
            tokio::time::resume();
        }

        peer.abort();
        let _ = peer.await;

        let Error::Retry(actual) = error else {
            panic!("expected retry control error, was {error:?}")
        };

        let expected = if cancel {
            RetryError::Canceled
        } else {
            RetryError::OperationTimeout { timeout_ms }
        };

        assert_eq!(actual, expected);
        assert!(!filepath.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        assert_eq!(client.credits_remaining(), 7);
    }

    #[tokio::test]
    async fn rest_cooldown_leaves_the_transport_free_for_flat_files() {
        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("export.csv");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = Url::parse(&format!("http://{address}/archive")).unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata")
                .await
                .unwrap();
            request
        });

        let client = CoinApiHttpClient::new(
            "test-api-key".into(),
            CoinApiHttpConfig {
                base_url: format!("http://{address}/v1/"),
                ..config()
            },
        )
        .unwrap();

        client.record_rest_cooldown(time::Instant::now() + Duration::from_secs(60));
        let query = ExchangesQuery::default();
        let request = client.get_exchanges(&query);
        tokio::pin!(request);
        poll_fn(|cx| {
            assert!(request.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        time::timeout(Duration::from_secs(3), client.download_file(url, &filepath))
            .await
            .unwrap()
            .unwrap();
        let request = server.await.unwrap();
        assert_eq!(request.lines().next(), Some("GET /archive HTTP/1.1"));
        assert_eq!(std::fs::read(&filepath).unwrap(), b"data");
        assert_eq!(client.credits_remaining(), 7);
    }

    #[rstest]
    #[case::pacing_longer(3_000, 1)]
    #[case::cooldown_longer(1_000, 3)]
    #[tokio::test(start_paused = true)]
    async fn rest_admission_honors_pacing_and_cooldown(
        #[case] interval_ms: u64,
        #[case] retry_after_secs: u64,
    ) {
        let client = CoinApiHttpClient::new(
            "test-api-key".into(),
            CoinApiHttpConfig {
                min_request_interval_ms: Some(interval_ms),
                ..config()
            },
        )
        .unwrap();

        let started = time::Instant::now();
        let (permit, _) = client.admit_rest(1).await.unwrap();
        client.record_rest_cooldown(time::Instant::now() + Duration::from_secs(retry_after_secs));
        drop(permit);

        let request = client.admit_rest(1);
        tokio::pin!(request);
        assert!(
            time::timeout(Duration::from_secs(2), request.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 6);
        let _permit = time::timeout(Duration::from_millis(1_100), request)
            .await
            .unwrap()
            .unwrap();
        assert!(started.elapsed() >= Duration::from_secs(3));
        assert_eq!(client.credits_remaining(), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn rest_pacing_rechecks_queued_attempts_before_reserving_credits() {
        let client = CoinApiHttpClient::new(
            "test-api-key".into(),
            CoinApiHttpConfig {
                max_concurrent_requests: 2,
                min_request_interval_ms: Some(1_000),
                ..config()
            },
        )
        .unwrap();

        let occupied = client.concurrency.acquire_many(2).await.unwrap();
        let first = client.admit_rest(1);
        let second = client.admit_rest(1);
        tokio::pin!(first, second);

        poll_fn(|cx| {
            assert!(first.as_mut().poll(cx).is_pending());
            assert!(second.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        drop(occupied);
        let (_first_permit, _) = first.await.unwrap();
        let started = time::Instant::now();
        assert!(
            time::timeout(Duration::from_millis(250), second.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 6);
        assert_eq!(client.concurrency.available_permits(), 1);

        let _second = second.await.unwrap();
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert_eq!(client.credits_remaining(), 5);
    }

    #[rstest]
    #[case::rate_limited(429, false)]
    #[case::unavailable(503, false)]
    #[case::gateway_timeout_with_date(504, true)]
    #[tokio::test]
    async fn retry_after_closes_rest_admission_before_body_completes(
        #[case] status: u16,
        #[case] date_header: bool,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = config();
        config.base_url = format!("http://{address}/v1/");
        config.max_concurrent_requests = 2;
        config.retry.max_retries = 0;
        let client = Arc::new(CoinApiHttpClient::new("test-api-key".into(), config).unwrap());

        let first = tokio::spawn({
            let client = Arc::clone(&client);
            async move { client.get_exchanges(&ExchangesQuery::default()).await }
        });

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;

        let (retry_after, expected_delay) = if date_header {
            let now = Timestamp::try_from(wall_clock_now()).unwrap();
            let date = now + jiff::SignedDuration::from_secs(30);
            let mut value = String::new();
            jiff::fmt::rfc2822::DateTimePrinter::new()
                .print_timestamp_rfc9110(&date, &mut value)
                .unwrap();
            (value, 25..=30)
        } else {
            ("2".into(), 2..=2)
        };

        stream
            .write_all(
                format!("HTTP/1.1 {status} Busy\r\nRetry-After: {retry_after}\r\nContent-Length: 4\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        wait_until_async(
            || std::future::ready(client.rest_not_before.lock().is_some()),
            Duration::from_secs(1),
        )
        .await;

        let query = SymbolsQuery::default();
        let second = client.get_symbols("COINBASE", &query);
        assert!(
            time::timeout(Duration::from_millis(50), second)
                .await
                .is_err()
        );
        assert!(
            time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        assert!(!first.is_finished());
        assert_eq!(client.credits_remaining(), 6);
        assert_eq!(client.concurrency.available_permits(), 1);

        stream.write_all(b"busy").await.unwrap();
        assert_eq!(request.lines().next(), Some("GET /v1/exchanges HTTP/1.1"));
        assert!(matches!(
            first.await.unwrap(),
            Err(Error::Http { status: received, retry_after_secs: Some(delay) })
                if received == status && expected_delay.contains(&delay)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn canceled_pacing_wait_does_not_reserve_credits_on_wake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let client = CoinApiHttpClient::new(
            "test-api-key".into(),
            CoinApiHttpConfig {
                base_url: format!("http://{address}/v1/"),
                min_request_interval_ms: Some(1_000),
                ..config()
            },
        )
        .unwrap();

        let (permit, _) = client.admit_rest(1).await.unwrap();
        drop(permit);

        let query = ExchangesQuery::default();
        let request = client.get_exchanges(&query);
        tokio::pin!(request);
        poll_fn(|cx| {
            assert!(request.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        client.cancel();
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(matches!(
            request.await,
            Err(Error::Retry(RetryError::Canceled))
        ));
        assert_eq!(client.credits_remaining(), 6);
    }

    #[rstest]
    #[case::extends_waiting_request(1, 3, 2)]
    #[case::preserves_later_deadline_across_timer_chunks(70, 1, 65)]
    #[tokio::test(start_paused = true)]
    async fn rest_cooldown_keeps_the_latest_deadline(
        #[case] first: u64,
        #[case] second: u64,
        #[case] blocked_secs: u64,
    ) {
        let client = CoinApiHttpClient::new("test-api-key".into(), config()).unwrap();
        let first_deadline = time::Instant::now() + Duration::from_secs(first);
        client.record_rest_cooldown(first_deadline);
        let request = client.admit_rest(1);
        tokio::pin!(request);

        if first < second {
            poll_fn(|cx| {
                assert!(request.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }

        let second_deadline = time::Instant::now() + Duration::from_secs(second);
        client.record_rest_cooldown(second_deadline);
        assert!(
            time::timeout(Duration::from_secs(blocked_secs), request.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 7);
        let _permit = request.await.unwrap();
        assert!(time::Instant::now() >= first_deadline.max(second_deadline));
        assert_eq!(client.credits_remaining(), 6);
    }

    #[rstest]
    #[case::admission_times_out(false, 7)]
    #[case::transport_uses_remaining_time(true, 6)]
    #[tokio::test(start_paused = true)]
    async fn rest_attempt_timeout_includes_admission(
        #[case] release_permit: bool,
        #[case] credits_remaining: u64,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = config();
        config.base_url = format!("http://{address}/v1/");
        config.retry.max_retries = 0;
        config.retry.operation_timeout_ms = Some(500);

        let client = CoinApiHttpClient::new("test-api-key".into(), config).unwrap();
        let permit = client.concurrency.acquire().await.unwrap();
        let query = ExchangesQuery::default();
        let request = client.get_exchanges(&query);
        tokio::pin!(request);
        assert!(
            time::timeout(Duration::from_millis(250), request.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 7);

        if release_permit {
            drop(permit);
        }

        let error = time::timeout(Duration::from_millis(300), request)
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Retry(RetryError::OperationTimeout { timeout_ms: 500 })
        ));
        assert_eq!(client.credits_remaining(), credits_remaining);
    }

    #[rstest]
    #[case::already_active(true)]
    #[case::published_while_queued(false)]
    #[tokio::test(start_paused = true)]
    async fn rest_attempt_budget_during_cooldown_with_busy_permit(#[case] already_active: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = config();
        config.base_url = format!("http://{address}/v1/");
        config.retry.max_retries = 0;
        config.retry.operation_timeout_ms = Some(500);

        let client = CoinApiHttpClient::new("test-api-key".into(), config).unwrap();
        let permit = client.concurrency.acquire().await.unwrap();
        if already_active {
            client.record_rest_cooldown(time::Instant::now() + Duration::from_secs(1));
        }

        let query = ExchangesQuery::default();
        let request = client.get_exchanges(&query);
        tokio::pin!(request);
        assert!(
            time::timeout(Duration::from_millis(300), request.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 7);

        if !already_active {
            client.record_rest_cooldown(time::Instant::now() + Duration::from_secs(1));
        }

        drop(permit);

        assert!(
            time::timeout(Duration::from_secs(1), request.as_mut())
                .await
                .is_err()
        );
        assert_eq!(client.credits_remaining(), 6);
        let error = time::timeout(Duration::from_millis(250), request)
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Retry(RetryError::OperationTimeout { timeout_ms: 500 })
        ));
    }

    #[rstest]
    #[case::refunded(6)]
    #[case::spent(0)]
    #[tokio::test(start_paused = true)]
    async fn rest_cooldown_waits_for_credit_settlement(#[case] refund: u64) {
        let client = CoinApiHttpClient::new(
            "test-api-key".into(),
            CoinApiHttpConfig {
                max_concurrent_requests: 2,
                ..config()
            },
        )
        .unwrap();

        let (reservation, _) = client.admit_rest(7).await.unwrap();
        let _download = client.concurrency.acquire().await.unwrap();
        let deadline = time::Instant::now() + Duration::from_secs(1);
        client.record_rest_cooldown(deadline);
        let request = client.admit_rest(1);
        tokio::pin!(request);
        poll_fn(|cx| {
            assert!(request.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        assert_eq!(client.credits_remaining(), 0);

        client
            .credits_remaining
            .fetch_add(refund, Ordering::Relaxed);
        drop(reservation);

        let result = time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap();

        if refund == 0 {
            assert!(matches!(result, Err(Error::CreditBudget)));
            assert!(time::Instant::now() < deadline);
            assert_eq!(client.credits_remaining(), 0);
        } else {
            let _permit = result.unwrap();
            assert!(time::Instant::now() >= deadline);
            assert_eq!(client.credits_remaining(), 5);
        }
    }

    fn config() -> CoinApiHttpConfig {
        CoinApiHttpConfig {
            credit_budget: 7,
            retry: RetryConfig {
                max_retries: 1,
                initial_delay_ms: 1,
                max_delay_ms: 1,
                jitter_ms: 0,
                operation_timeout_ms: Some(5000),
                max_elapsed_ms: Some(10_000),
                ..RetryConfig::default()
            },
            ..CoinApiHttpConfig::default()
        }
    }

    async fn read_request(socket: &mut TcpStream) -> String {
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
        }

        String::from_utf8(request).unwrap()
    }
}

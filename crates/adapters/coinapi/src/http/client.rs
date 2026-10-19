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

//! CoinAPI REST transport with bounded retries and local admission limits.

use std::{
    collections::HashMap,
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use jiff::Timestamp;
use nautilus_core::{UnixNanos, string::secret::SecretString};
use nautilus_model::{
    identifiers::InstrumentId, instruments::Instrument, types::fixed::check_fixed_precision,
};
use nautilus_network::{
    backoff::ExponentialBackoff,
    http::{HttpClient, HttpRedirectPolicy, HttpResponse, Method},
    retry::{RetryConfig, RetryManager},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    error::Error,
    models::{Book, Exchange, Ohlcv, Quote, Symbol, Trade},
    parse::{CoinApiInstrument, parse_instrument, parse_instruments},
    query::{
        BookQuery, ExchangesQuery, HISTORY_LIMIT_MAX, HistoryQuery, HistoryRange, OhlcvQuery,
        SymbolsQuery, TradesQuery, provider_time,
    },
};
use crate::common::{consts::COINAPI_NAUTILUS_REFERRER, credential::is_valid_api_key};

/// Primary CoinAPI Market Data REST endpoint, including its version prefix.
pub const REST_URL: &str = "https://rest.coinapi.io/v1/";

/// Transport configuration for the raw REST client.
///
/// # Credit Allowance
///
/// `credit_budget` is shared by all requests on this instance:
///
/// - Each attempt reserves ceil(requested limit / 100) credits, or one credit
///   when the request has no record limit.
/// - A successful historical page keeps ceil(returned records / 100) credits,
///   with a minimum of one, and refunds the rest of its reservation.
/// - Failed or canceled attempts keep their full reservation.
///
/// This is a local safety limit, not the provider's billing ledger, and it does
/// not coordinate with other clients.
#[derive(Debug, Clone)]
pub struct CoinApiHttpConfig {
    /// Base URL including the version prefix; accepts local simulated peers.
    pub base_url: String,
    /// Optional explicit proxy URL; ambient system proxies remain disabled.
    pub proxy_url: Option<SecretString>,
    /// Maximum simultaneous requests for this client instance.
    pub max_concurrent_requests: usize,
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

/// Authenticated, data-only CoinAPI REST client.
///
/// Requests return raw protocol models. The single-page historical methods fetch exactly one
/// page; callers must not infer completeness from a full page. The range methods page through a
/// whole window.
#[derive(Debug)]
pub struct CoinApiHttpClient {
    client: HttpClient,
    base_url: Url,
    api_key: SecretString,
    concurrency: tokio::sync::Semaphore,
    // Serializes range requests so each sizes its pages after earlier ranges settle their credits
    ranges: tokio::sync::Mutex<()>,
    credits_remaining: AtomicU64,
    retry: RetryManager<Error>,
    cancellation: CancellationToken,
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

        ExponentialBackoff::new(
            Duration::from_millis(config.retry.initial_delay_ms),
            Duration::from_millis(config.retry.max_delay_ms),
            config.retry.backoff_factor,
            config.retry.jitter_ms,
            config.retry.immediate_first,
        )
        .map_err(|e| Error::Validation(e.to_string()))?;

        let client = HttpClient::builder()
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
            .build()?;
        Ok(Self {
            client,
            base_url,
            api_key,
            concurrency: tokio::sync::Semaphore::new(config.max_concurrent_requests),
            ranges: tokio::sync::Mutex::new(()),
            credits_remaining: AtomicU64::new(config.credit_budget),
            retry: RetryManager::new(config.retry),
            cancellation: CancellationToken::new(),
        })
    }

    /// Cancels pending and future requests on this instance.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Returns the remaining conservative local credit allowance.
    #[must_use]
    pub fn credits_remaining(&self) -> u64 {
        self.credits_remaining.load(Ordering::Relaxed)
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
        for (instrument_id, precision) in price_precisions {
            check_fixed_precision(*precision).map_err(|e| {
                Error::Validation(format!("price precision for {instrument_id}: {e}"))
            })?;
        }

        let symbols = self.get_symbols(exchange_id, query).await?;
        let (mut instruments, errors) = parse_instruments(&symbols, ts_init);

        for e in errors {
            log::warn!("Skipping CoinAPI instrument: {e}");
        }

        for loaded in &mut instruments {
            let Some(&precision) = price_precisions.get(&loaded.instrument.id()) else {
                continue;
            };

            let Some(symbol) = symbols.iter().find(|s| s.symbol_id == loaded.symbol_id) else {
                continue;
            };

            let symbol = Symbol {
                price_precision: Some(Decimal::new(1, u32::from(precision))),
                ..symbol.clone()
            };

            match parse_instrument(&symbol, ts_init) {
                Ok(overridden) => *loaded = overridden,
                Err(e) => log::warn!("Keeping CoinAPI price resolution: {e}"),
            }
        }

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
        if query.period_id.trim().is_empty() {
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
        period_id: &str,
        range: &HistoryRange,
    ) -> Result<HistoryPages<Ohlcv>, Error> {
        self.get_range(
            range,
            |ohlcv: &Ohlcv| ohlcv.time_period_start,
            |page| async move {
                let query = OhlcvQuery {
                    period_id: period_id.to_owned(),
                    range: page,
                };

                self.get_ohlcv(symbol_id, &query).await
            },
        )
        .await
    }

    // A full page can end partway through the records that share its last timestamp, so that
    // group is dropped and the next page starts at its timestamp. CoinAPI returns the window
    // [time_start, time_end), including every record at time_start.
    async fn get_range<T, F, Fut>(
        &self,
        range: &HistoryRange,
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

            // CoinAPI can substitute nearby OHLCV periods for an empty window
            page.retain(|record| (cursor..range.time_end).contains(&key(record)));

            // A full page emptied by filtering held only fallback periods outside the window
            if !is_full || page.is_empty() || remaining == Some(page_limit) {
                records.extend(page);
                return Ok(HistoryPages {
                    records,
                    is_truncated: false,
                });
            }

            let last = key(&page[page.len() - 1]);
            let boundary = page.partition_point(|record| key(record) < last);

            if boundary == 0 {
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
        if path
            .iter()
            .any(|part| part.trim().is_empty() || matches!(*part, "." | ".."))
        {
            return Err(Error::Validation(
                "path identifiers must not be empty or dot segments".into(),
            ));
        }

        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| Error::Validation("base URL cannot contain path segments".into()))?
            .pop_if_empty()
            .extend(path);
        self.retry
            .invocation(
                "CoinAPI GET",
                || async {
                    let _permit = self
                        .concurrency
                        .acquire()
                        .await
                        .map_err(|_| Error::Validation("request admission closed".into()))?;
                    self.credits_remaining
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                            remaining.checked_sub(cost)
                        })
                        .map_err(|_| Error::CreditBudget)?;

                    let headers = HashMap::from([
                        (
                            "X-CoinAPI-Key".into(),
                            self.api_key.expose_secret().to_owned(),
                        ),
                        ("Accept".into(), "application/json".into()),
                        ("X-Referrer".into(), COINAPI_NAUTILUS_REFERRER.into()),
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
                    let value = self.decode(&response)?;
                    self.credits_remaining
                        .fetch_add(cost.saturating_sub(billed(&value)), Ordering::Relaxed);
                    Ok(value)
                },
                Error::is_retryable,
                Error::Retry,
            )
            .retry_delay(&Error::retry_delay)
            .cancellation_token(&self.cancellation)
            .execute()
            .await
    }

    fn decode<T: DeserializeOwned>(&self, response: &HttpResponse) -> Result<T, Error> {
        for (key, value) in &response.headers {
            if key.starts_with("x-credits-") || key == "x-ratelimit-request-cost" {
                log::debug!("CoinAPI usage {key}={value}");
            }
        }

        if !response.status.is_success() {
            let status = response.status.as_u16();
            let retry_after_secs = response
                .headers
                .get("retry-after")
                .and_then(|value| value.parse().ok());
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

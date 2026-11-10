// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Binance Portfolio Margin (PAPI) HTTP client providing the signed request boundary.
//!
//! This client owns routing, signing, admission, retry, and error decoding for the PAPI service;
//! endpoint-specific request and response models sit at the boundaries that consume it.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Weak},
    time::Duration,
};

use ahash::AHashMap;
use jiff::Timestamp;
use nautilus_core::datetime::SECONDS_IN_DAY;
use nautilus_network::{
    http::{
        HttpClient, HttpRedirectPolicy, HttpResponse, Method, create_standard_nautilus_headers,
    },
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
    retry::{RetryError, RetryManager},
};
use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::error::{BinancePapiHttpError, BinancePapiHttpResult};
use crate::common::{
    consts::{
        BINANCE_API_KEY_HEADER, BINANCE_PAPI_PATH, BINANCE_PAPI_RATE_LIMITS,
        BINANCE_RETRY_AFTER_HEADER, BinanceRateLimitQuota,
    },
    credential::SigningCredential,
    enums::{BinanceEnvironment, BinanceRateLimitInterval, BinanceRateLimitType},
    http::{BinanceHttpBackoff, BinanceHttpEndpointScope, sha256},
    http_weights::{papi_route, request_weight_papi},
    models::BinanceErrorResponse,
    urls::get_papi_base_url,
};

const BINANCE_GLOBAL_RATE_KEY: &str = "binance:global";
const BINANCE_ORDERS_RATE_KEY: &str = "binance:orders";

type BinancePapiLimiter = RateLimiter<Ustr, MonotonicClock>;
type BinancePapiRateLimiter = Arc<BinancePapiLimiter>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct BinancePapiRequestScope {
    endpoint: BinanceHttpEndpointScope,
    proxy_url_digest: Option<[u8; 32]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BinancePapiAccountScope([u8; 32]);

#[derive(Clone, PartialEq, Eq, Hash)]
struct BinancePapiOrderScope {
    endpoint: BinanceHttpEndpointScope,
    account: BinancePapiAccountScope,
}

static BINANCE_PAPI_REQUEST_LIMITERS: LazyLock<
    Mutex<AHashMap<BinancePapiRequestScope, Weak<BinancePapiLimiter>>>,
> = LazyLock::new(|| Mutex::new(AHashMap::new()));

static BINANCE_PAPI_ORDER_LIMITERS: LazyLock<
    Mutex<AHashMap<BinancePapiOrderScope, Weak<BinancePapiLimiter>>>,
> = LazyLock::new(|| Mutex::new(AHashMap::new()));

struct PapiRateLimitConfig {
    request_quota: Quota,
    order_quotas: Vec<(String, Quota)>,
    order_keys: Vec<String>,
}

/// Raw HTTP client for the Binance Portfolio Margin REST API.
#[derive(Clone)]
pub struct BinanceRawPapiHttpClient {
    retry_manager: Arc<RetryManager<BinancePapiHttpError>>,
    client: HttpClient,
    base_url: String,
    credential: Option<SigningCredential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
    backoff: BinanceHttpBackoff,
}

impl Debug for BinanceRawPapiHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The nested HTTP client carries the API key header and is elided; the signing
        // credential redacts itself; the base URL prints its origin only.
        f.debug_struct(stringify!(BinanceRawPapiHttpClient))
            .field("base_url", &nautilus_network::url_origin(&self.base_url))
            .field("credential", &self.credential)
            .field("recv_window", &self.recv_window)
            .finish_non_exhaustive()
    }
}

impl BinanceRawPapiHttpClient {
    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub fn http_client(&self) -> &HttpClient {
        &self.client
    }

    /// Returns whether this client has complete signing credentials.
    #[must_use]
    pub const fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Creates a new raw Binance Portfolio Margin HTTP client.
    ///
    /// Binance documents only a production PAPI host and HMAC-SHA256 request signing, so
    /// testnet and demo environments and Ed25519 credentials are rejected before any connection
    /// is attempted.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment has no PAPI host, the credentials use an unsupported
    /// signature scheme, credentials are incomplete, or the HTTP client fails to build.
    pub fn new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinancePapiHttpResult<Self> {
        let Some(base_url) = get_papi_base_url(environment) else {
            return Err(BinancePapiHttpError::ValidationError(format!(
                "Binance PAPI has no {environment:?} environment host"
            )));
        };

        let base_url = match base_url_override {
            Some(url) => url.trim_end_matches('/').to_string(),
            None => base_url.to_string(),
        };

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(SigningCredential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinancePapiHttpError::MissingCredentials),
        };

        if let Some(credential) = &credential
            && credential.is_ed25519()
        {
            return Err(BinancePapiHttpError::UnsupportedSignatureScheme(
                "Ed25519".to_string(),
            ));
        }

        let PapiRateLimitConfig {
            request_quota,
            order_quotas,
            order_keys,
        } = Self::rate_limit_config();

        let backoff = BinanceHttpBackoff::new_papi(
            environment,
            &base_url,
            proxy_url.as_deref(),
            credential.as_ref(),
        );

        let account_scope = credential.as_ref().map(Self::account_scope);
        let rate_limiters = Self::shared_rate_limiters(
            environment,
            &base_url,
            proxy_url.as_deref(),
            account_scope,
            request_quota,
            order_quotas,
        );

        let headers = Self::default_headers(&credential);

        let client = HttpClient::builder()
            .redirect_policy(HttpRedirectPolicy::Reject)
            .headers(headers)
            .header_keys(vec![
                BINANCE_API_KEY_HEADER.to_string(),
                BINANCE_RETRY_AFTER_HEADER.to_string(),
            ])
            .maybe_timeout_secs(timeout_secs)
            .maybe_proxy_url(proxy_url)
            .rate_limiters(rate_limiters)
            .build()?;

        Ok(Self {
            retry_manager: Arc::new(RetryManager::new(crate::common::http::retry_config())),
            client,
            base_url,
            credential,
            recv_window,
            order_rate_keys: order_keys,
            backoff,
        })
    }

    fn shared_rate_limiters(
        environment: BinanceEnvironment,
        base_url: &str,
        proxy_url: Option<&str>,
        account_scope: Option<BinancePapiAccountScope>,
        request_quota: Quota,
        order_quotas: Vec<(String, Quota)>,
    ) -> Vec<BinancePapiRateLimiter> {
        let endpoint = BinanceHttpEndpointScope::Custom {
            environment,
            base_url: base_url.to_string(),
        };

        let request_scope = BinancePapiRequestScope {
            endpoint: endpoint.clone(),
            proxy_url_digest: proxy_url.map(sha256),
        };

        let request_limiter = Self::request_rate_limiter(request_scope, request_quota);
        let mut limiters = vec![request_limiter];

        if let Some(account) = account_scope {
            let order_scope = BinancePapiOrderScope { endpoint, account };
            limiters.push(Self::order_rate_limiter(order_scope, order_quotas));
        }

        limiters
    }

    fn account_scope(credential: &SigningCredential) -> BinancePapiAccountScope {
        BinancePapiAccountScope(sha256(credential.api_key()))
    }

    fn request_rate_limiter(
        scope: BinancePapiRequestScope,
        quota: Quota,
    ) -> BinancePapiRateLimiter {
        let mut registry = BINANCE_PAPI_REQUEST_LIMITERS.lock();

        if let Some(limiter) = registry.get(&scope).and_then(Weak::upgrade) {
            return limiter;
        }

        let limiter = Arc::new(RateLimiter::new_with_quota(
            None,
            vec![(Ustr::from(BINANCE_GLOBAL_RATE_KEY), quota)],
        ));
        registry.insert(scope, Arc::downgrade(&limiter));
        limiter
    }

    fn order_rate_limiter(
        scope: BinancePapiOrderScope,
        quotas: Vec<(String, Quota)>,
    ) -> BinancePapiRateLimiter {
        let mut registry = BINANCE_PAPI_ORDER_LIMITERS.lock();

        if let Some(limiter) = registry.get(&scope).and_then(Weak::upgrade) {
            return limiter;
        }

        let quotas = quotas
            .into_iter()
            .map(|(key, quota)| (Ustr::from(&key), quota))
            .collect();
        let limiter = Arc::new(RateLimiter::new_with_quota(None, quotas));
        registry.insert(scope, Arc::downgrade(&limiter));
        limiter
    }

    /// Performs a GET request and deserializes the response body.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
    pub async fn get<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::GET, path, params, signed, use_order_quota)
            .await
    }

    /// Performs a POST request, signing the query when `signed` is true.
    ///
    /// The request is sent exactly once: a timeout or ambiguous outcome must be resolved by the
    /// caller's reconciliation rather than a local resubmission.
    ///
    /// Every documented PAPI route carries its parameters in the query string; a request body is
    /// not part of the PAPI contract, and the signature covers the query only.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
    pub async fn post<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request_once(Method::POST, path, params, signed, use_order_quota, None)
            .await
    }

    /// Performs a PUT request, signing the query when `signed` is true.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
    pub async fn request_put<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::PUT, path, params, signed, use_order_quota)
            .await
    }

    /// Performs a DELETE request, signing the query when `signed` is true.
    ///
    /// The request is sent exactly once: a timeout or ambiguous outcome must be resolved by the
    /// caller's reconciliation rather than a local resubmission.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
    pub async fn request_delete<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::DELETE, path, params, signed, use_order_quota)
            .await
    }

    async fn request<P, T>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let operation =
            || self.request_once(method.clone(), path, params, signed, use_order_quota, None);

        if method != Method::GET {
            return operation().await;
        }

        self.retry_manager
            .invocation(
                path,
                operation,
                |e| e.is_retryable(),
                |e| match e {
                    RetryError::Canceled => {
                        BinancePapiHttpError::Canceled("HTTP requests canceled".to_string())
                    }
                    RetryError::OperationTimeout { timeout_ms } => {
                        BinancePapiHttpError::Timeout(format!("Request exceeded {timeout_ms}ms"))
                    }
                    RetryError::InvalidConfiguration { message } => {
                        BinancePapiHttpError::ValidationError(message)
                    }
                    e @ RetryError::ElapsedBudgetExceeded { .. } => {
                        BinancePapiHttpError::RetryBudgetExceeded(e.to_string())
                    }
                },
            )
            .retry_delay(&BinancePapiHttpError::retry_after)
            .execute()
            .await
    }

    async fn request_once<P, T>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
        body: Option<Vec<u8>>,
    ) -> BinancePapiHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinancePapiHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = HashMap::new();

        let credential = if signed {
            Some(
                self.credential
                    .as_ref()
                    .ok_or(BinancePapiHttpError::MissingCredentials)?,
            )
        } else {
            None
        };

        let keys = self.rate_limit_keys(&method, path, &query, use_order_quota);
        self.backoff
            .admit(&self.client, keys, use_order_quota, method == Method::GET)
            .await
            .map_err(|()| BinancePapiHttpError::RateLimitBlocked)?;

        if let Some(cred) = credential {
            if !query.is_empty() {
                query.push('&');
            }

            let timestamp = Timestamp::now().as_millisecond();
            query.push_str(&format!("timestamp={timestamp}"));
            if let Some(recv_window) = self.recv_window {
                query.push_str(&format!("&recvWindow={recv_window}"));
            }

            let signature = Self::percent_encode(&cred.sign(&query));
            query.push_str(&format!("&signature={signature}"));
            headers.insert(
                BINANCE_API_KEY_HEADER.to_string(),
                cred.api_key().to_string(),
            );
        }

        let url = self.build_url(path, &query);

        let response = self
            .client
            .request(
                method,
                url,
                None::<&HashMap<String, Vec<String>>>,
                Some(headers),
                body,
                None,
                None,
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(&response);
        }

        serde_json::from_slice::<T>(&response.body)
            .map_err(|e| BinancePapiHttpError::JsonError(e.to_string()))
    }

    fn build_url(&self, path: &str, query: &str) -> String {
        let url_path = if path.starts_with("/papi/") {
            path.to_string()
        } else if path.starts_with('/') {
            format!("{BINANCE_PAPI_PATH}{path}")
        } else {
            format!("{BINANCE_PAPI_PATH}/{path}")
        };

        let mut url = format!("{}{}", self.base_url, url_path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }

        url
    }

    fn rate_limit_keys(
        &self,
        method: &Method,
        path: &str,
        query: &str,
        use_orders: bool,
    ) -> Vec<Ustr> {
        let weight = request_weight_papi(method, path, query);
        let mut keys = vec![Ustr::from(BINANCE_GLOBAL_RATE_KEY); weight as usize];

        if use_orders {
            // The venue counts one OCO placement as two orders against the order rate limit.
            // <https://developers.binance.com/docs/catalog/advanced-trading-derivatives-trading-portfolio-margin/api/rest-api/trade#margin-account-new-oco>
            let order_cost = if *method == Method::POST && papi_route(path) == "margin/order/oco" {
                2
            } else {
                1
            };

            keys.extend(
                self.order_rate_keys
                    .iter()
                    .flat_map(|key| std::iter::repeat_n(Ustr::from(key), order_cost)),
            );
        }

        keys
    }

    fn parse_error_response<T>(&self, response: &HttpResponse) -> BinancePapiHttpResult<T> {
        let status = response.status.as_u16();
        let body = String::from_utf8_lossy(&response.body).to_string();
        let retry_after = crate::common::http::retry_after(&response.headers, Timestamp::now());
        if let Ok(err) = serde_json::from_str::<BinanceErrorResponse>(&body) {
            self.backoff.observe(status, Some(err.code), retry_after);
            return Err(BinancePapiHttpError::BinanceError {
                code: err.code,
                message: err.msg,
                status,
                retry_after,
            });
        }

        self.backoff.observe(status, None, retry_after);
        Err(BinancePapiHttpError::UnexpectedStatus {
            status,
            body,
            retry_after,
        })
    }

    fn rate_limit_config() -> PapiRateLimitConfig {
        let mut order_quotas = Vec::new();
        let mut order_keys = Vec::new();
        let mut request_quota = None;

        for quota in BINANCE_PAPI_RATE_LIMITS {
            let Some(q) = Self::quota_from(quota) else {
                continue;
            };

            match quota.rate_limit_type {
                BinanceRateLimitType::RequestWeight if request_quota.is_none() => {
                    request_quota = Some(q);
                }
                BinanceRateLimitType::Orders => {
                    let key = format!(
                        "{}:{}:{:?}",
                        BINANCE_ORDERS_RATE_KEY, quota.interval_num, quota.interval
                    );
                    order_keys.push(key.clone());
                    order_quotas.push((key, q));
                }
                _ => {}
            }
        }

        let request_quota = request_quota.unwrap_or_else(|| {
            Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid constant")
        });

        PapiRateLimitConfig {
            request_quota,
            order_quotas,
            order_keys,
        }
    }

    fn quota_from(quota: &BinanceRateLimitQuota) -> Option<Quota> {
        let burst = NonZeroU32::new(quota.limit)?;
        let period = Self::quota_period(quota)?;
        let replenish_interval_ns = period.as_nanos() / u128::from(quota.limit);
        let replenish_interval_ns = u64::try_from(replenish_interval_ns).ok()?;

        Quota::with_period(Duration::from_nanos(replenish_interval_ns))
            .map(|q| q.allow_burst(burst))
    }

    fn quota_period(quota: &BinanceRateLimitQuota) -> Option<Duration> {
        match quota.interval {
            BinanceRateLimitInterval::Second => {
                Some(Duration::from_secs(u64::from(quota.interval_num)))
            }
            BinanceRateLimitInterval::Minute => {
                Some(Duration::from_secs(60 * u64::from(quota.interval_num)))
            }
            BinanceRateLimitInterval::Day => Some(Duration::from_secs(
                SECONDS_IN_DAY * u64::from(quota.interval_num),
            )),
            BinanceRateLimitInterval::Unknown => None,
        }
    }

    fn default_headers(credential: &Option<SigningCredential>) -> HashMap<String, String> {
        let mut headers: HashMap<String, String> =
            create_standard_nautilus_headers().into_iter().collect();

        if let Some(cred) = credential {
            headers.insert(
                BINANCE_API_KEY_HEADER.to_string(),
                cred.api_key().to_string(),
            );
        }

        headers
    }

    /// Percent-encodes a string for use in URL query parameters.
    fn percent_encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push('%');
                    result.push_str(&format!("{byte:02X}"));
                }
            }
        }

        result
    }
}
#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use axum::{
        Router,
        http::{HeaderMap, Method as AxumMethod},
        response::IntoResponse,
        routing::any,
    };
    use base64::Engine;
    use nautilus_network::ratelimiter::quota::Quota;
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::common::enums::BinanceEnvironment;

    /// PKCS#8 DER wrapping of RFC 8032 test vector 1 Ed25519 private key, matching the vector in
    /// `common::credential` tests.
    const ED25519_PKCS8_TEST_VECTOR: [u8; 48] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20, 0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
        0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c,
        0xae, 0x7f, 0x60,
    ];

    #[derive(Clone, Debug, Default)]
    struct CapturedRequest {
        method: String,
        path: String,
        query: String,
        api_key_header: Option<String>,
    }

    fn hmac_signature(query: &str) -> String {
        let credential =
            SigningCredential::new("papi-test-key".to_string(), "papi-test-secret".to_string());
        credential.sign(query)
    }

    async fn new_client(base_url: &str, recv_window: Option<u64>) -> BinanceRawPapiHttpClient {
        BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("papi-test-key".to_string()),
            Some("papi-test-secret".to_string()),
            Some(base_url.to_string()),
            recv_window,
            None,
            None,
        )
        .unwrap()
    }

    #[rstest]
    #[case(BinanceEnvironment::Testnet, "Testnet")]
    #[case(BinanceEnvironment::Demo, "Demo")]
    fn test_new_rejects_environments_without_papi_host(
        #[case] environment: BinanceEnvironment,
        #[case] name: &str,
        #[values(None, Some("http://127.0.0.1:9"))] base_url_override: Option<&str>,
    ) {
        let result = BinanceRawPapiHttpClient::new(
            environment,
            Some("key".to_string()),
            Some("secret".to_string()),
            base_url_override.map(String::from),
            None,
            None,
            None,
        );

        match result {
            Err(BinancePapiHttpError::ValidationError(message)) => {
                assert!(message.contains(&format!("no {name}")), "was {message}");
            }
            other => panic!("expected ValidationError, was {other:?}"),
        }
    }

    #[rstest]
    fn test_new_rejects_ed25519_credentials() {
        let key_b64 = base64::engine::general_purpose::STANDARD.encode(ED25519_PKCS8_TEST_VECTOR);

        let result = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            Some(key_b64),
            None,
            None,
            None,
            None,
        );

        match result {
            Err(BinancePapiHttpError::UnsupportedSignatureScheme(scheme)) => {
                assert_eq!(scheme, "Ed25519");
            }
            other => panic!("expected UnsupportedSignatureScheme, was {other:?}"),
        }
    }

    #[rstest]
    fn test_new_rejects_incomplete_credentials() {
        let result = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            None,
            None,
            None,
            None,
            None,
        );

        assert!(matches!(
            result,
            Err(BinancePapiHttpError::MissingCredentials)
        ));
    }

    #[tokio::test]
    async fn test_signed_get_captures_exact_request() {
        let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = Router::new().fallback(any(
            move |method: AxumMethod, uri: axum::http::Uri, headers: HeaderMap| {
                let captured = captured.clone();
                async move {
                    let (path, query) = match uri.path_and_query() {
                        Some(pq) => match pq.query() {
                            Some(q) => (pq.path().to_string(), q.to_string()),
                            None => (pq.path().to_string(), String::new()),
                        },
                        None => (uri.path().to_string(), String::new()),
                    };

                    captured.lock().unwrap().push(CapturedRequest {
                        method: method.to_string(),
                        path,
                        query,
                        api_key_header: headers
                            .get("X-MBX-APIKEY")
                            .map(|value| value.to_str().unwrap().to_string()),
                    });

                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            },
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, Some(5_000)).await;
        let params = json!({"asset": "BTC"});
        let result: Result<serde_json::Value, _> = client
            .get("/papi/v1/balance", Some(&params), true, false)
            .await;

        server.abort();

        result.unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/papi/v1/balance");
        assert_eq!(request.api_key_header.as_deref(), Some("papi-test-key"));

        // Signed query: params, timestamp, recvWindow, signature; the signature must sign
        // everything before it.
        let parts: Vec<&str> = request.query.split('&').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "asset=BTC");
        assert!(parts[1].starts_with("timestamp="));
        assert_eq!(parts[2], "recvWindow=5000");
        assert!(parts[3].starts_with("signature="));

        let signed = request
            .query
            .split("&signature=")
            .next()
            .unwrap()
            .to_string();
        assert_eq!(
            parts[3].trim_start_matches("signature="),
            hmac_signature(&signed)
        );
    }

    #[tokio::test]
    async fn test_signed_request_without_params_omits_leading_separator() {
        let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = Router::new().fallback(any(
            move |method: AxumMethod, uri: axum::http::Uri, headers: HeaderMap| {
                let captured = captured.clone();
                async move {
                    let (path, query) = match uri.path_and_query() {
                        Some(pq) => match pq.query() {
                            Some(q) => (pq.path().to_string(), q.to_string()),
                            None => (pq.path().to_string(), String::new()),
                        },
                        None => (uri.path().to_string(), String::new()),
                    };

                    captured.lock().unwrap().push(CapturedRequest {
                        method: method.to_string(),
                        path,
                        query,
                        api_key_header: headers
                            .get("X-MBX-APIKEY")
                            .map(|value| value.to_str().unwrap().to_string()),
                    });

                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            },
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        result.unwrap();
        let requests = requests.lock().unwrap();
        let query = &requests[0].query;

        // Without caller params the signed query starts directly at `timestamp`, with no
        // leading separator that would invalidate the signature.
        let parts: Vec<&str> = query.split('&').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].starts_with("timestamp="));
        assert!(parts[1].starts_with("signature="));
    }

    #[tokio::test]
    async fn test_redirect_is_rejected_without_following() {
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app = Router::new().fallback(any(move |method: AxumMethod| {
            let captured = captured.clone();
            async move {
                captured.fetch_add(1, Ordering::Relaxed);

                if method.as_str() == "GET" {
                    let mut response = axum::http::StatusCode::FOUND.into_response();
                    response.headers_mut().insert(
                        "Location",
                        axum::http::HeaderValue::from_static("/papi/v1/elsewhere"),
                    );
                    response
                } else {
                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        // A redirect carrying the signed query must not be followed.
        assert!(result.is_err());
        assert_eq!(requests.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_signed_request_without_credentials_fails_before_dispatch() {
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app = Router::new().fallback(any(move || {
            let captured = captured.clone();
            async move {
                captured.fetch_add(1, Ordering::Relaxed);
                (axum::http::StatusCode::OK, "{}").into_response()
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            None,
            None,
            Some(url),
            None,
            None,
            None,
        )
        .unwrap();
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        match result {
            Err(BinancePapiHttpError::MissingCredentials) => {}
            other => panic!("expected MissingCredentials, was {other:?}"),
        }

        assert_eq!(requests.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_relative_path_routes_under_papi_prefix() {
        let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = Router::new().fallback(any(move |method: AxumMethod, uri: axum::http::Uri| {
            let captured = captured.clone();
            async move {
                captured.lock().unwrap().push(CapturedRequest {
                    method: method.to_string(),
                    path: uri.path().to_string(),
                    query: String::new(),
                    api_key_header: None,
                });

                (axum::http::StatusCode::OK, "{}").into_response()
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>("um/order", None::<&serde_json::Value>, false, false)
            .await;

        server.abort();

        result.unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].path, "/papi/v1/um/order");
    }

    #[rstest]
    #[case::permissions(
        -2015,
        400,
        "Invalid API-key, IP, or permissions for the action.",
        Some("17")
    )]
    #[case::clock_window(
        -1021,
        400,
        "Timestamp for this request is outside of the recvWindow.",
        None
    )]
    #[case::status_unknown(
        -1007,
        500,
        "Timeout waiting for response from backend server.",
        None
    )]
    #[tokio::test]
    async fn test_venue_error_decodes_code_and_message(
        #[case] code: i64,
        #[case] status: u16,
        #[case] message: &str,
        #[case] retry_after_header: Option<&str>,
    ) {
        let body = json!({"code": code, "msg": message}).to_string();
        let retry_after_header = retry_after_header.map(String::from);
        let expected_retry_after = retry_after_header
            .as_deref()
            .map(|secs| Duration::from_secs(secs.parse().unwrap()));

        let app = Router::new().fallback(any(move || {
            let body = body.clone();
            let retry_after_header = retry_after_header.clone();
            async move {
                let mut response = (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    body.clone(),
                )
                    .into_response();

                if let Some(value) = &retry_after_header {
                    response.headers_mut().insert(
                        "Retry-After",
                        axum::http::HeaderValue::from_str(value).unwrap(),
                    );
                }

                response
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        match result {
            Err(BinancePapiHttpError::BinanceError {
                code: venue_code,
                message: venue_message,
                status: venue_status,
                retry_after,
            }) => {
                assert_eq!(venue_code, code);
                assert_eq!(venue_status, status);
                assert_eq!(venue_message, message);
                assert_eq!(retry_after, expected_retry_after);

                // Clock/window and permission failures are permanent venue rejections;
                // only the backend timeout is retryable.
                let error = BinancePapiHttpError::BinanceError {
                    code: venue_code,
                    message: venue_message,
                    status: venue_status,
                    retry_after,
                };

                assert_eq!(error.is_retryable(), status >= 500);
            }
            other => panic!("expected BinanceError, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_timeout_surfaces_unknown_outcome_error() {
        let app = Router::new().fallback(any(|| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            (axum::http::StatusCode::OK, "{}").into_response()
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("papi-test-key".to_string()),
            Some("papi-test-secret".to_string()),
            Some(url),
            None,
            Some(1),
            None,
        )
        .unwrap();
        let result: Result<serde_json::Value, _> = client
            .post::<serde_json::Value, _>(
                "/papi/v1/um/order",
                None::<&serde_json::Value>,
                true,
                true,
            )
            .await;

        server.abort();

        // A timed-out write leaves the outcome unknown: the error must surface rather than
        // retry, and the caller reconciles the order state.
        match result {
            Err(BinancePapiHttpError::Timeout(message)) => {
                assert_eq!(message, "request deadline elapsed");
            }
            other => panic!("expected Timeout, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_unexpected_status_without_venue_payload() {
        let app = Router::new().fallback(any(|| async {
            (axum::http::StatusCode::BAD_GATEWAY, "upstream error").into_response()
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        match result {
            Err(BinancePapiHttpError::UnexpectedStatus { status, body, .. }) => {
                assert_eq!(status, 502);
                assert_eq!(body, "upstream error");
            }
            other => panic!("expected UnexpectedStatus, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_get_retries_after_venue_throttle() {
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app = Router::new().fallback(any(move || {
            let captured = captured.clone();
            async move {
                if captured.fetch_add(1, Ordering::Relaxed) == 0 {
                    let mut response = (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        r#"{"code":-1003,"msg":"Requests rate too high."}"#,
                    )
                        .into_response();
                    response
                        .headers_mut()
                        .insert("Retry-After", axum::http::HeaderValue::from_static("0"));
                    response
                } else {
                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let result: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        result.unwrap();
        assert_eq!(requests.load(Ordering::Relaxed), 2);
    }

    #[rstest]
    #[case::post(Method::POST)]
    #[case::put(Method::PUT)]
    #[case::delete(Method::DELETE)]
    #[tokio::test]
    async fn test_write_is_sent_exactly_once_on_throttle(#[case] method: Method) {
        let requests = Arc::new(AtomicUsize::new(0));
        let bodies: Arc<Mutex<Vec<Option<Vec<u8>>>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let captured_bodies = bodies.clone();

        let app = Router::new().fallback(any(move |body: axum::body::Bytes| {
            let captured = captured.clone();
            let captured_bodies = captured_bodies.clone();
            async move {
                captured_bodies
                    .lock()
                    .unwrap()
                    .push((!body.is_empty()).then(|| body.to_vec()));

                if captured.fetch_add(1, Ordering::Relaxed) == 0 {
                    let mut response = (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        r#"{"code":-1003,"msg":"Requests rate too high."}"#,
                    )
                        .into_response();
                    response
                        .headers_mut()
                        .insert("Retry-After", axum::http::HeaderValue::from_static("0"));
                    response
                } else {
                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let params =
            json!({"symbol": "BTCUSDT", "side": "BUY", "type": "MARKET", "quantity": "0.01"});

        let result: Result<serde_json::Value, _> = match method {
            Method::POST => {
                client
                    .post("/papi/v1/um/order", Some(&params), true, true)
                    .await
            }
            Method::PUT => {
                client
                    .request_put("/papi/v1/um/order", Some(&params), true, true)
                    .await
            }
            _ => {
                client
                    .request_delete("/papi/v1/um/order", Some(&params), true, true)
                    .await
            }
        };

        server.abort();

        // A throttled mutating request surfaces the venue error rather than resubmitting;
        // the ambiguous outcome belongs to the caller's reconciliation.
        match result {
            Err(BinancePapiHttpError::BinanceError { code, status, .. }) => {
                assert_eq!(code, -1003);
                assert_eq!(status, 429);
            }
            other => panic!("expected BinanceError, was {other:?}"),
        }

        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert_eq!(bodies.lock().unwrap().as_slice(), [None].as_slice());
    }

    #[tokio::test(start_paused = false)]
    async fn test_ip_throttle_backoff_blocks_next_request_until_expiry() {
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app = Router::new().fallback(any(move || {
            let captured = captured.clone();
            async move {
                if captured.fetch_add(1, Ordering::Relaxed) == 0 {
                    let mut response = (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        r#"{"code":-1003,"msg":"Requests rate too high."}"#,
                    )
                        .into_response();
                    response
                        .headers_mut()
                        .insert("Retry-After", axum::http::HeaderValue::from_static("1"));
                    response
                } else {
                    (axum::http::StatusCode::OK, "{}").into_response()
                }
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = new_client(&url, None).await;
        let first: Result<serde_json::Value, _> = client
            .post::<serde_json::Value, _>(
                "/papi/v1/um/order",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;
        assert!(first.is_err());

        let start = Instant::now();
        let second: Result<serde_json::Value, _> = client
            .get::<serde_json::Value, _>(
                "/papi/v1/balance",
                None::<&serde_json::Value>,
                true,
                false,
            )
            .await;

        server.abort();

        // The shared IP backoff admits the next request no earlier than the venue delay.
        second.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(900));
        assert_eq!(requests.load(Ordering::Relaxed), 2);
    }

    #[rstest]
    fn test_default_base_url_is_official_papi_host() {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            Some("secret".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let url = client.build_url("/papi/v1/ping", "");
        assert_eq!(url, "https://papi.binance.com/papi/v1/ping");
    }

    #[rstest]
    fn test_override_base_url_trailing_slash_normalizes() {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            Some("secret".to_string()),
            Some("http://127.0.0.1:9999/".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let url = client.build_url("/papi/v1/ping", "");
        assert_eq!(url, "http://127.0.0.1:9999/papi/v1/ping");
    }

    #[rstest]
    fn test_rate_limit_keys_carry_weight_and_order_quota() {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            Some("secret".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let read = client.rate_limit_keys(&Method::GET, "/papi/v1/um/income/asyn", "", false);
        assert_eq!(read.len(), 1_500);

        let order = client.rate_limit_keys(&Method::POST, "/papi/v1/um/order", "", true);
        assert_eq!(order.len(), 2);
        assert!(order[1].as_str().starts_with("binance:orders"));
    }

    #[rstest]
    fn test_debug_output_redacts_api_key() {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("papi-test-key".to_string()),
            Some("papi-test-secret".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let formatted = format!("{client:?}");

        assert!(!formatted.contains("papi-test-key"));
        assert!(!formatted.contains("papi-test-secret"));
        assert!(formatted.contains("BinanceRawPapiHttpClient"));
    }

    #[rstest]
    #[case("https://user:url-secret@papi-mirror.example/base#token")]
    fn test_debug_output_redacts_credential_bearing_url_override(#[case] url: &str) {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("papi-test-key".to_string()),
            Some("papi-test-secret".to_string()),
            Some(url.to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let formatted = format!("{client:?}");

        assert!(!formatted.contains("url-secret"));
        assert!(!formatted.contains("token"));
        assert!(formatted.contains("https://papi-mirror.example"));
    }

    #[rstest]
    fn test_debug_output_redacts_url_override_without_scheme() {
        // A scheme-less override is not a valid HTTP(S) URL and prints fully redacted.
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("papi-test-key".to_string()),
            Some("papi-test-secret".to_string()),
            Some("user:url-secret@papi-mirror.example".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let formatted = format!("{client:?}");

        assert!(!formatted.contains("url-secret"));
        assert!(formatted.contains("<redacted>"));
    }

    #[rstest]
    fn test_rate_limit_keys_charge_two_order_units_for_margin_oco() {
        let client = BinanceRawPapiHttpClient::new(
            BinanceEnvironment::Live,
            Some("key".to_string()),
            Some("secret".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let order = client.rate_limit_keys(&Method::POST, "/papi/v1/um/order", "", true);
        let oco = client.rate_limit_keys(&Method::POST, "/papi/v1/margin/order/oco", "", true);

        // One OCO placement consumes two order-count units against the venue limit.
        assert_eq!(order.len(), 2);
        assert_eq!(oco.len(), 3);
    }

    #[rstest]
    fn test_concurrent_clients_share_request_limiter_scope() {
        let quota = Quota::per_minute(NonZeroU32::new(6_000).unwrap());

        let first = BinanceRawPapiHttpClient::request_rate_limiter(
            BinancePapiRequestScope {
                endpoint: BinanceHttpEndpointScope::Custom {
                    environment: BinanceEnvironment::Live,
                    base_url: "https://papi.example".to_string(),
                },
                proxy_url_digest: None,
            },
            quota,
        );

        let second = BinanceRawPapiHttpClient::request_rate_limiter(
            BinancePapiRequestScope {
                endpoint: BinanceHttpEndpointScope::Custom {
                    environment: BinanceEnvironment::Live,
                    base_url: "https://papi.example".to_string(),
                },
                proxy_url_digest: None,
            },
            quota,
        );

        assert!(Arc::ptr_eq(&first, &second));

        let other = BinanceRawPapiHttpClient::request_rate_limiter(
            BinancePapiRequestScope {
                endpoint: BinanceHttpEndpointScope::Custom {
                    environment: BinanceEnvironment::Live,
                    base_url: "https://papi-other.example".to_string(),
                },
                proxy_url_digest: None,
            },
            quota,
        );

        assert!(!Arc::ptr_eq(&first, &other));
    }
}

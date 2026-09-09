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

//! HTTP client implementation with rate limiting and timeout support.

use std::{borrow::Cow, collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::Bytes;
use http::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use http_body_util::Full;
use nautilus_core::{collections::into_ustr_vec, string::secret::SecretString};
use nautilus_cryptography::providers::install_cryptographic_provider;
use url::Url;
use ustr::Ustr;

use super::{
    HttpClientError, HttpResponse, HttpResponseStream, HttpStatus,
    stream::{read_chunk, response_error},
};
use crate::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};

/// Default maximum idle connections per host.
#[cfg(not(all(feature = "simulation", madsim)))]
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 32;

/// Default idle connection timeout in seconds.
#[cfg(not(all(feature = "simulation", madsim)))]
const DEFAULT_POOL_IDLE_TIMEOUT_SECS: u64 = 60;

/// Default HTTP/2 keep-alive interval in seconds.
#[cfg(not(all(feature = "simulation", madsim)))]
const DEFAULT_HTTP2_KEEP_ALIVE_SECS: u64 = 30;

/// Default maximum HTTP response body size in bytes (100 MiB).
///
/// Bounds peak memory per response so a hostile or malfunctioning endpoint
/// cannot exhaust memory by streaming an arbitrarily large body. Mirrors the
/// caps already enforced on the WebSocket and raw-socket paths.
const DEFAULT_MAX_RESPONSE_BYTES: usize = 100 * 1024 * 1024;

#[cfg(all(feature = "simulation", madsim))]
pub(super) const REQUEST_TIMEOUT_MESSAGE: &str = "simulated request deadline elapsed";
#[cfg(not(all(feature = "simulation", madsim)))]
pub(super) const REQUEST_TIMEOUT_MESSAGE: &str = "request deadline elapsed";

/// Controls whether an HTTP client follows redirects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HttpRedirectPolicy {
    /// Follow up to ten redirects.
    #[default]
    Follow,
    /// Reject every redirect response.
    Reject,
}

/// An asynchronous HTTP client with rate limiting, timeouts, and custom headers.
///
/// The client uses Hyper for normal I/O and supports default and per-key quotas. Multiple
/// clients can share the same rate limiter when their requests consume one quota budget.
/// With `simulation` and `cfg(madsim)`, plaintext HTTP/1.1 uses simulated byte streams;
/// HTTPS, explicit proxies, and redirect following are unsupported.
#[derive(Clone, Debug)]
pub struct HttpClient {
    pub(crate) client: InnerHttpClient,
    pub(crate) rate_limiters: Arc<[Arc<RateLimiter<Ustr, MonotonicClock>>]>,
}

#[bon::bon]
impl HttpClient {
    /// Returns a builder for a new [`HttpClient`] instance.
    ///
    /// Set `rate_limiters` to share quota state across clients. When omitted, the client creates
    /// one rate limiter from `default_quota` and `keyed_quotas`. An explicit empty vector disables
    /// rate limiting. Each request awaits every configured limiter with the same keys. A limiter
    /// without a default quota ignores keys it does not own, allowing independent scopes such as
    /// per-IP and per-account limits to apply to one request.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Shared rate limiters are combined with quota configuration.
    /// - The proxy URL is malformed.
    /// - Building the underlying HTTP transport fails.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "owned proxy URLs are part of the public builder API"
    )]
    #[builder(finish_fn = build)]
    pub fn builder(
        #[builder(default)] headers: HashMap<String, String>,
        #[builder(default)] header_keys: Vec<String>,
        #[builder(default)] keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        rate_limiters: Option<Vec<Arc<RateLimiter<Ustr, MonotonicClock>>>>,
        #[builder(default)] redirect_policy: HttpRedirectPolicy,
        #[builder(default = true)] use_system_proxy: bool,
    ) -> Result<Self, HttpClientError> {
        let rate_limiters = if let Some(rate_limiters) = rate_limiters {
            if default_quota.is_some() || !keyed_quotas.is_empty() {
                return Err(HttpClientError::Error(
                    "Cannot combine shared rate limiters with quota configuration".to_string(),
                ));
            }
            rate_limiters
        } else {
            let keyed_quotas = keyed_quotas
                .into_iter()
                .map(|(key, quota)| (Ustr::from(&key), quota))
                .collect();
            vec![Arc::new(RateLimiter::new_with_quota(
                default_quota,
                keyed_quotas,
            ))]
        };

        Self::build(
            headers,
            header_keys,
            timeout_secs,
            proxy_url.as_deref(),
            rate_limiters,
            redirect_policy,
            use_system_proxy,
        )
    }

    fn build(
        headers: HashMap<String, String>,
        header_keys: Vec<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<&str>,
        rate_limiters: Vec<Arc<RateLimiter<Ustr, MonotonicClock>>>,
        redirect_policy: HttpRedirectPolicy,
        use_system_proxy: bool,
    ) -> Result<Self, HttpClientError> {
        install_cryptographic_provider();

        let mut header_map = HeaderMap::new();

        for (key, value) in headers {
            let header_name = HeaderName::from_str(&key)
                .map_err(|e| HttpClientError::Error(format!("Invalid header name '{key}': {e}")))?;
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                HttpClientError::Error(format!("Invalid header value for '{key}': {e}"))
            })?;
            header_map.insert(header_name, header_value);
        }

        #[cfg(all(feature = "simulation", madsim))]
        let simulation = super::simulation::Client::new(redirect_policy, proxy_url)?;

        #[cfg(not(all(feature = "simulation", madsim)))]
        let client = super::transport::Client::new(
            proxy_url,
            use_system_proxy,
            super::transport::Settings {
                pool_max_idle_per_host: DEFAULT_POOL_MAX_IDLE_PER_HOST,
                pool_idle_timeout: Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT_SECS),
                keep_alive_interval: Some(Duration::from_secs(DEFAULT_HTTP2_KEEP_ALIVE_SECS)),
                adaptive_window: true,
            },
        )?;
        #[cfg(all(feature = "simulation", madsim))]
        let _ = use_system_proxy;

        // Pre-intern header keys as HeaderName. An invalid key is an error: a silent drop would
        // make response extraction read nothing.
        let response_headers = header_keys
            .into_iter()
            .map(|key| match HeaderName::from_str(&key) {
                Ok(name) => Ok((key, name)),
                Err(e) => Err(HttpClientError::Error(format!(
                    "Invalid header key '{key}': {e}"
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let client = InnerHttpClient {
            #[cfg(not(all(feature = "simulation", madsim)))]
            client,
            headers: header_map,
            timeout: timeout_secs.map(Duration::from_secs),
            #[cfg(not(all(feature = "simulation", madsim)))]
            redirect_policy,
            #[cfg(all(feature = "simulation", madsim))]
            simulation,
            response_headers: Arc::from(response_headers),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        };

        Ok(Self {
            client,
            rate_limiters: rate_limiters.into(),
        })
    }

    /// Sends an HTTP request.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    ///
    /// # Examples
    ///
    /// If requesting `/foo/bar`, pass rate-limit keys `["foo/bar", "foo"]`.
    #[expect(clippy::too_many_arguments)]
    pub async fn request(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        let keys = keys.map(into_ustr_vec);

        self.request_with_ustr_keys(method, url, params, headers, body, timeout_secs, keys)
            .await
    }

    /// Sends an HTTP request whose body contains secret material.
    ///
    /// The body retains its zeroizing owner until the transport releases the last byte buffer.
    /// Transport, TLS, and operating-system layers may make additional plaintext copies that this
    /// client cannot zeroize.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send the request or if it times out.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_with_secret_body(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: SecretString,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        let keys = keys.map(into_ustr_vec);
        self.await_rate_limits(keys.as_deref()).await;

        self.client
            .send_request_with_secret_body(method, url, params, headers, body, timeout_secs)
            .await
    }

    /// Sends an HTTP request while redacting the URL from logs and transport errors.
    ///
    /// Use this for endpoints whose path or other URL components can carry credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_with_url_redacted(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        let keys = keys.map(into_ustr_vec);
        self.await_rate_limits(keys.as_deref()).await;

        self.client
            .send_request_with_url_redacted(method, url, params, headers, body, timeout_secs)
            .await
    }

    /// Sends an HTTP request with serializable query parameters.
    ///
    /// This method accepts any type implementing `Serialize` for query parameters,
    /// which are URL-encoded directly into the query string without an intermediate `HashMap`.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_with_params<P: serde::Serialize>(
        &self,
        method: Method,
        url: String,
        params: Option<&P>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        let keys = keys.map(into_ustr_vec);
        self.await_rate_limits(keys.as_deref()).await;

        self.client
            .send_request_with_query(method, url, params, headers, body, timeout_secs)
            .await
    }

    /// Sends an HTTP request with serializable query parameters while redacting the URL from logs
    /// and transport errors.
    ///
    /// Use this for query parameters that can carry credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_with_params_url_redacted<P: serde::Serialize>(
        &self,
        method: Method,
        url: String,
        params: Option<&P>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        let keys = keys.map(into_ustr_vec);
        self.await_rate_limits(keys.as_deref()).await;

        self.client
            .send_request_with_query_url_redacted(method, url, params, headers, body, timeout_secs)
            .await
    }

    /// Sends a GET request and returns its response body as a stream.
    ///
    /// Applies default headers and the client timeout. No rate-limit keys are supplied, so no
    /// quota is consumed. One absolute deadline covers response headers and the whole body,
    /// including time spent processing chunks. Streaming has no total body size limit; callers
    /// must process or discard each chunk without accumulating an unbounded body.
    /// Dropping the response releases the unfinished exchange, including its simulated driver.
    ///
    /// # Errors
    ///
    /// Returns an error if request preparation, connection, or response headers fail or time out.
    pub async fn get_stream(&self, url: String) -> Result<HttpResponseStream, HttpClientError> {
        self.await_rate_limits(None).await;
        self.client
            .send_stream_internal::<[(String, String); 0]>(
                Method::GET,
                &url,
                None,
                None,
                None,
                None,
                false,
            )
            .await
    }

    /// Sends an HTTP request using pre-interned rate limiter keys.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send the request or the request times out.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_with_ustr_keys(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<Ustr>>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.await_rate_limits(keys.as_deref()).await;

        self.client
            .send_request(method, url, params, headers, body, timeout_secs)
            .await
    }

    pub(crate) async fn await_rate_limits(&self, keys: Option<&[Ustr]>) {
        RateLimiter::await_limiters_ready(&self.rate_limiters, keys).await;
    }

    /// Sends an HTTP GET request.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn get(
        &self,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(Method::GET, url, params, headers, None, timeout_secs, keys)
            .await
    }

    /// Sends an HTTP POST request.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn post(
        &self,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(Method::POST, url, params, headers, body, timeout_secs, keys)
            .await
    }

    /// Sends an HTTP PATCH request.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn patch(
        &self,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(
            Method::PATCH,
            url,
            params,
            headers,
            body,
            timeout_secs,
            keys,
        )
        .await
    }

    /// Sends an HTTP DELETE request.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn delete(
        &self,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        timeout_secs: Option<u64>,
        keys: Option<Vec<String>>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(
            Method::DELETE,
            url,
            params,
            headers,
            None,
            timeout_secs,
            keys,
        )
        .await
    }
}

/// Internal implementation backing [`HttpClient`].
///
/// The underlying Hyper client reuses pooled connections and is cheap to clone. Responses
/// retain only configured header fields, and bodies larger than `max_response_bytes` are rejected.
#[derive(Clone, Debug)]
pub struct InnerHttpClient {
    #[cfg(all(feature = "simulation", madsim))]
    simulation: super::simulation::Client,
    #[cfg(not(all(feature = "simulation", madsim)))]
    client: super::transport::Client,
    headers: HeaderMap,
    timeout: Option<Duration>,
    #[cfg(not(all(feature = "simulation", madsim)))]
    redirect_policy: HttpRedirectPolicy,
    pub(crate) response_headers: Arc<[(String, HeaderName)]>,
    pub(crate) max_response_bytes: usize,
}

impl InnerHttpClient {
    /// Sends an HTTP request and returns an [`HttpResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.send_request_with_redaction(
            method,
            url,
            params,
            headers,
            body.map(RequestBody::Plain),
            timeout_secs,
            false,
        )
        .await
    }

    async fn send_request_with_secret_body(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: SecretString,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.send_request_with_redaction(
            method,
            url,
            params,
            headers,
            Some(RequestBody::Secret(body)),
            timeout_secs,
            false,
        )
        .await
    }

    async fn send_request_with_url_redacted(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.send_request_with_redaction(
            method,
            url,
            params,
            headers,
            body.map(RequestBody::Plain),
            timeout_secs,
            true,
        )
        .await
    }

    #[expect(clippy::too_many_arguments)]
    async fn send_request_with_redaction(
        &self,
        method: Method,
        url: String,
        params: Option<&HashMap<String, Vec<String>>>,
        headers: Option<HashMap<String, String>>,
        body: Option<RequestBody>,
        timeout_secs: Option<u64>,
        redact_url: bool,
    ) -> Result<HttpResponse, HttpClientError> {
        let full_url = encode_url_params(&url, params)?;
        self.send_request_internal(
            method,
            full_url.as_ref(),
            None::<&()>,
            headers,
            body,
            timeout_secs,
            redact_url,
        )
        .await
    }

    /// Sends an HTTP request with URL-encoded serializable query parameters.
    ///
    /// This method accepts any type implementing `Serialize` for query parameters,
    /// avoiding `HashMap` conversion overhead.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn send_request_with_query<Q: serde::Serialize>(
        &self,
        method: Method,
        url: String,
        query: Option<&Q>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.send_request_internal(
            method,
            &url,
            query,
            headers,
            body.map(RequestBody::Plain),
            timeout_secs,
            false,
        )
        .await
    }

    async fn send_request_with_query_url_redacted<Q: serde::Serialize>(
        &self,
        method: Method,
        url: String,
        query: Option<&Q>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        self.send_request_internal(
            method,
            &url,
            query,
            headers,
            body.map(RequestBody::Plain),
            timeout_secs,
            true,
        )
        .await
    }

    /// Internal implementation for sending HTTP requests.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    #[expect(clippy::too_many_arguments)]
    async fn send_request_internal<Q: serde::Serialize>(
        &self,
        method: Method,
        url: &str,
        query: Option<&Q>,
        headers: Option<HashMap<String, String>>,
        body: Option<RequestBody>,
        timeout_secs: Option<u64>,
        redact_url: bool,
    ) -> Result<HttpResponse, HttpClientError> {
        let stream = self
            .send_stream_internal(method, url, query, headers, body, timeout_secs, redact_url)
            .await?;
        let result = self
            .consume_response(stream.response, stream.deadline)
            .await;
        result.map_err(|e| response_error(e, stream.url.as_ref()))
    }

    #[expect(clippy::too_many_arguments)]
    async fn send_stream_internal<Q: serde::Serialize>(
        &self,
        method: Method,
        url: &str,
        query: Option<&Q>,
        headers: Option<HashMap<String, String>>,
        body: Option<RequestBody>,
        timeout_secs: Option<u64>,
        redact_url: bool,
    ) -> Result<HttpResponseStream, HttpClientError> {
        let mut url =
            Url::parse(url).map_err(|e| HttpClientError::from(format!("URL parse error: {e}")))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(HttpClientError::Error(
                "unsupported HTTP URL scheme or hostname".into(),
            ));
        }

        let mut header_map = self.headers.clone();

        if let Ok(username) = percent_encoding::percent_decode_str(url.username()).decode_utf8() {
            let password = url.password().and_then(|password| {
                percent_encoding::percent_decode_str(password)
                    .decode_utf8()
                    .ok()
            });

            if !username.is_empty() || password.is_some() {
                let mut value = HeaderValue::from_str(&format!(
                    "Basic {}",
                    BASE64.encode(format!(
                        "{username}:{}",
                        password.as_deref().unwrap_or_default()
                    ))
                ))
                .map_err(|e| HttpClientError::Error(e.to_string()))?;
                value.set_sensitive(true);
                header_map.insert(http::header::AUTHORIZATION, value);
                let _ = url.set_username("");
                let _ = url.set_password(None);
            }
        }

        let extra_header_count = headers.as_ref().map_or(0, HashMap::len);

        if let Some(headers) = headers {
            for (key, value) in headers {
                let key = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|e| HttpClientError::from(format!("Invalid header name: {e}")))?;
                let value = HeaderValue::from_str(&value)
                    .map_err(|e| HttpClientError::from(format!("Invalid header value: {e}")))?;
                if header_map.insert(key.clone(), value).is_some() {
                    log::trace!("Replaced duplicate request header '{key}'");
                }
            }
        }

        if let Some(query) = query {
            {
                let mut pairs = url.query_pairs_mut();
                let serializer = serde_urlencoded::Serializer::new(&mut pairs);
                query
                    .serialize(serializer)
                    .map_err(|e| HttpClientError::Error(e.to_string()))?;
            }

            if url.query() == Some("") {
                url.set_query(None);
            }
        }

        if !header_map.contains_key(http::header::ACCEPT) {
            header_map.insert(http::header::ACCEPT, HeaderValue::from_static("*/*"));
        }

        let body = body.map(RequestBody::into_bytes).unwrap_or_default();
        let body_len = body.len();
        let query_len = url.query().map_or(0, str::len);
        let mut request = http::Request::new(Full::new(body));
        *request.method_mut() = method;
        *request.uri_mut() = url[..url::Position::AfterQuery]
            .parse()
            .map_err(|_| HttpClientError::Error("invalid HTTP request target".into()))?;
        *request.headers_mut() = header_map;
        log::trace!(
            "Sending HTTP request: method={} extra_headers={extra_header_count} \
             query_bytes={query_len} body_bytes={body_len}",
            request.method(),
        );

        let duration = timeout_secs.map(Duration::from_secs).or(self.timeout);
        let deadline = duration.map(|duration| crate::dst::time::Instant::now() + duration);
        let operation = async {
            #[cfg(all(feature = "simulation", madsim))]
            let (response, connection) = self.simulation.send(request, &url).await?;
            #[cfg(not(all(feature = "simulation", madsim)))]
            let response = self.client.send(request, self.redirect_policy).await?;
            Ok(HttpResponseStream {
                response,
                deadline,
                url: (!redact_url).then(|| url.clone()),
                #[cfg(all(feature = "simulation", madsim))]
                _connection: connection,
            })
        };

        let result = match deadline {
            Some(deadline) => tokio::select! {
                biased;
                () = crate::dst::time::sleep_until(deadline) => Err(HttpClientError::TimeoutError(REQUEST_TIMEOUT_MESSAGE.into())),
                result = operation => result,
            },
            None => operation.await,
        };
        result.map_err(|e| response_error(e, (!redact_url).then_some(&url)))
    }

    async fn consume_response<B>(
        &self,
        response: http::Response<B>,
        deadline: Option<crate::dst::time::Instant>,
    ) -> Result<HttpResponse, HttpClientError>
    where
        B: http_body::Body<Data = Bytes> + Unpin,
        B::Error: std::error::Error + 'static,
    {
        let (parts, mut body) = response.into_parts();
        let mut headers =
            HashMap::with_capacity(self.response_headers.len().min(parts.headers.len()));
        for (key, name) in self.response_headers.iter() {
            if let Some(value) = parts
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
            {
                headers.insert(key.clone(), value.to_owned());
            }
        }

        let max = self.max_response_bytes;
        if let Some(len) = body.size_hint().exact()
            && len > max as u64
        {
            return Err(HttpClientError::Error(format!(
                "HTTP response body of {len} bytes exceeds maximum of {max} bytes",
            )));
        }

        let mut buf = bytes::BytesMut::new();
        while let Some(chunk) = read_chunk(&mut body, deadline).await? {
            if chunk.len() > max - buf.len() {
                return Err(HttpClientError::Error(format!(
                    "HTTP response body exceeds maximum of {max} bytes",
                )));
            }
            buf.extend_from_slice(&chunk);
        }

        log::trace!(
            "Received HTTP response: status={} headers={} body_bytes={}",
            parts.status,
            parts.headers.len(),
            buf.len()
        );
        Ok(HttpResponse {
            status: HttpStatus::new(parts.status),
            headers,
            body: buf.freeze(),
        })
    }
}

enum RequestBody {
    Plain(Vec<u8>),
    Secret(SecretString),
}

impl RequestBody {
    fn into_bytes(self) -> Bytes {
        match self {
            Self::Plain(body) => body.into(),
            Self::Secret(body) => Bytes::from_owner(SecretBody(body)),
        }
    }
}

struct SecretBody(SecretString);

impl AsRef<[u8]> for SecretBody {
    fn as_ref(&self) -> &[u8] {
        self.0.expose_secret().as_bytes()
    }
}

impl Default for InnerHttpClient {
    /// Creates a new default [`InnerHttpClient`] instance.
    ///
    /// The default client has an empty list of response header keys. Production clients reuse a
    /// connection pool; simulated clients open a connection per request.
    ///
    /// # Panics
    ///
    /// Panics if the production HTTP transport cannot be initialized.
    fn default() -> Self {
        install_cryptographic_provider();
        #[cfg(not(all(feature = "simulation", madsim)))]
        let client =
            super::transport::Client::new(None, true, super::transport::Settings::default())
                .expect("failed to build default HTTP client");
        Self {
            #[cfg(not(all(feature = "simulation", madsim)))]
            client,
            headers: HeaderMap::new(),
            timeout: None,
            #[cfg(not(all(feature = "simulation", madsim)))]
            redirect_policy: HttpRedirectPolicy::default(),
            #[cfg(all(feature = "simulation", madsim))]
            simulation: super::simulation::Client::default(),
            response_headers: Arc::default(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

/// Encodes URL parameters into the query string.
///
/// Returns `Cow::Borrowed` when no parameters need appending (zero-alloc fast path).
/// Parameters can have multiple values per key (for doseq=True behavior).
/// Preserves existing query strings in the URL by appending with '&' instead of '?'.
/// The query is inserted before any fragment, which is preserved unchanged.
fn encode_url_params<'a>(
    url: &'a str,
    params: Option<&HashMap<String, Vec<String>>>,
) -> Result<Cow<'a, str>, HttpClientError> {
    let Some(params) = params else {
        return Ok(Cow::Borrowed(url));
    };

    let pairs: Vec<(&str, &str)> = params
        .iter()
        .flat_map(|(key, values)| {
            values
                .iter()
                .map(move |value| (key.as_str(), value.as_str()))
        })
        .collect();

    if pairs.is_empty() {
        return Ok(Cow::Borrowed(url));
    }

    let query_string = serde_urlencoded::to_string(pairs)
        .map_err(|e| HttpClientError::Error(format!("Failed to encode params: {e}")))?;

    // The first literal '#' starts the fragment per RFC 3986 section 3.5.
    // A data '#' in an earlier component must be percent-encoded as "%23".
    let (base, fragment) = match url.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (url, None),
    };
    let separator = if base.contains('?') { '&' } else { '?' };

    Ok(Cow::Owned(match fragment {
        Some(fragment) => format!("{base}{separator}{query_string}#{fragment}"),
        None => format!("{base}{separator}{query_string}"),
    }))
}

#[cfg(test)]
mod encode_url_params_tests {
    use std::{borrow::Cow, collections::HashMap};

    use rstest::rstest;

    use super::encode_url_params;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();

        for (key, value) in pairs {
            map.entry((*key).to_string())
                .or_default()
                .push((*value).to_string());
        }

        map
    }

    #[rstest]
    #[case("https://x/y", "https://x/y?a=b")]
    #[case("https://x/y?old=1", "https://x/y?old=1&a=b")]
    #[case("https://x/y#frag", "https://x/y?a=b#frag")]
    #[case("https://x/y?old=1#frag", "https://x/y?old=1&a=b#frag")]
    #[case(
        "https://x/y#section?display=full",
        "https://x/y?a=b#section?display=full"
    )]
    #[case("https://x/y#", "https://x/y?a=b#")]
    fn test_query_is_inserted_before_the_fragment(#[case] url: &str, #[case] expected: &str) {
        let params = params(&[("a", "b")]);

        assert_eq!(encode_url_params(url, Some(&params)).unwrap(), expected);
    }

    #[rstest]
    fn test_url_is_borrowed_when_no_params_are_supplied() {
        assert!(matches!(
            encode_url_params("https://x/y#frag", None).unwrap(),
            Cow::Borrowed("https://x/y#frag")
        ));
    }

    #[rstest]
    fn test_url_is_borrowed_when_params_are_empty() {
        let params = HashMap::new();

        assert!(matches!(
            encode_url_params("https://x/y#frag", Some(&params)).unwrap(),
            Cow::Borrowed("https://x/y#frag")
        ));
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
#[cfg(not(all(feature = "simulation", madsim)))]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        Router,
        body::to_bytes,
        extract::Request,
        response::IntoResponse,
        routing::{any, delete, get, patch, post},
        serve,
    };
    use http::status::StatusCode;
    use log::Level;
    use rstest::rstest;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::oneshot,
    };

    use super::*;
    use crate::logging::tests::capture_logs;

    async fn capture_request(request: Request) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();
        let default_header = parts.headers.get("x-default").unwrap().to_str().unwrap();
        let request_header = parts.headers.get("x-request").unwrap().to_str().unwrap();
        let query = parts.uri.query().unwrap_or_default();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let capture = format!(
            "{}\n{}\n{query}\n{default_header}\n{request_header}\n{body}",
            parts.method,
            parts.uri.path(),
        );

        ([("x-response-id", "response-42")], capture)
    }

    fn create_router() -> Router {
        Router::new()
            .route("/get", get(|| async { "hello-world!" }))
            .route("/post", post(|body: Bytes| async move { body }))
            .route("/patch", patch(|body: Bytes| async move { body }))
            .route("/delete", delete(|| async { StatusCode::OK }))
            .route("/capture", any(capture_request))
            .route("/notfound", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/redirect",
                get(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/get")]) }),
            )
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "Eventually responded"
                }),
            )
            .route(
                "/large",
                // Returns a 1 MiB body to exercise the response size cap.
                get(|| async { "x".repeat(1024 * 1024) }),
            )
    }

    async fn start_test_server() -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            serve(listener, create_router()).await.unwrap();
        });

        Ok(addr)
    }

    async fn spawn_connection_dropper() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                drop(stream);
            }
        });

        (addr, task)
    }

    async fn spawn_chunked_response_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];

            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n\
                      5\r\nfirst\r\n6\r\nsecond\r\n0\r\n\r\n",
                )
                .await
                .unwrap();
        });

        (addr, task)
    }

    async fn spawn_rejecting_connect_proxy() -> (SocketAddr, oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            request_tx
                .send(String::from_utf8(request).unwrap())
                .unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .unwrap();
        });

        (addr, request_rx)
    }

    #[tokio::test(start_paused = true)]
    async fn test_body_ready_at_deadline_is_rejected() {
        let client = InnerHttpClient::default();
        let response = http::Response::new(Full::new(Bytes::from_static(b"ready")));
        let result = client
            .consume_response(response, Some(crate::dst::time::Instant::now()))
            .await;
        assert!(
            matches!(result, Err(HttpClientError::TimeoutError(message)) if message == REQUEST_TIMEOUT_MESSAGE)
        );
    }

    #[tokio::test]
    async fn test_get() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(Method::GET, format!("{url}/get"), None, None, None, None)
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"hello-world!");
    }

    #[tokio::test]
    async fn test_request_preserves_wire_semantics_and_extracts_response_headers() {
        let addr = start_test_server().await.unwrap();
        let mut default_headers = HashMap::new();
        default_headers.insert("x-default".to_string(), "default-a".to_string());
        let client = HttpClient::builder()
            .headers(default_headers)
            .header_keys(vec!["x-response-id".to_string()])
            .build()
            .unwrap();
        let mut params = HashMap::new();
        params.insert(
            "tag".to_string(),
            vec!["A B".to_string(), "C/D".to_string()],
        );
        let mut request_headers = HashMap::new();
        request_headers.insert("x-request".to_string(), "request-b".to_string());

        let response = client
            .request(
                Method::PUT,
                format!("http://{addr}/capture?existing=seed"),
                Some(&params),
                Some(request_headers),
                Some(b"payload-c".to_vec()),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(
            response.headers,
            HashMap::from([("x-response-id".to_string(), "response-42".to_string())])
        );
        assert_eq!(
            response.body.as_ref(),
            b"PUT\n/capture\nexisting=seed&tag=A+B&tag=C%2FD\ndefault-a\nrequest-b\npayload-c"
        );
    }

    #[tokio::test]
    async fn test_request_with_secret_body_preserves_wire_body() {
        let addr = start_test_server().await.unwrap();
        let client = HttpClient::builder()
            .headers(HashMap::from([(
                "x-default".to_string(),
                "default-secret".to_string(),
            )]))
            .build()
            .unwrap();
        let headers = HashMap::from([("x-request".to_string(), "request-secret".to_string())]);

        let response = client
            .request_with_secret_body(
                Method::POST,
                format!("http://{addr}/capture"),
                None,
                Some(headers),
                SecretString::from("credential-body"),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(
            response.body.as_ref(),
            b"POST\n/capture\n\ndefault-secret\nrequest-secret\ncredential-body"
        );
    }

    #[tokio::test]
    async fn test_request_with_params_serializes_query_fields() {
        #[derive(serde::Serialize)]
        struct Query<'a> {
            symbol: &'a str,
            limit: u32,
        }

        let addr = start_test_server().await.unwrap();
        let mut default_headers = HashMap::new();
        default_headers.insert("x-default".to_string(), "default-d".to_string());
        let client = HttpClient::builder()
            .headers(default_headers)
            .header_keys(vec!["x-response-id".to_string()])
            .build()
            .unwrap();
        let mut request_headers = HashMap::new();
        request_headers.insert("x-request".to_string(), "request-e".to_string());
        let params = Query {
            symbol: "BTC/USDT",
            limit: 37,
        };

        let response = client
            .request_with_params(
                Method::GET,
                format!("http://{addr}/capture"),
                Some(&params),
                Some(request_headers),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(
            response.headers,
            HashMap::from([("x-response-id".to_string(), "response-42".to_string())])
        );
        assert_eq!(
            response.body.as_ref(),
            b"GET\n/capture\nsymbol=BTC%2FUSDT&limit=37\ndefault-d\nrequest-e\n"
        );
    }

    #[tokio::test]
    async fn test_request_with_params_url_redacted_preserves_query_fields() {
        #[derive(serde::Serialize)]
        struct Query<'a> {
            auth: &'a str,
            market_id: i16,
        }

        let addr = start_test_server().await.unwrap();
        let client = HttpClient::builder()
            .headers(HashMap::from([(
                "x-default".to_string(),
                "default-f".to_string(),
            )]))
            .build()
            .unwrap();
        let headers = HashMap::from([("x-request".to_string(), "request-g".to_string())]);
        let params = Query {
            auth: "token/42",
            market_id: 7,
        };

        let response = client
            .request_with_params_url_redacted(
                Method::GET,
                format!("http://{addr}/capture"),
                Some(&params),
                Some(headers),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(
            response.body.as_ref(),
            b"GET\n/capture\nauth=token%2F42&market_id=7\ndefault-f\nrequest-g\n"
        );
    }

    #[rstest]
    #[case::empty_at_zero_cap(b"", 0)]
    #[case::at_cap(b"body-37", 7)]
    #[case::below_cap(b"body-37", 8)]
    #[tokio::test]
    async fn test_declared_response_body_at_or_below_cap_is_returned(
        #[case] bytes: &'static [u8],
        #[case] max_response_bytes: usize,
    ) {
        let client = InnerHttpClient {
            max_response_bytes,
            ..Default::default()
        };
        let response = http::Response::new(Full::new(Bytes::from_static(bytes)));

        let response = client.consume_response(response, None).await.unwrap();

        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), bytes);
    }

    #[tokio::test]
    async fn test_response_body_within_cap_is_returned() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        // Cap above the 1 MiB payload: body should be returned intact.
        let client = InnerHttpClient {
            max_response_bytes: 4 * 1024 * 1024,
            ..Default::default()
        };

        let response = client
            .send_request(Method::GET, format!("{url}/large"), None, None, None, None)
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), vec![b'x'; 1024 * 1024]);
    }

    #[tokio::test]
    async fn test_response_body_exceeding_cap_is_rejected() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        // Cap below the 1 MiB payload: the request must fail rather than buffer it.
        let client = InnerHttpClient {
            max_response_bytes: 16 * 1024,
            ..Default::default()
        };

        let result = client
            .send_request(Method::GET, format!("{url}/large"), None, None, None, None)
            .await;

        let err = result.expect_err("oversized response body should be rejected");
        let HttpClientError::Error(message) = err else {
            panic!("expected HTTP error, was {err:?}");
        };
        assert_eq!(
            message,
            "HTTP response body of 1048576 bytes exceeds maximum of 16384 bytes"
        );
    }

    #[rstest]
    #[case::at_cap(11)]
    #[case::below_cap(12)]
    #[tokio::test]
    async fn test_chunked_response_body_at_or_below_cap_is_returned(
        #[case] max_response_bytes: usize,
    ) {
        let (addr, server_task) = spawn_chunked_response_server().await;
        let client = InnerHttpClient {
            max_response_bytes,
            ..Default::default()
        };

        let response = client
            .send_request(
                Method::GET,
                format!("http://{addr}"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        server_task.await.unwrap();

        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"firstsecond");
    }

    #[tokio::test]
    async fn test_chunked_response_body_exceeding_cap_is_rejected() {
        let (addr, server_task) = spawn_chunked_response_server().await;
        let max_response_bytes = 8;
        let client = InnerHttpClient {
            max_response_bytes,
            ..Default::default()
        };

        let error = client
            .send_request(
                Method::GET,
                format!("http://{addr}"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("chunked response body should be rejected");
        server_task.await.unwrap();

        let HttpClientError::Error(message) = error else {
            panic!("expected HTTP error, was {error:?}");
        };
        assert_eq!(
            message,
            format!("HTTP response body exceeds maximum of {max_response_bytes} bytes")
        );
    }

    #[tokio::test]
    async fn test_post() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(Method::POST, format!("{url}/post"), None, None, None, None)
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }

    #[tokio::test]
    async fn test_post_with_body() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();

        let mut body = HashMap::new();
        body.insert(
            "key1".to_string(),
            serde_json::Value::String("value1".to_string()),
        );
        body.insert(
            "key2".to_string(),
            serde_json::Value::String("value2".to_string()),
        );

        let body_string = serde_json::to_string(&body).unwrap();
        let body_bytes = body_string.into_bytes();

        let response = client
            .send_request(
                Method::POST,
                format!("{url}/post"),
                None,
                None,
                Some(body_bytes.clone()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), body_bytes);
    }

    #[tokio::test]
    async fn test_patch() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                Method::PATCH,
                format!("{url}/patch"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }

    #[tokio::test]
    async fn test_delete() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                Method::DELETE,
                format!("{url}/delete"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }

    #[tokio::test]
    async fn test_not_found() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/notfound");
        let client = InnerHttpClient::default();

        let response = client
            .send_request(Method::GET, url, None, None, None, None)
            .await
            .unwrap();

        assert!(response.status.is_client_error());
        assert_eq!(response.status.as_u16(), 404);
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }

    #[tokio::test]
    async fn test_timeout() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/slow");
        let client = InnerHttpClient::default();

        // We'll set a 1-second timeout for a route that sleeps 2 seconds
        let result = client
            .send_request(Method::GET, url, None, None, None, Some(1))
            .await;

        assert!(
            matches!(&result, Err(HttpClientError::TimeoutError(_))),
            "Expected a timeout error, was: {result:?}"
        );
    }

    #[rstest]
    fn test_http_client_without_proxy() {
        // Create client with no proxy
        let result = HttpClient::builder().build();

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_builder_preserves_empty_rate_limiters() {
        let client = HttpClient::builder()
            .rate_limiters(Vec::new())
            .build()
            .unwrap();

        assert!(client.rate_limiters.is_empty());
    }

    #[rstest]
    fn test_http_client_builder_rejects_shared_rate_limiters_with_quotas() {
        let quota = Quota::with_period(Duration::from_secs(1)).unwrap();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(None, Vec::new()));
        let result = HttpClient::builder()
            .default_quota(quota)
            .rate_limiters(vec![rate_limiter])
            .build();

        assert_eq!(
            result.unwrap_err().to_string(),
            "HTTP error occurred: Cannot combine shared rate limiters with quota configuration"
        );
    }

    #[tokio::test]
    async fn test_http_client_without_proxy_requests_directly() {
        let addr = start_test_server().await.unwrap();
        let client = HttpClient::builder().timeout_secs(2).build().unwrap();
        let response = client
            .request(
                Method::GET,
                format!("http://{addr}/get"),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("direct request");

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.body.as_ref(), b"hello-world!");
    }

    #[tokio::test]
    async fn test_http_client_redirect_policy() {
        let addr = start_test_server().await.unwrap();
        let follow = HttpClient::builder().timeout_secs(2).build().unwrap();
        let reject = HttpClient::builder()
            .timeout_secs(2)
            .redirect_policy(HttpRedirectPolicy::Reject)
            .build()
            .unwrap();

        let followed = follow
            .request(
                Method::GET,
                format!("http://{addr}/redirect"),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let rejected = reject
            .request(
                Method::GET,
                format!("http://{addr}/redirect"),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(followed.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(followed.body.as_ref(), b"hello-world!");
        assert_eq!(
            rejected.status.as_u16(),
            StatusCode::TEMPORARY_REDIRECT.as_u16()
        );
        assert!(rejected.body.is_empty());
    }

    #[tokio::test]
    async fn test_http_client_redacted_url_request_preserves_response() {
        let addr = start_test_server().await.unwrap();
        let client = HttpClient::builder().timeout_secs(2).build().unwrap();
        let response = client
            .request_with_url_redacted(
                Method::GET,
                format!("http://{addr}/get"),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("direct request with URL redaction");

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.body.as_ref(), b"hello-world!");
    }

    #[tokio::test]
    async fn test_http_client_redacted_url_request_removes_endpoint_from_error() {
        const USERINFO_SECRET: &str = "transport-userinfo-secret";
        const PATH_SECRET: &str = "transport-path-secret";
        const QUERY_SECRET: &str = "transport-query-secret";
        let (addr, drop_task) = spawn_connection_dropper().await;
        let url = format!(
            "http://rpc-user:{USERINFO_SECRET}@{addr}/{PATH_SECRET}?api_key={QUERY_SECRET}"
        );
        let client = HttpClient::builder().timeout_secs(1).build().unwrap();

        let error = client
            .request_with_url_redacted(Method::GET, url.clone(), None, None, None, None, None)
            .await
            .expect_err("an unreachable endpoint should fail");
        drop_task.abort();
        let task_error = drop_task
            .await
            .expect_err("connection dropper should be cancelled");

        assert!(task_error.is_cancelled());
        for rendered in [error.to_string(), format!("{error:?}")] {
            assert!(!rendered.contains(USERINFO_SECRET));
            assert!(!rendered.contains(PATH_SECRET));
            assert!(!rendered.contains(QUERY_SECRET));
            assert!(!rendered.contains(&url));
        }
    }

    #[tokio::test]
    async fn test_request_with_params_url_redacted_removes_query_from_error() {
        const QUERY_SECRET: &str = "transport-query-secret";
        #[derive(serde::Serialize)]
        struct Query<'a> {
            auth: &'a str,
        }

        let (addr, drop_task) = spawn_connection_dropper().await;
        let url = format!("http://{addr}/trades");
        let params = Query { auth: QUERY_SECRET };
        let client = HttpClient::builder().timeout_secs(1).build().unwrap();

        let error = client
            .request_with_params_url_redacted(
                Method::GET,
                url,
                Some(&params),
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("a dropped connection should fail");
        drop_task.abort();
        let task_error = drop_task
            .await
            .expect_err("connection dropper should be cancelled");

        assert!(task_error.is_cancelled());
        for rendered in [error.to_string(), format!("{error:?}")] {
            assert!(!rendered.contains("auth="));
            assert!(!rendered.contains(QUERY_SECRET));
        }
    }

    #[tokio::test]
    async fn test_http_client_redacted_url_request_removes_endpoint_from_trace_logs() {
        const USERINFO_SECRET: &str = "trace-userinfo-secret";
        const PATH_SECRET: &str = "trace-path-secret";
        const QUERY_SECRET: &str = "trace-query-secret";
        let capture = capture_logs().await;
        let addr = start_test_server().await.unwrap();
        let url = format!(
            "http://rpc-user:{USERINFO_SECRET}@{addr}/{PATH_SECRET}?api_key={QUERY_SECRET}"
        );
        let client = HttpClient::builder().timeout_secs(2).build().unwrap();

        let response = client
            .request_with_url_redacted(Method::GET, url.clone(), None, None, None, None, None)
            .await
            .expect("credentialized endpoint should return an HTTP response");
        let messages = capture.messages();

        assert_eq!(response.status.as_u16(), StatusCode::NOT_FOUND.as_u16());
        assert!(messages.iter().any(|(level, message)| {
            *level == Level::Trace && message.starts_with("Sending HTTP request: method=GET")
        }));
        assert!(messages.iter().any(|(level, message)| {
            *level == Level::Trace
                && message.starts_with("Received HTTP response: status=404 Not Found")
        }));

        for (_, message) in messages {
            assert!(!message.contains(USERINFO_SECRET));
            assert!(!message.contains(PATH_SECRET));
            assert!(!message.contains(QUERY_SECRET));
            assert!(!message.contains(&url));
        }
    }

    #[tokio::test]
    async fn test_http_client_uses_connect_and_proxy_authorization_for_https() {
        const USERNAME: &str = "proxytest";
        const PASSWORD: &str = "fixture42";
        let (proxy_addr, request_rx) = spawn_rejecting_connect_proxy().await;
        let client = HttpClient::builder()
            .timeout_secs(2)
            .proxy_url(format!("http://{USERNAME}:{PASSWORD}@{proxy_addr}"))
            .build()
            .unwrap();
        let error = client
            .request(
                Method::GET,
                "https://fixture.example.test/path".to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("proxy should reject CONNECT");
        let request = request_rx.await.expect("captured CONNECT request");
        let mut lines = request.split("\r\n");
        let request_line = lines.next().expect("CONNECT request line");
        let auth_value = lines
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("proxy-authorization")
                    .then_some(value.trim())
            })
            .expect("Proxy-Authorization header");
        let expected_auth = format!("Basic {}", BASE64.encode(format!("{USERNAME}:{PASSWORD}")));

        assert_eq!(request_line, "CONNECT fixture.example.test:443 HTTP/1.1");
        assert_eq!(auth_value, expected_auth);
        assert!(!error.to_string().contains(PASSWORD));
        assert!(!error.to_string().contains(&BASE64.encode(PASSWORD)));
        assert!(!error.to_string().contains(&expected_auth));
    }

    #[tokio::test]
    async fn test_http_client_unreachable_proxy_error_redacts_credentials() {
        const USERNAME: &str = "proxy-user";
        const SECRET: &str = "unreachable-proxy-secret";
        let (proxy_addr, drop_task) = spawn_connection_dropper().await;
        let client = HttpClient::builder()
            .timeout_secs(1)
            .proxy_url(format!("http://{USERNAME}:{SECRET}@{proxy_addr}"))
            .build()
            .unwrap();
        let error = client
            .request(
                Method::GET,
                "https://fixture.example.test/".to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("unreachable proxy should fail");
        drop_task.abort();
        let task_error = drop_task
            .await
            .expect_err("connection dropper should be cancelled");

        assert!(task_error.is_cancelled());
        assert!(!error.to_string().contains(SECRET));
        assert!(!error.to_string().contains(&BASE64.encode(SECRET)));
        assert!(
            !error
                .to_string()
                .contains(&BASE64.encode(format!("{USERNAME}:{SECRET}")))
        );
    }

    #[rstest]
    fn test_http_client_with_valid_proxy() {
        // Create client with a valid proxy URL
        let result = HttpClient::builder()
            .proxy_url("http://proxy.example.com:8080".to_string())
            .build();

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_with_socks5_proxy() {
        // Create client with a SOCKS5 proxy URL
        let result = HttpClient::builder()
            .proxy_url("socks5://127.0.0.1:1080".to_string())
            .build();

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_with_malformed_proxy() {
        // Proxy parsing accepts scheme-less hostnames.
        // It only fails on obviously malformed URLs like "://invalid" or "http://".
        // More subtle issues (like "not-a-valid-url") are caught when connecting.
        let result = HttpClient::builder()
            .proxy_url("://invalid".to_string())
            .build();

        assert!(result.is_err());
        assert!(matches!(result, Err(HttpClientError::InvalidProxy(_))));
    }

    #[rstest]
    fn test_http_client_invalid_proxy_error_redacts_credentials() {
        const SECRET: &str = "unique-proxy-secret";
        let result = HttpClient::builder()
            .proxy_url(format!("http://proxytest:{SECRET}@[::1"))
            .build();
        let error = result.expect_err("malformed proxy URL should fail");

        assert_eq!(
            error.to_string(),
            "Invalid proxy URL: proxy URL is malformed"
        );
        assert!(!error.to_string().contains(SECRET));
    }

    #[rstest]
    fn test_http_client_with_empty_proxy_string() {
        // Create client with an empty proxy URL string
        let result = HttpClient::builder().proxy_url(String::new()).build();

        assert!(result.is_err());
        assert!(matches!(result, Err(HttpClientError::InvalidProxy(_))));
    }

    #[tokio::test]
    async fn test_http_client_get() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/get");

        let client = HttpClient::builder().build().unwrap();
        let response = client.get(url, None, None, None, None).await.unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"hello-world!");
    }

    #[tokio::test]
    async fn test_http_client_post() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/post");

        let client = HttpClient::builder().build().unwrap();
        let response = client
            .post(url, None, None, Some(b"post-body-73".to_vec()), None, None)
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"post-body-73");
    }

    #[tokio::test]
    async fn test_http_client_patch() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/patch");

        let client = HttpClient::builder().build().unwrap();
        let response = client
            .patch(url, None, None, Some(b"patch-body-91".to_vec()), None, None)
            .await
            .unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"patch-body-91");
    }

    #[tokio::test]
    async fn test_http_client_delete() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/delete");

        let client = HttpClient::builder().build().unwrap();
        let response = client.delete(url, None, None, None, None).await.unwrap();

        assert_eq!(response.status.as_u16(), StatusCode::OK.as_u16());
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use std::{num::NonZeroU32, sync::Arc, time::Duration};

    #[cfg(all(feature = "simulation", madsim))]
    use madsim::task as test_task;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use tokio::task as test_task;
    use ustr::Ustr;

    use super::HttpClient;
    use crate::ratelimiter::{RateLimiter, quota::Quota};

    #[tokio::test]
    async fn test_http_client_awaits_multiple_rate_limiters() {
        let quota = Quota::per_minute(NonZeroU32::MIN);
        let request_key = Ustr::from("scope:request");
        let order_key = Ustr::from("scope:order");
        let request_limiter = Arc::new(RateLimiter::new_with_quota(
            None,
            vec![(request_key, quota)],
        ));
        let order_limiter = Arc::new(RateLimiter::new_with_quota(None, vec![(order_key, quota)]));
        let client = HttpClient::builder()
            .rate_limiters(vec![
                Arc::clone(&request_limiter),
                Arc::clone(&order_limiter),
            ])
            .build()
            .unwrap();

        client
            .await_rate_limits(Some(&[request_key, order_key]))
            .await;

        assert!(request_limiter.check_key(&request_key).is_err());
        assert!(order_limiter.check_key(&order_key).is_err());
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_http_client_reserves_multiple_rate_limits_together() {
        let global_key = Ustr::from("scope:global");
        let order_key = Ustr::from("scope:order");
        let global_limiter = Arc::new(RateLimiter::new_with_quota(
            None,
            vec![(
                global_key,
                Quota::with_period(Duration::from_secs(1)).unwrap(),
            )],
        ));
        let order_limiter = Arc::new(RateLimiter::new_with_quota(
            None,
            vec![(
                order_key,
                Quota::with_period(Duration::from_secs(10)).unwrap(),
            )],
        ));
        order_limiter.check_key(&order_key).unwrap();

        let client = HttpClient::builder()
            .rate_limiters(vec![
                Arc::clone(&global_limiter),
                Arc::clone(&order_limiter),
            ])
            .build()
            .unwrap();

        let request = test_task::spawn(async move {
            client
                .await_rate_limits(Some(&[global_key, order_key]))
                .await;
        });
        test_task::yield_now().await;

        global_limiter.check_key(&global_key).unwrap();
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(9_999)).await;
        global_limiter.until_key_ready(&global_key).await;
        global_limiter.until_key_ready(&global_key).await;
        advance_test_clock(Duration::from_millis(1)).await;
        test_task::yield_now().await;
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(998)).await;
        test_task::yield_now().await;
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(1)).await;
        request.await.unwrap();

        assert!(global_limiter.check_key(&global_key).is_err());
        assert!(order_limiter.check_key(&order_key).is_err());
    }

    #[cfg(all(feature = "simulation", madsim))]
    async fn advance_test_clock(duration: Duration) {
        madsim::time::advance(duration);
        test_task::yield_now().await;
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    async fn advance_test_clock(duration: Duration) {
        tokio::time::advance(duration).await;
    }
}

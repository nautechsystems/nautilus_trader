// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use nautilus_core::collections::into_ustr_vec;
use reqwest::{
    Method, Response, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use ustr::Ustr;

use super::{HttpClientError, HttpResponse, HttpStatus};
use crate::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};

/// An HTTP client that supports rate limiting and timeouts.
///
/// Built on `reqwest` for async I/O. Allows per-endpoint and default quotas
/// through a rate limiter.
///
/// This struct is designed to handle HTTP requests efficiently, providing
/// support for rate limiting, timeouts, and custom headers. The client is
/// built on top of `reqwest` and can be used for both synchronous and
/// asynchronous HTTP requests.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpClient {
    /// The underlying HTTP client used to make requests.
    pub(crate) client: InnerHttpClient,
    /// The rate limiter to control the request rate.
    pub(crate) rate_limiter: Arc<RateLimiter<Ustr, MonotonicClock>>,
}

impl HttpClient {
    /// Creates a new [`HttpClient`] instance.
    ///
    /// # Errors
    ///
    /// - Returns `InvalidProxy` if the proxy URL is malformed.
    /// - Returns `ClientBuildError` if building the underlying `reqwest::Client` fails.
    pub fn new(
        headers: HashMap<String, String>,
        header_keys: Vec<String>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, HttpClientError> {
        // Build default headers
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            let header_name = HeaderName::from_str(&key)
                .map_err(|e| HttpClientError::Error(format!("Invalid header name '{key}': {e}")))?;
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                HttpClientError::Error(format!("Invalid header value '{value}': {e}"))
            })?;
            header_map.insert(header_name, header_value);
        }

        let mut client_builder = reqwest::Client::builder().default_headers(header_map);
        client_builder = client_builder.tcp_nodelay(true);

        if let Some(timeout_secs) = timeout_secs {
            client_builder = client_builder.timeout(Duration::from_secs(timeout_secs));
        }

        // Configure proxy if provided
        if let Some(proxy_url) = proxy_url {
            let proxy = reqwest::Proxy::all(&proxy_url)
                .map_err(|e| HttpClientError::InvalidProxy(format!("{proxy_url}: {e}")))?;
            client_builder = client_builder.proxy(proxy);
        }

        let client = client_builder
            .build()
            .map_err(|e| HttpClientError::ClientBuildError(e.to_string()))?;

        let client = InnerHttpClient {
            client,
            header_keys: Arc::new(header_keys),
        };

        let keyed_quotas = keyed_quotas
            .into_iter()
            .map(|(key, quota)| (Ustr::from(&key), quota))
            .collect();

        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        Ok(Self {
            client,
            rate_limiter,
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
    #[allow(clippy::too_many_arguments)]
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

    /// Sends an HTTP request with serializable query parameters.
    ///
    /// This method accepts any type implementing `Serialize` for query parameters,
    /// which will be automatically encoded into the URL query string using reqwest's
    /// `.query()` method, avoiding unnecessary HashMap allocations.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    #[allow(clippy::too_many_arguments)]
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
        let rate_limiter = self.rate_limiter.clone();
        rate_limiter.await_keys_ready(keys).await;

        self.client
            .send_request_with_query(method, url, params, headers, body, timeout_secs)
            .await
    }

    /// Sends an HTTP request using pre-interned rate limiter keys.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send the request or the request times out.
    #[allow(clippy::too_many_arguments)]
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
        let rate_limiter = self.rate_limiter.clone();
        rate_limiter.await_keys_ready(keys).await;

        self.client
            .send_request(method, url, params, headers, body, timeout_secs)
            .await
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
/// The client is backed by a [`reqwest::Client`] which keeps connections alive and
/// can be cloned cheaply. The client also has a list of header fields to
/// extract from the response.
///
/// The client returns an [`HttpResponse`]. The client filters only the key value
/// for the give `header_keys`.
#[derive(Clone, Debug)]
pub struct InnerHttpClient {
    pub(crate) client: reqwest::Client,
    pub(crate) header_keys: Arc<Vec<String>>,
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
        let full_url = encode_url_params(&url, params)?;
        self.send_request_internal(method, full_url, None::<&()>, headers, body, timeout_secs)
            .await
    }

    /// Sends an HTTP request with query parameters using reqwest's `.query()` method.
    ///
    /// This method accepts any type implementing `Serialize` for query parameters,
    /// avoiding HashMap conversion overhead.
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
        self.send_request_internal(method, url, query, headers, body, timeout_secs)
            .await
    }

    /// Internal implementation for sending HTTP requests.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    async fn send_request_internal<Q: serde::Serialize>(
        &self,
        method: Method,
        url: String,
        query: Option<&Q>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        let headers = headers.unwrap_or_default();
        let reqwest_url = Url::parse(url.as_str())
            .map_err(|e| HttpClientError::from(format!("URL parse error: {e}")))?;

        let mut header_map = HeaderMap::new();
        for (header_key, header_value) in &headers {
            let key = HeaderName::from_bytes(header_key.as_bytes())
                .map_err(|e| HttpClientError::from(format!("Invalid header name: {e}")))?;
            if let Some(old_value) = header_map.insert(
                key.clone(),
                header_value
                    .parse()
                    .map_err(|e| HttpClientError::from(format!("Invalid header value: {e}")))?,
            ) {
                tracing::trace!("Replaced header '{key}': old={old_value:?}, new={header_value}");
            }
        }

        let mut request_builder = self.client.request(method, reqwest_url).headers(header_map);

        if let Some(q) = query {
            request_builder = request_builder.query(q);
        }

        if let Some(timeout_secs) = timeout_secs {
            request_builder = request_builder.timeout(Duration::new(timeout_secs, 0));
        }

        let request = match body {
            Some(b) => request_builder
                .body(b)
                .build()
                .map_err(HttpClientError::from)?,
            None => request_builder.build().map_err(HttpClientError::from)?,
        };

        tracing::trace!("{request:?}");

        let response = self
            .client
            .execute(request)
            .await
            .map_err(HttpClientError::from)?;

        self.to_response(response).await
    }

    /// Converts a `reqwest::Response` into an `HttpResponse`.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to send request or times out.
    pub async fn to_response(&self, response: Response) -> Result<HttpResponse, HttpClientError> {
        tracing::trace!("{response:?}");

        let headers: HashMap<String, String> = self
            .header_keys
            .iter()
            .filter_map(|key| response.headers().get(key).map(|val| (key, val)))
            .filter_map(|(key, val)| val.to_str().map(|v| (key, v)).ok())
            .map(|(k, v)| (k.clone(), v.to_owned()))
            .collect();
        let status = HttpStatus::new(response.status());
        let body = response.bytes().await.map_err(HttpClientError::from)?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

impl Default for InnerHttpClient {
    /// Creates a new default [`InnerHttpClient`] instance.
    ///
    /// The default client is initialized with an empty list of header keys and a new `reqwest::Client`.
    fn default() -> Self {
        let client = reqwest::Client::new();
        Self {
            client,
            header_keys: Default::default(),
        }
    }
}

/// Helper function to encode URL parameters.
///
/// Takes a base URL and optional query parameters, returning the full URL with encoded query string.
/// Parameters can have multiple values per key (for doseq=True behavior).
/// Preserves existing query strings in the URL by appending with '&' instead of '?'.
fn encode_url_params(
    url: &str,
    params: Option<&HashMap<String, Vec<String>>>,
) -> Result<String, HttpClientError> {
    let Some(params) = params else {
        return Ok(url.to_string());
    };

    // Flatten HashMap<String, Vec<String>> into Vec<(String, String)> for serde_urlencoded
    let pairs: Vec<(String, String)> = params
        .iter()
        .flat_map(|(key, values)| values.iter().map(move |value| (key.clone(), value.clone())))
        .collect();

    if pairs.is_empty() {
        return Ok(url.to_string());
    }

    let query_string = serde_urlencoded::to_string(pairs)
        .map_err(|e| HttpClientError::Error(format!("Failed to encode params: {e}")))?;

    // Check if URL already has a query string
    let separator = if url.contains('?') { '&' } else { '?' };
    Ok(format!("{}{}{}", url, separator, query_string))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::net::{SocketAddr, TcpListener};

    use axum::{
        Router,
        routing::{delete, get, patch, post},
        serve,
    };
    use http::status::StatusCode;
    use rstest::rstest;

    use super::*;

    fn get_unique_port() -> u16 {
        // Create a temporary TcpListener to get an available port
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("Failed to bind temporary TcpListener");
        let port = listener.local_addr().unwrap().port();

        // Close the listener to free up the port
        drop(listener);

        port
    }

    fn create_router() -> Router {
        Router::new()
            .route("/get", get(|| async { "hello-world!" }))
            .route("/post", post(|| async { StatusCode::OK }))
            .route("/patch", patch(|| async { StatusCode::OK }))
            .route("/delete", delete(|| async { StatusCode::OK }))
            .route("/notfound", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "Eventually responded"
                }),
            )
    }

    async fn start_test_server() -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let port = get_unique_port();
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            serve(listener, create_router()).await.unwrap();
        });

        Ok(addr)
    }

    #[tokio::test]
    async fn test_get() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                reqwest::Method::GET,
                format!("{url}/get"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "hello-world!");
    }

    #[tokio::test]
    async fn test_post() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                reqwest::Method::POST,
                format!("{url}/post"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(response.status.is_success());
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
                reqwest::Method::POST,
                format!("{url}/post"),
                None,
                None,
                Some(body_bytes),
                None,
            )
            .await
            .unwrap();

        assert!(response.status.is_success());
    }

    #[tokio::test]
    async fn test_patch() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                reqwest::Method::PATCH,
                format!("{url}/patch"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(response.status.is_success());
    }

    #[tokio::test]
    async fn test_delete() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}");

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                reqwest::Method::DELETE,
                format!("{url}/delete"),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(response.status.is_success());
    }

    #[tokio::test]
    async fn test_not_found() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/notfound");
        let client = InnerHttpClient::default();

        let response = client
            .send_request(reqwest::Method::GET, url, None, None, None, None)
            .await
            .unwrap();

        assert!(response.status.is_client_error());
        assert_eq!(response.status.as_u16(), 404);
    }

    #[tokio::test]
    async fn test_timeout() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/slow");
        let client = InnerHttpClient::default();

        // We'll set a 1-second timeout for a route that sleeps 2 seconds
        let result = client
            .send_request(reqwest::Method::GET, url, None, None, None, Some(1))
            .await;

        match result {
            Err(HttpClientError::TimeoutError(msg)) => {
                println!("Got expected timeout error: {msg}");
            }
            Err(e) => panic!("Expected a timeout error, was: {e:?}"),
            Ok(resp) => panic!("Expected a timeout error, but was a successful response: {resp:?}"),
        }
    }

    #[rstest]
    fn test_http_client_without_proxy() {
        // Create client with no proxy
        let result = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            None,
            None,
            None, // No proxy
        );

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_with_valid_proxy() {
        // Create client with a valid proxy URL
        let result = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            None,
            None,
            Some("http://proxy.example.com:8080".to_string()),
        );

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_with_socks5_proxy() {
        // Create client with a SOCKS5 proxy URL
        let result = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            None,
            None,
            Some("socks5://127.0.0.1:1080".to_string()),
        );

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_http_client_with_malformed_proxy() {
        // Note: reqwest::Proxy::all() is lenient and accepts most strings.
        // It only fails on obviously malformed URLs like "://invalid" or "http://".
        // More subtle issues (like "not-a-valid-url") are caught when connecting.
        let result = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            None,
            None,
            Some("://invalid".to_string()),
        );

        assert!(result.is_err());
        assert!(matches!(result, Err(HttpClientError::InvalidProxy(_))));
    }

    #[rstest]
    fn test_http_client_with_empty_proxy_string() {
        // Create client with an empty proxy URL string
        let result = HttpClient::new(
            HashMap::new(),
            vec![],
            vec![],
            None,
            None,
            Some(String::new()),
        );

        assert!(result.is_err());
        assert!(matches!(result, Err(HttpClientError::InvalidProxy(_))));
    }

    #[tokio::test]
    async fn test_http_client_get() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/get");

        let client = HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).unwrap();
        let response = client.get(url, None, None, None, None).await.unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "hello-world!");
    }

    #[tokio::test]
    async fn test_http_client_post() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/post");

        let client = HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).unwrap();
        let response = client
            .post(url, None, None, None, None, None)
            .await
            .unwrap();

        assert!(response.status.is_success());
    }

    #[tokio::test]
    async fn test_http_client_patch() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/patch");

        let client = HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).unwrap();
        let response = client
            .patch(url, None, None, None, None, None)
            .await
            .unwrap();

        assert!(response.status.is_success());
    }

    #[tokio::test]
    async fn test_http_client_delete() {
        let addr = start_test_server().await.unwrap();
        let url = format!("http://{addr}/delete");

        let client = HttpClient::new(HashMap::new(), vec![], vec![], None, None, None).unwrap();
        let response = client.delete(url, None, None, None, None).await.unwrap();

        assert!(response.status.is_success());
    }
}

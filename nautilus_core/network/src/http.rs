// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! A high-performance HTTP client implementation.

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    error::Error,
    fmt::Display,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use futures_util::{stream, StreamExt};
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes};
use reqwest::{
    header::{HeaderMap, HeaderName},
    Method, Response, Url,
};

use crate::ratelimiter::{clock::MonotonicClock, quota::Quota, RateLimiter};

/// Represents the HTTP methods supported by the `HttpClient`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}

#[allow(clippy::from_over_into)]
impl Into<Method> for HttpMethod {
    fn into(self) -> Method {
        match self {
            Self::GET => Method::GET,
            Self::POST => Method::POST,
            Self::PUT => Method::PUT,
            Self::DELETE => Method::DELETE,
            Self::PATCH => Method::PATCH,
        }
    }
}

/// Represents the response from an HTTP request.
///
/// This struct encapsulates the status, headers, and body of an HTTP response,
/// providing easy access to the key components of the response.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpResponse {
    /// The HTTP status code returned by the server.
    pub status: u16,
    /// The headers returned by the server as a map of key-value pairs.
    pub(crate) headers: HashMap<String, String>,
    /// The body of the response as raw bytes.
    pub(crate) body: Bytes,
}

/// A high-performance HTTP client with rate limiting and timeout capabilities.
///
/// This struct is designed to handle HTTP requests efficiently, providing
/// support for rate limiting, timeouts, and custom headers. The client is
/// built on top of `reqwest` and can be used for both synchronous and
/// asynchronous HTTP requests.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpClient {
    /// The rate limiter to control the request rate.
    pub(crate) rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    /// The underlying HTTP client used to make requests.
    pub(crate) client: InnerHttpClient,
}

/// Represents errors that can occur when using the `HttpClient`.
///
/// This enum provides variants for general HTTP errors and timeout errors,
/// allowing for more granular error handling.
#[derive(thiserror::Error, Debug)]
pub enum HttpClientError {
    #[error("HTTP error occurred: {0}")]
    Error(String),

    #[error("HTTP request timed out: {0}")]
    TimeoutError(String),
}

impl From<reqwest::Error> for HttpClientError {
    fn from(source: reqwest::Error) -> Self {
        if source.is_timeout() {
            Self::TimeoutError(source.to_string())
        } else {
            Self::Error(source.to_string())
        }
    }
}

impl From<String> for HttpClientError {
    fn from(value: String) -> Self {
        Self::Error(value)
    }
}

/// A high-performance `HttpClient` for HTTP requests.
///
/// The client is backed by a hyper Client which keeps connections alive and
/// can be cloned cheaply. The client also has a list of header fields to
/// extract from the response.
///
/// The client returns an [`HttpResponse`]. The client filters only the key value
/// for the give `header_keys`.
#[derive(Clone)]
pub struct InnerHttpClient {
    pub(crate) client: reqwest::Client,
    pub(crate) header_keys: Vec<String>,
}

impl InnerHttpClient {
    /// Sends an HTTP request with the specified method, URL, headers, and body.
    ///
    /// - `method`: The HTTP method to use (e.g., GET, POST).
    /// - `url`: The URL to send the request to.
    /// - `headers`: A map of header key-value pairs to include in the request.
    /// - `body`: An optional body for the request, represented as a byte vector.
    /// - `timeout_secs`: An optional timeout for the request in seconds.
    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        timeout_secs: Option<u64>,
    ) -> Result<HttpResponse, HttpClientError> {
        let reqwest_url = Url::parse(url.as_str())
            .map_err(|e| HttpClientError::from(format!("URL parse error: {}", e)))?;

        let mut header_map = HeaderMap::new();
        for (header_key, header_value) in &headers {
            let key = HeaderName::from_bytes(header_key.as_bytes())
                .map_err(|e| HttpClientError::from(format!("Invalid header name: {}", e)))?;
            let _ = header_map.insert(
                key,
                header_value
                    .parse()
                    .map_err(|e| HttpClientError::from(format!("Invalid header value: {}", e)))?,
            );
        }

        let mut request_builder = self.client.request(method, reqwest_url).headers(header_map);

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
    pub async fn to_response(&self, response: Response) -> Result<HttpResponse, HttpClientError> {
        tracing::trace!("{response:?}");

        let headers: HashMap<String, String> = self
            .header_keys
            .iter()
            .filter_map(|key| response.headers().get(key).map(|val| (key, val)))
            .filter_map(|(key, val)| val.to_str().map(|v| (key, v)).ok())
            .map(|(k, v)| (k.clone(), v.to_owned()))
            .collect();
        let status = response.status().as_u16();
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::net::{SocketAddr, TcpListener};

    use axum::{
        routing::{delete, get, patch, post},
        serve, Router,
    };
    use http::status::StatusCode;

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
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
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
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
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
                HashMap::new(),
                Some(body_bytes),
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
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
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
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
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }
}

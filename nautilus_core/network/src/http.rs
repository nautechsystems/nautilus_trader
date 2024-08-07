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
    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        timeout_sec: Option<u64>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let reqwest_url = Url::parse(url.as_str())?;

        let mut header_map = HeaderMap::new();
        for (header_key, header_value) in &headers {
            let key = HeaderName::from_bytes(header_key.as_bytes())?;
            let _ = header_map.insert(key, header_value.parse().unwrap());
        }

        let request_builder = match timeout_sec {
            Some(timeout_sec) => self
                .client
                .request(method, reqwest_url)
                .headers(header_map)
                .timeout(Duration::new(timeout_sec, 0)),
            None => self.client.request(method, reqwest_url).headers(header_map),
        };

        let request = match body {
            Some(b) => request_builder.body(b).build()?,
            None => request_builder.build()?,
        };

        tracing::trace!("{request:?}");

        let response = self.client.execute(request).await?;
        self.to_response(response).await
    }

    pub async fn to_response(
        &self,
        response: Response,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        tracing::trace!("{response:?}");

        let headers: HashMap<String, String> = self
            .header_keys
            .iter()
            .filter_map(|key| response.headers().get(key).map(|val| (key, val)))
            .filter_map(|(key, val)| val.to_str().map(|v| (key, v)).ok())
            .map(|(k, v)| (k.clone(), v.to_owned()))
            .collect();
        let status = response.status().as_u16();
        let body = response.bytes().await?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

impl Default for InnerHttpClient {
    /// Creates a new default [`InnerHttpClient`] instance.
    fn default() -> Self {
        let client = reqwest::Client::new();
        Self {
            client,
            header_keys: Default::default(),
        }
    }
}

/// HttpResponse contains relevant data from a HTTP request.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpResponse {
    pub status: u16,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Bytes,
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpClient {
    pub(crate) rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    pub(crate) client: InnerHttpClient,
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

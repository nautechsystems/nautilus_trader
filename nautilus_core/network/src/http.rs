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

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::Arc,
};

use futures_util::{stream, StreamExt};
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes};
use reqwest::{
    header::{HeaderMap, HeaderName},
    Method, Response, Url,
};

use crate::ratelimiter::{clock::MonotonicClock, quota::Quota, RateLimiter};

/// Provides a high-performance `HttpClient` for HTTP requests.
///
/// The client is backed by a hyper Client which keeps connections alive and
/// can be cloned cheaply. The client also has a list of header fields to
/// extract from the response.
///
/// The client returns an [`HttpResponse`]. The client filters only the key value
/// for the give `header_keys`.
#[derive(Clone)]
pub struct InnerHttpClient {
    client: reqwest::Client,
    header_keys: Vec<String>,
}

impl InnerHttpClient {
    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let reqwest_url = Url::parse(url.as_str())?;

        let mut header_map = HeaderMap::new();
        for (header_key, header_value) in &headers {
            let key = HeaderName::from_bytes(header_key.as_bytes())?;
            let _ = header_map.insert(key, header_value.parse().unwrap());
        }

        let request_builder = self.client.request(method, reqwest_url).headers(header_map);

        let request = match body {
            Some(b) => request_builder.body(b).build()?,
            None => request_builder.build()?,
        };

        let res = self.client.execute(request).await?;
        self.to_response(res).await
    }

    pub async fn to_response(
        &self,
        res: Response,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let headers: HashMap<String, String> = self
            .header_keys
            .iter()
            .filter_map(|key| res.headers().get(key).map(|val| (key, val)))
            .filter_map(|(key, val)| val.to_str().map(|v| (key, v)).ok())
            .map(|(k, v)| (k.clone(), v.to_owned()))
            .collect();
        let status = res.status().as_u16();
        let bytes = res.bytes().await?;

        Ok(HttpResponse {
            status,
            headers,
            body: bytes.to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[pymethods]
impl HttpMethod {
    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }
}

/// HttpResponse contains relevant data from a HTTP request.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpResponse {
    #[pyo3(get)]
    pub status: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Default for InnerHttpClient {
    fn default() -> Self {
        let client = reqwest::Client::new();
        Self {
            client,
            header_keys: Default::default(),
        }
    }
}

#[pymethods]
impl HttpResponse {
    #[new]
    fn py_new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            headers: Default::default(),
        }
    }

    #[getter]
    fn get_body(&self, py: Python) -> PyResult<Py<PyBytes>> {
        Ok(PyBytes::new(py, &self.body).into())
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct HttpClient {
    rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    client: InnerHttpClient,
}

#[pymethods]
impl HttpClient {
    /// Create a new HttpClient.
    ///
    /// * `header_keys` - The key value pairs for the given `header_keys` are retained from the responses.
    /// * `keyed_quota` - A list of string quota pairs that gives quota for specific key values.
    /// * `default_quota` - The default rate limiting quota for any request.
    /// Default quota is optional and no quota is passthrough.
    #[new]
    #[pyo3(signature = (header_keys = Vec::new(), keyed_quotas = Vec::new(), default_quota = None))]
    #[must_use]
    pub fn py_new(
        header_keys: Vec<String>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Self {
        let client = reqwest::Client::new();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        let client = InnerHttpClient {
            client,
            header_keys,
        };

        Self {
            rate_limiter,
            client,
        }
    }

    /// Send an HTTP request.
    ///
    /// * `method` - The HTTP method to call.
    /// * `url` - The request is sent to this url.
    /// * `headers` - The header key value pairs in the request.
    /// * `body` - The bytes sent in the body of request.
    /// * `keys` - The keys used for rate limiting the request.
    #[pyo3(name = "request")]
    fn py_request<'py>(
        &self,
        method: HttpMethod,
        url: String,
        headers: Option<HashMap<String, String>>,
        body: Option<&'py PyBytes>,
        keys: Option<Vec<String>>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let headers = headers.unwrap_or_default();
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let keys = keys.unwrap_or_default();
        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let method = method.into();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            // Check keys for rate limiting quota
            let tasks = keys.iter().map(|key| rate_limiter.until_key_ready(key));
            stream::iter(tasks)
                .for_each(|key| async move {
                    key.await;
                })
                .await;
            match client.send_request(method, url, headers, body_vec).await {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling response: {e}"
                ))),
            }
        })
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
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }
}
